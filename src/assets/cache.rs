//! HTTP-aware asset cache stored in `.eigen_cache/assets/`.
//!
//! Each cached asset is stored alongside a `.meta` JSON file containing:
//! - Original URL
//! - ETag (if the server provided one)
//! - Last-Modified (if the server provided one)
//! - Local filename in `dist/assets/`
//!
//! On subsequent builds, we send conditional requests (`If-None-Match` /
//! `If-Modified-Since`) and skip re-downloading if the server responds 304.

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Metadata stored alongside each cached asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetCacheMeta {
    /// The original remote URL.
    pub url: String,
    /// The local filename (just the name, not the full path) in `dist/assets/`.
    pub local_filename: String,
    /// HTTP ETag header from the server, if provided.
    pub etag: Option<String>,
    /// HTTP Last-Modified header from the server, if provided.
    pub last_modified: Option<String>,
    /// HTTP Content-Type header from the server, if provided.
    pub content_type: Option<String>,
}

/// Manages the on-disk asset cache.
pub struct AssetCache {
    /// Path to `.eigen_cache/assets/`.
    cache_dir: PathBuf,
    /// In-memory index: URL → cache metadata.
    index: HashMap<String, AssetCacheMeta>,
}

impl AssetCache {
    /// Open (or create) the asset cache for a project.
    pub fn open(project_root: &Path) -> Result<Self> {
        let cache_dir = project_root.join(".eigen_cache").join("assets");
        std::fs::create_dir_all(&cache_dir)
            .wrap_err_with(|| format!("Failed to create cache dir {}", cache_dir.display()))?;

        let mut index = HashMap::new();

        // Load all .meta files into the index.
        if cache_dir.is_dir() {
            for entry in std::fs::read_dir(&cache_dir)
                .wrap_err("Failed to read cache directory")?
            {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("meta") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(meta) = serde_json::from_str::<AssetCacheMeta>(&content) {
                            index.insert(meta.url.clone(), meta);
                        }
                    }
                }
            }
        }

        Ok(Self { cache_dir, index })
    }

    /// Look up cache metadata for a URL.
    pub fn get(&self, url: &str) -> Option<&AssetCacheMeta> {
        self.index.get(url)
    }

    /// Get the path to the cached binary file for a URL.
    pub fn cached_file_path(&self, url: &str) -> Option<PathBuf> {
        self.index.get(url).map(|meta| {
            self.cache_dir.join(&meta.local_filename)
        })
    }

    /// Check if the cached binary file actually exists on disk.
    pub fn has_file(&self, url: &str) -> bool {
        self.cached_file_path(url)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Store a downloaded asset in the cache.
    ///
    /// Returns the final local filename (which may have an extension appended
    /// from `content_type` if the original had none).
    pub fn store(
        &mut self,
        url: &str,
        data: &[u8],
        local_filename: &str,
        etag: Option<String>,
        last_modified: Option<String>,
        content_type: Option<String>,
    ) -> Result<String> {
        // If the filename has no extension, try to derive one from content_type.
        let local_filename: Cow<'_, str> =
            if Path::new(local_filename).extension().is_none() {
                if let Some(ext) = content_type.as_deref().and_then(mime_to_ext) {
                    Cow::Owned(format!("{}.{}", local_filename, ext))
                } else {
                    Cow::Borrowed(local_filename)
                }
            } else {
                Cow::Borrowed(local_filename)
            };

        // Write the binary data.
        let data_path = self.cache_dir.join(local_filename.as_ref());
        std::fs::write(&data_path, data)
            .wrap_err_with(|| format!("Failed to write cached asset {}", data_path.display()))?;

        // Write metadata.
        let meta = AssetCacheMeta {
            url: url.to_string(),
            local_filename: local_filename.to_string(),
            etag,
            last_modified,
            content_type,
        };

        let meta_path = self.cache_dir.join(format!("{}.meta", url_hash(url)));
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(&meta_path, meta_json)
            .wrap_err_with(|| format!("Failed to write cache metadata {}", meta_path.display()))?;

        self.index.insert(url.to_string(), meta);
        Ok(local_filename.into_owned())
    }

    /// Update only the HTTP caching headers for an existing entry (after a 304).
    #[allow(dead_code)]
    pub fn update_headers(
        &mut self,
        url: &str,
        etag: Option<String>,
        last_modified: Option<String>,
    ) -> Result<()> {
        if let Some(meta) = self.index.get_mut(url) {
            if etag.is_some() {
                meta.etag = etag;
            }
            if last_modified.is_some() {
                meta.last_modified = last_modified;
            }

            let meta_path = self.cache_dir.join(format!("{}.meta", url_hash(url)));
            let meta_json = serde_json::to_string_pretty(meta)?;
            std::fs::write(&meta_path, meta_json)?;
        }
        Ok(())
    }

    /// Copy a cached asset file into the dist/assets/ directory.
    pub fn copy_to_dist(&self, url: &str, dist_assets_dir: &Path) -> Result<Option<String>> {
        if let Some(meta) = self.index.get(url) {
            let src = self.cache_dir.join(&meta.local_filename);
            if src.exists() {
                std::fs::create_dir_all(dist_assets_dir)?;
                let dst = dist_assets_dir.join(&meta.local_filename);
                std::fs::copy(&src, &dst)
                    .wrap_err_with(|| {
                        format!("Failed to copy {} → {}", src.display(), dst.display())
                    })?;
                return Ok(Some(meta.local_filename.clone()));
            }
        }
        Ok(None)
    }
}

/// Produce a short hex hash of a URL for use as a cache key filename.
pub fn url_hash(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let result = hasher.finalize();
    // Use first 12 hex chars (6 bytes) — enough to avoid collisions.
    hex_encode(&result[..6])
}

/// Generate a local filename from the URL: `{stem}-{hash}.{ext}`.
pub fn local_filename_for_url(url: &str) -> String {
    let hash = url_hash(url);

    // Try to extract a meaningful filename from the URL path.
    let path_part = url.split('?').next().unwrap_or(url);
    let path_part = path_part.split('#').next().unwrap_or(path_part);

    let last_segment = path_part
        .rsplit('/')
        .next()
        .unwrap_or("asset");

    // Split into stem and extension.
    let (stem, ext) = if let Some(dot_pos) = last_segment.rfind('.') {
        let s = &last_segment[..dot_pos];
        let e = &last_segment[dot_pos + 1..];
        // Sanitize: only keep alphanumeric, hyphen, underscore.
        let s = sanitize_filename_part(s);
        let e = sanitize_filename_part(e);
        if s.is_empty() {
            ("asset".to_string(), e)
        } else {
            (s, e)
        }
    } else {
        (sanitize_filename_part(last_segment), String::new())
    };

    let stem = if stem.is_empty() { "asset".to_string() } else { stem };

    if ext.is_empty() {
        format!("{}-{}", stem, hash)
    } else {
        format!("{}-{}.{}", stem, hash, ext)
    }
}

/// Keep only safe filename characters.
fn sanitize_filename_part(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Map a MIME type to a file extension.
fn mime_to_ext(mime: &str) -> Option<&'static str> {
    match mime {
        "image/svg+xml" => Some("svg"),
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/avif" => Some("avif"),
        "image/ico" | "image/x-icon" => Some("ico"),
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "application/pdf" => Some("pdf"),
        "text/css" => Some("css"),
        "application/javascript" | "text/javascript" => Some("js"),
        "font/woff" => Some("woff"),
        "font/woff2" => Some("woff2"),
        "font/ttf" => Some("ttf"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_hash_deterministic() {
        let h1 = url_hash("https://example.com/photo.jpg");
        let h2 = url_hash("https://example.com/photo.jpg");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 12);
    }

    #[test]
    fn test_url_hash_different_urls() {
        let h1 = url_hash("https://example.com/a.jpg");
        let h2 = url_hash("https://example.com/b.jpg");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_local_filename_basic() {
        let name = local_filename_for_url("https://example.com/images/photo.jpg");
        assert!(name.starts_with("photo-"));
        assert!(name.ends_with(".jpg"));
        assert!(name.len() > "photo-.jpg".len()); // has hash in middle
    }

    #[test]
    fn test_local_filename_no_extension() {
        let name = local_filename_for_url("https://example.com/images/photo");
        assert!(name.starts_with("photo-"));
        assert!(!name.contains('.'));
    }

    #[test]
    fn test_local_filename_query_string_stripped() {
        let name = local_filename_for_url("https://example.com/photo.png?w=100&h=200");
        assert!(name.ends_with(".png"));
        assert!(name.starts_with("photo-"));
    }

    #[test]
    fn test_local_filename_special_chars() {
        let name = local_filename_for_url("https://example.com/my%20photo!@#.jpg");
        // Should only contain safe chars.
        assert!(name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.'));
    }

    #[test]
    fn test_sanitize_filename_part() {
        assert_eq!(sanitize_filename_part("hello-world_1"), "hello-world_1");
        assert_eq!(sanitize_filename_part("hello world!"), "helloworld");
        assert_eq!(sanitize_filename_part(""), "");
    }

    // ── mime_to_ext ──────────────────────────────────────────────────────────

    #[test]
    fn test_mime_to_ext_known_types() {
        assert_eq!(mime_to_ext("image/svg+xml"), Some("svg"));
        assert_eq!(mime_to_ext("image/png"), Some("png"));
        assert_eq!(mime_to_ext("image/jpeg"), Some("jpg"));
        assert_eq!(mime_to_ext("image/jpg"), Some("jpg"));
        assert_eq!(mime_to_ext("image/gif"), Some("gif"));
        assert_eq!(mime_to_ext("image/webp"), Some("webp"));
        assert_eq!(mime_to_ext("image/avif"), Some("avif"));
        assert_eq!(mime_to_ext("video/mp4"), Some("mp4"));
        assert_eq!(mime_to_ext("font/woff2"), Some("woff2"));
        assert_eq!(mime_to_ext("text/css"), Some("css"));
        assert_eq!(mime_to_ext("application/javascript"), Some("js"));
        assert_eq!(mime_to_ext("text/javascript"), Some("js"));
    }

    #[test]
    fn test_mime_to_ext_unknown_returns_none() {
        assert_eq!(mime_to_ext("application/octet-stream"), None);
        assert_eq!(mime_to_ext("text/html"), None);
        assert_eq!(mime_to_ext(""), None);
    }

    // ── AssetCache::store — extension derivation ─────────────────────────────

    #[test]
    fn test_store_appends_ext_from_content_type_when_no_ext() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cache = AssetCache::open(tmp.path()).unwrap();

        let url = "https://cms.example.com/uploads/file/abc123";
        // Filename has no extension (simulates substrukt hash-only URL).
        let filename = "abc123-deadbeef";
        cache.store(url, b"<svg/>", filename, None, None, Some("image/svg+xml".to_string())).unwrap();

        let meta = cache.get(url).unwrap();
        assert_eq!(meta.local_filename, "abc123-deadbeef.svg");
        assert_eq!(meta.content_type.as_deref(), Some("image/svg+xml"));

        assert!(cache.has_file(url));
    }

    #[test]
    fn test_store_appends_png_ext_from_content_type() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cache = AssetCache::open(tmp.path()).unwrap();

        let url = "https://cms.example.com/uploads/file/deadbeef";
        let filename = "deadbeef-cafebabe";
        cache.store(url, b"\x89PNG\r\n", filename, None, None, Some("image/png".to_string())).unwrap();

        let meta = cache.get(url).unwrap();
        assert_eq!(meta.local_filename, "deadbeef-cafebabe.png");
    }

    #[test]
    fn test_store_does_not_double_append_ext_when_already_present() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cache = AssetCache::open(tmp.path()).unwrap();

        let url = "https://cms.example.com/images/photo.png";
        // URL already has extension so local_filename_for_url would produce "photo-<hash>.png"
        let filename = "photo-deadbeef.png";
        cache.store(url, b"\x89PNG\r\n", filename, None, None, Some("image/png".to_string())).unwrap();

        let meta = cache.get(url).unwrap();
        // Should not become "photo-deadbeef.png.png"
        assert_eq!(meta.local_filename, "photo-deadbeef.png");
    }

    #[test]
    fn test_store_no_ext_appended_when_content_type_unknown() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cache = AssetCache::open(tmp.path()).unwrap();

        let url = "https://cms.example.com/uploads/file/xyz";
        let filename = "xyz-aabbccdd";
        cache.store(url, b"data", filename, None, None, Some("application/octet-stream".to_string())).unwrap();

        let meta = cache.get(url).unwrap();
        // Unknown MIME — no extension appended.
        assert_eq!(meta.local_filename, "xyz-aabbccdd");
    }

    #[test]
    fn test_store_no_ext_appended_when_content_type_none() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cache = AssetCache::open(tmp.path()).unwrap();

        let url = "https://cms.example.com/uploads/file/notype";
        let filename = "notype-11223344";
        cache.store(url, b"data", filename, None, None, None).unwrap();

        let meta = cache.get(url).unwrap();
        assert_eq!(meta.local_filename, "notype-11223344");
    }

    #[test]
    fn test_store_returns_final_filename_with_extension() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cache = AssetCache::open(tmp.path()).unwrap();

        let url = "https://cms.example.com/uploads/file/logosvg";
        let filename = "logosvg-aabbccdd"; // no ext
        let returned = cache.store(url, b"<svg/>", filename, None, None, Some("image/svg+xml".to_string())).unwrap();

        assert_eq!(returned, "logosvg-aabbccdd.svg");
    }
}
