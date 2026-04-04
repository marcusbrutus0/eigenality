//! Disk-persisted cache for data source responses.
//!
//! Each cached response is stored as two sidecar files under `.eigen_cache/data/`:
//! - `<hash>.body` — the raw response bytes
//! - `<hash>.meta` — JSON with cache key, ETag, and Last-Modified
//!
//! On subsequent fetches, conditional HTTP requests (`If-None-Match` /
//! `If-Modified-Since`) can be sent using the stored headers, and a 304
//! response means the body file is still valid.

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Metadata stored alongside each cached data response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCacheMeta {
    /// Cache key: `"GET:<url>"` or `"POST:<url>:<body_hash>"`.
    pub cache_key: String,
    /// HTTP ETag header from the server, if provided.
    pub etag: Option<String>,
    /// HTTP Last-Modified header from the server, if provided.
    pub last_modified: Option<String>,
}

/// Manages the on-disk data source cache.
pub struct DataCache {
    /// Path to `.eigen_cache/data/`.
    cache_dir: PathBuf,
    /// In-memory index: cache_key → metadata.
    index: HashMap<String, DataCacheMeta>,
}

/// Hash a cache key into a stable hex string for use as a filename.
///
/// Uses SHA-256 (not `DefaultHasher`) because the hash must be stable
/// across Rust toolchain versions — the value is persisted on disk.
pub(crate) fn cache_key_hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(key.as_bytes());
    format!("{:x}", digest)
}

impl DataCache {
    /// Open (or create) the data cache for a project.
    ///
    /// Loads all `.meta` files found under `.eigen_cache/data/`.
    /// Malformed files are logged as warnings and skipped.
    pub fn open(project_root: &Path) -> Result<Self> {
        let cache_dir = project_root.join(".eigen_cache").join("data");
        std::fs::create_dir_all(&cache_dir)
            .wrap_err_with(|| format!("Failed to create cache dir {}", cache_dir.display()))?;

        let mut index = HashMap::new();

        for entry in std::fs::read_dir(&cache_dir).wrap_err("Failed to read cache directory")? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("meta") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Err(e) => tracing::warn!("Could not read {}: {}", path.display(), e),
                Ok(content) => match serde_json::from_str::<DataCacheMeta>(&content) {
                    Err(e) => tracing::warn!("Skipping malformed meta {}: {}", path.display(), e),
                    Ok(meta) => {
                        index.insert(meta.cache_key.clone(), meta);
                    }
                },
            }
        }

        Ok(Self { cache_dir, index })
    }

    /// Look up cache metadata by cache key.
    pub fn get(&self, cache_key: &str) -> Option<&DataCacheMeta> {
        self.index.get(cache_key)
    }

    /// Read the cached body bytes for a cache key, or `None` if missing.
    pub fn read(&self, cache_key: &str) -> Option<Vec<u8>> {
        if !self.index.contains_key(cache_key) {
            return None;
        }
        let hash = cache_key_hash(cache_key);
        let body_path = self.cache_dir.join(format!("{}.body", hash));
        std::fs::read(&body_path).ok()
    }

    /// Store a response body and its HTTP caching headers.
    pub fn store(
        &mut self,
        cache_key: &str,
        body: &[u8],
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<()> {
        let hash = cache_key_hash(cache_key);

        let body_path = self.cache_dir.join(format!("{}.body", hash));
        std::fs::write(&body_path, body)
            .wrap_err_with(|| format!("Failed to write body file {}", body_path.display()))?;

        let meta = DataCacheMeta {
            cache_key: cache_key.to_string(),
            etag: etag.map(str::to_string),
            last_modified: last_modified.map(str::to_string),
        };

        let meta_path = self.cache_dir.join(format!("{}.meta", hash));
        let meta_json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(&meta_path, meta_json)
            .wrap_err_with(|| format!("Failed to write meta file {}", meta_path.display()))?;

        self.index.insert(cache_key.to_string(), meta);
        Ok(())
    }

    /// Delete all cached files and clear the in-memory index.
    #[allow(dead_code)]
    pub fn clear(&mut self) -> Result<()> {
        for entry in
            std::fs::read_dir(&self.cache_dir).wrap_err("Failed to read cache directory")?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                std::fs::remove_file(&path)
                    .wrap_err_with(|| format!("Failed to delete {}", path.display()))?;
            }
        }
        self.index.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open(tmp: &TempDir) -> DataCache {
        DataCache::open(tmp.path()).expect("open should succeed")
    }

    #[test]
    fn store_and_read_round_trip() {
        let tmp = TempDir::new().unwrap();
        let mut cache = open(&tmp);

        let key = "GET:https://api.example.com/posts";
        let body = b"[{\"id\":1}]";

        cache
            .store(key, body, Some("\"abc123\""), Some("Mon, 01 Jan 2024 00:00:00 GMT"))
            .expect("store should succeed");

        let meta = cache.get(key).expect("get should return meta");
        assert_eq!(meta.cache_key, key);
        assert_eq!(meta.etag.as_deref(), Some("\"abc123\""));
        assert_eq!(
            meta.last_modified.as_deref(),
            Some("Mon, 01 Jan 2024 00:00:00 GMT")
        );

        let read_body = cache.read(key).expect("read should return body");
        assert_eq!(read_body, body);
    }

    #[test]
    fn read_returns_none_for_unknown_key() {
        let tmp = TempDir::new().unwrap();
        let cache = open(&tmp);
        assert!(cache.read("GET:https://unknown.example.com/data").is_none());
    }

    #[test]
    fn clear_removes_all_entries() {
        let tmp = TempDir::new().unwrap();
        let mut cache = open(&tmp);

        cache.store("GET:https://a.example.com/1", b"data1", None, None).unwrap();
        cache.store("GET:https://a.example.com/2", b"data2", None, None).unwrap();

        cache.clear().expect("clear should succeed");

        assert!(cache.get("GET:https://a.example.com/1").is_none());
        assert!(cache.read("GET:https://a.example.com/1").is_none());
        assert!(cache.get("GET:https://a.example.com/2").is_none());

        // Cache dir should now be empty.
        let entries: Vec<_> = std::fs::read_dir(tmp.path().join(".eigen_cache").join("data"))
            .unwrap()
            .collect();
        assert!(entries.is_empty(), "cache dir should be empty after clear");
    }

    #[test]
    fn reopen_loads_persisted_entries() {
        let tmp = TempDir::new().unwrap();

        let key = "GET:https://api.example.com/items";
        let body = b"persisted";

        {
            let mut cache = open(&tmp);
            cache.store(key, body, Some("\"etag-v1\""), None).unwrap();
        }

        // Re-open and verify persistence.
        let cache = open(&tmp);
        let meta = cache.get(key).expect("meta should survive reopen");
        assert_eq!(meta.etag.as_deref(), Some("\"etag-v1\""));
        let read_body = cache.read(key).expect("body should survive reopen");
        assert_eq!(read_body, body);
    }

    #[test]
    fn corrupted_meta_is_skipped() {
        let tmp = TempDir::new().unwrap();

        // Write a bad .meta file before opening.
        let cache_dir = tmp.path().join(".eigen_cache").join("data");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("badhash.meta"), b"not valid json {{{{").unwrap();

        // open() should succeed and simply skip the malformed file.
        let cache = DataCache::open(tmp.path()).expect("open should succeed despite bad meta");
        assert!(cache.index.is_empty(), "bad meta should not appear in index");
    }

    #[test]
    fn missing_body_returns_none() {
        let tmp = TempDir::new().unwrap();
        let mut cache = open(&tmp);

        let key = "GET:https://api.example.com/gone";
        cache.store(key, b"body", None, None).unwrap();

        // Delete the .body file manually.
        let hash = cache_key_hash(key);
        let body_path = tmp
            .path()
            .join(".eigen_cache")
            .join("data")
            .join(format!("{}.body", hash));
        std::fs::remove_file(&body_path).unwrap();

        // read() should return None gracefully.
        assert!(cache.read(key).is_none());
        // get() for meta still works.
        assert!(cache.get(key).is_some());
    }

    #[test]
    fn store_overwrites_existing_entry() {
        let tmp = TempDir::new().unwrap();
        let mut cache = open(&tmp);

        let key = "GET:https://api.example.com/thing";

        cache.store(key, b"old-body", Some("\"etag-1\""), None).unwrap();
        cache.store(key, b"new-body", Some("\"etag-2\""), None).unwrap();

        let meta = cache.get(key).unwrap();
        assert_eq!(meta.etag.as_deref(), Some("\"etag-2\""));
        assert_eq!(cache.read(key).unwrap(), b"new-body");
    }
}
