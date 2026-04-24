//! Incremental build manifest: per-page SHA-256 hash tracking.
//!
//! Two-tier invalidation:
//! - Tier 1 (global): config, layouts, global data, content manifest hashes.
//!   If any changes, the entire build is full.
//! - Tier 2 (per-page): template body, frontmatter, data hashes.
//!   If unchanged and all output files exist, the page is skipped.
//!
//! The manifest is persisted at `.eigen_cache/build_manifest.json`.

use std::collections::HashMap;
use std::path::Path;

use eyre::{Result, WrapErr};
use hex;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::build::content_hash::AssetManifest;

/// Per-page record stored in the build manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageRecord {
    /// Template path relative to `templates/`, e.g. `"posts/index.html"`.
    pub template_path: String,
    /// SHA-256 of the template body (frontmatter stripped).
    pub template_hash: String,
    /// SHA-256 of the raw frontmatter YAML string.
    pub frontmatter_hash: String,
    /// SHA-256 of each data query result, keyed by query name.
    pub data_hashes: HashMap<String, String>,
    /// Output file paths relative to `dist/`.
    pub output_files: Vec<String>,
    /// URL path for reconstructing `RenderedPage` on skip, e.g. `"/posts/"`.
    pub url_path: String,
    /// Whether the output path is `index.html` (drives sitemap `is_index`).
    pub is_index: bool,
    /// Whether this page came from a dynamic template.
    pub is_dynamic: bool,
}

/// Per-template record for dynamic pages.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DynamicTemplateRecord {
    /// All slugs rendered from this template in the last build.
    pub slugs: Vec<String>,
}

/// Manifest persisted at `.eigen_cache/build_manifest.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildManifest {
    /// Eigen version string; version changes trigger a full rebuild.
    pub eigen_version: String,
    /// SHA-256 of `site.toml`.
    pub config_hash: String,
    /// SHA-256 of all `_`-prefixed layout/partial templates combined.
    pub layout_hash: String,
    /// SHA-256 of all files in `_data/` combined.
    pub global_data_hash: String,
    /// SHA-256 of the content asset manifest (hashed filenames).
    pub content_manifest_hash: String,
    /// Per-page records keyed by URL path.
    pub pages: HashMap<String, PageRecord>,
    /// Per-template slug lists for dynamic pages.
    pub dynamic_templates: HashMap<String, DynamicTemplateRecord>,
}

impl BuildManifest {
    /// Create an empty manifest stamped with the current Eigen version.
    pub fn new_empty() -> Self {
        Self {
            eigen_version: env!("CARGO_PKG_VERSION").to_string(),
            config_hash: String::new(),
            layout_hash: String::new(),
            global_data_hash: String::new(),
            content_manifest_hash: String::new(),
            pages: HashMap::new(),
            dynamic_templates: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Hashing helpers
// ---------------------------------------------------------------------------

/// SHA-256 hash of a string, returned as a lowercase hex string.
pub fn sha256_str(s: &str) -> String {
    sha256_bytes(s.as_bytes())
}

/// SHA-256 hash of bytes, returned as a lowercase hex string.
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Hash `site.toml` (the site config file).
pub fn compute_config_hash(project_root: &Path) -> Result<String> {
    let config_path = project_root.join("site.toml");
    let content = std::fs::read(&config_path)
        .wrap_err_with(|| format!("Failed to read {}", config_path.display()))?;
    Ok(sha256_bytes(&content))
}

/// Hash all `_`-prefixed layout/partial templates under `templates/`.
///
/// Collects `(relative_path, contents)` pairs sorted by path so the hash is
/// deterministic regardless of filesystem ordering. Returns `sha256_str("")`
/// if no such files exist.
pub fn compute_layout_hash(project_root: &Path) -> Result<String> {
    let templates_dir = project_root.join("templates");
    if !templates_dir.is_dir() {
        return Ok(sha256_str(""));
    }

    let mut pairs: Vec<(String, String)> = Vec::new();

    for entry in WalkDir::new(&templates_dir).into_iter() {
        let entry = entry.wrap_err("Failed to read entry while hashing layout templates")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let rel = path
            .strip_prefix(&templates_dir)
            .wrap_err("Layout template outside templates/")?;

        // Only underscore-prefixed files (same set used by environment.rs).
        let is_underscore = rel.components().any(|c| {
            c.as_os_str()
                .to_str()
                .map(|s| s.starts_with('_'))
                .unwrap_or(false)
        });
        if !is_underscore {
            continue;
        }

        let contents = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read layout {}", path.display()))?;
        pairs.push((rel.to_string_lossy().replace('\\', "/"), contents));
    }

    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut combined = String::new();
    for (name, body) in &pairs {
        combined.push_str(name);
        combined.push('\0');
        combined.push_str(body);
        combined.push('\0');
    }

    Ok(sha256_str(&combined))
}

/// Hash all files in `_data/` under the project root.
///
/// Returns `sha256_str("")` if the directory is absent.
pub fn compute_global_data_hash(project_root: &Path) -> Result<String> {
    let data_dir = project_root.join("_data");
    if !data_dir.is_dir() {
        return Ok(sha256_str(""));
    }

    let mut pairs: Vec<(String, String)> = Vec::new();

    for entry in WalkDir::new(&data_dir).into_iter() {
        let entry = entry.wrap_err("Failed to read entry while hashing global data")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(&data_dir)
            .wrap_err("Data file outside _data/")?;
        let contents = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("Failed to read data file {}", path.display()))?;
        pairs.push((rel.to_string_lossy().replace('\\', "/"), contents));
    }

    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut combined = String::new();
    for (name, body) in &pairs {
        combined.push_str(name);
        combined.push('\0');
        combined.push_str(body);
        combined.push('\0');
    }

    Ok(sha256_str(&combined))
}

/// Hash the content asset manifest (hashed filenames for cache busting).
///
/// Uses `entries_sorted()` for a deterministic ordering.
/// Returns `sha256_str("")` for an empty manifest.
pub fn compute_content_manifest_hash(manifest: &AssetManifest) -> String {
    if manifest.is_empty() {
        return sha256_str("");
    }
    let mut combined = String::new();
    for (original, hashed) in manifest.entries_sorted() {
        combined.push_str(original);
        combined.push('\0');
        combined.push_str(hashed);
        combined.push('\0');
    }
    sha256_str(&combined)
}

// ---------------------------------------------------------------------------
// Manifest persistence
// ---------------------------------------------------------------------------

/// Load the build manifest from `.eigen_cache/build_manifest.json`.
///
/// Returns `None` on any I/O or parse error — a missing or corrupt manifest
/// is treated as a first run.
pub fn load_manifest(project_root: &Path) -> Option<BuildManifest> {
    let path = project_root
        .join(".eigen_cache")
        .join("build_manifest.json");
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Persist the build manifest to `.eigen_cache/build_manifest.json`.
///
/// Creates parent directories if absent. Returns an error on write failure
/// (losing the manifest causes the next run to be a full rebuild, which is
/// surprising and wasteful).
pub fn save_manifest(project_root: &Path, manifest: &BuildManifest) -> Result<()> {
    let dir = project_root.join(".eigen_cache");
    std::fs::create_dir_all(&dir).wrap_err("Failed to create .eigen_cache directory")?;
    let path = dir.join("build_manifest.json");
    let json =
        serde_json::to_string_pretty(manifest).wrap_err("Failed to serialize build manifest")?;
    std::fs::write(&path, json)
        .wrap_err_with(|| format!("Failed to write build manifest to {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Invalidation checks
// ---------------------------------------------------------------------------

/// Return `true` if any Tier 1 hash has changed since the previous build.
///
/// Tier 1 changes trigger a full rebuild.
pub fn tier1_changed(
    prev: &BuildManifest,
    eigen_version: &str,
    config_hash: &str,
    layout_hash: &str,
    global_data_hash: &str,
    content_manifest_hash: &str,
) -> bool {
    prev.eigen_version != eigen_version
        || prev.config_hash != config_hash
        || prev.layout_hash != layout_hash
        || prev.global_data_hash != global_data_hash
        || prev.content_manifest_hash != content_manifest_hash
}

/// Return `true` if the page needs to be re-rendered.
///
/// A page is changed if:
/// - no previous record exists, or
/// - any hash differs, or
/// - any expected output file is missing from `dist/`.
pub fn page_changed(
    prev_record: Option<&PageRecord>,
    template_hash: &str,
    frontmatter_hash: &str,
    data_hashes: &HashMap<String, String>,
    output_files: &[String],
    dist_dir: &Path,
) -> bool {
    let Some(rec) = prev_record else {
        return true;
    };

    if rec.template_hash != template_hash
        || rec.frontmatter_hash != frontmatter_hash
        || rec.data_hashes != *data_hashes
    {
        return true;
    }

    for f in output_files {
        if !dist_dir.join(f).exists() {
            return true;
        }
    }

    false
}

/// Delete output files from `dist/` for pages that existed in the previous
/// manifest but are absent from the current one (i.e. the template was deleted
/// or renamed between builds).
///
/// Only runs during incremental builds (when `prev_manifest` is `Some`).
/// Returns the number of files deleted.
pub fn delete_orphan_outputs(
    prev_manifest: &BuildManifest,
    current_manifest: &BuildManifest,
    dist_dir: &Path,
) -> Result<usize> {
    let mut deleted = 0;
    for (url_path, prev_record) in &prev_manifest.pages {
        if current_manifest.pages.contains_key(url_path) {
            continue;
        }
        // This page is no longer being built — delete its output files.
        for rel_file in &prev_record.output_files {
            let abs_path = dist_dir.join(rel_file);
            if abs_path.exists() {
                std::fs::remove_file(&abs_path).wrap_err_with(|| {
                    format!("Failed to delete orphan output {}", abs_path.display())
                })?;
                deleted += 1;
                tracing::debug!("Deleted orphan output: {}", abs_path.display());
            }
        }
    }
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &std::path::Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    // --- sha256_str ---

    #[test]
    fn test_sha256_str_stable() {
        let h1 = sha256_str("hello");
        let h2 = sha256_str("hello");
        let h3 = sha256_str("world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert_eq!(h1.len(), 64); // hex SHA-256 is 64 chars
    }

    // --- compute_layout_hash ---

    #[test]
    fn test_layout_hash_sorted() {
        // Two different temp dirs with same files but potentially different
        // traversal order should produce the same hash.
        let tmp1 = TempDir::new().unwrap();
        write(tmp1.path(), "templates/_base.html", "base");
        write(tmp1.path(), "templates/_partials/nav.html", "nav");
        write(tmp1.path(), "templates/_partials/footer.html", "footer");
        write(tmp1.path(), "templates/index.html", "should be ignored");

        let tmp2 = TempDir::new().unwrap();
        write(tmp2.path(), "templates/_partials/footer.html", "footer");
        write(tmp2.path(), "templates/_partials/nav.html", "nav");
        write(tmp2.path(), "templates/_base.html", "base");

        let h1 = compute_layout_hash(tmp1.path()).unwrap();
        let h2 = compute_layout_hash(tmp2.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_layout_hash_no_templates_dir() {
        let tmp = TempDir::new().unwrap();
        let h = compute_layout_hash(tmp.path()).unwrap();
        assert_eq!(h, sha256_str(""));
    }

    #[test]
    fn test_layout_hash_change_on_edit() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "templates/_base.html", "v1");
        let h1 = compute_layout_hash(tmp.path()).unwrap();
        write(tmp.path(), "templates/_base.html", "v2");
        let h2 = compute_layout_hash(tmp.path()).unwrap();
        assert_ne!(h1, h2);
    }

    // --- compute_global_data_hash ---

    #[test]
    fn test_global_data_hash_sorted() {
        let tmp1 = TempDir::new().unwrap();
        write(tmp1.path(), "_data/authors.yaml", "authors: []");
        write(tmp1.path(), "_data/nav.yaml", "nav: []");

        let tmp2 = TempDir::new().unwrap();
        write(tmp2.path(), "_data/nav.yaml", "nav: []");
        write(tmp2.path(), "_data/authors.yaml", "authors: []");

        let h1 = compute_global_data_hash(tmp1.path()).unwrap();
        let h2 = compute_global_data_hash(tmp2.path()).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_global_data_hash_no_data_dir() {
        let tmp = TempDir::new().unwrap();
        let h = compute_global_data_hash(tmp.path()).unwrap();
        assert_eq!(h, sha256_str(""));
    }

    // --- compute_content_manifest_hash ---

    #[test]
    fn test_content_manifest_hash_deterministic() {
        let mut m = AssetManifest::new();
        m.insert(
            "/css/style.css".to_string(),
            "/css/style.abc123.css".to_string(),
        );
        m.insert("/js/app.js".to_string(), "/js/app.def456.js".to_string());
        m.insert(
            "/img/logo.png".to_string(),
            "/img/logo.ghi789.png".to_string(),
        );

        let h1 = compute_content_manifest_hash(&m);
        let h2 = compute_content_manifest_hash(&m);
        assert_eq!(h1, h2);
        assert_ne!(h1, sha256_str("")); // non-empty manifest has non-empty hash
    }

    #[test]
    fn test_content_manifest_hash_empty() {
        let m = AssetManifest::new();
        assert_eq!(compute_content_manifest_hash(&m), sha256_str(""));
    }

    // --- tier1_changed ---

    fn sample_manifest() -> BuildManifest {
        BuildManifest {
            eigen_version: "1.0.0".to_string(),
            config_hash: "cfg".to_string(),
            layout_hash: "lay".to_string(),
            global_data_hash: "gdata".to_string(),
            content_manifest_hash: "cm".to_string(),
            pages: HashMap::new(),
            dynamic_templates: HashMap::new(),
        }
    }

    #[test]
    fn test_tier1_unchanged() {
        let m = sample_manifest();
        assert!(!tier1_changed(&m, "1.0.0", "cfg", "lay", "gdata", "cm"));
    }

    #[test]
    fn test_tier1_changed_config() {
        let m = sample_manifest();
        assert!(tier1_changed(&m, "1.0.0", "CHANGED", "lay", "gdata", "cm"));
    }

    #[test]
    fn test_tier1_changed_layout() {
        let m = sample_manifest();
        assert!(tier1_changed(&m, "1.0.0", "cfg", "CHANGED", "gdata", "cm"));
    }

    #[test]
    fn test_tier1_changed_version() {
        let m = sample_manifest();
        assert!(tier1_changed(&m, "2.0.0", "cfg", "lay", "gdata", "cm"));
    }

    #[test]
    fn test_tier1_changed_global_data() {
        let m = sample_manifest();
        assert!(tier1_changed(&m, "1.0.0", "cfg", "lay", "CHANGED", "cm"));
    }

    #[test]
    fn test_tier1_changed_content_manifest() {
        let m = sample_manifest();
        assert!(tier1_changed(&m, "1.0.0", "cfg", "lay", "gdata", "CHANGED"));
    }

    // --- page_changed ---

    fn sample_record(output_files: Vec<String>) -> PageRecord {
        PageRecord {
            template_path: "index.html".to_string(),
            template_hash: "thash".to_string(),
            frontmatter_hash: "fmhash".to_string(),
            data_hashes: HashMap::from([("posts".to_string(), "dhash".to_string())]),
            output_files,
            url_path: "/".to_string(),
            is_index: true,
            is_dynamic: false,
        }
    }

    #[test]
    fn test_page_unchanged() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "index.html", "content");

        let rec = sample_record(vec!["index.html".to_string()]);
        let data = HashMap::from([("posts".to_string(), "dhash".to_string())]);
        assert!(!page_changed(
            Some(&rec),
            "thash",
            "fmhash",
            &data,
            &["index.html".to_string()],
            tmp.path(),
        ));
    }

    #[test]
    fn test_page_changed_no_prev_record() {
        let tmp = TempDir::new().unwrap();
        let data = HashMap::new();
        assert!(page_changed(
            None,
            "thash",
            "fmhash",
            &data,
            &[],
            tmp.path()
        ));
    }

    #[test]
    fn test_page_changed_template() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "index.html", "content");
        let rec = sample_record(vec!["index.html".to_string()]);
        let data = HashMap::from([("posts".to_string(), "dhash".to_string())]);
        assert!(page_changed(
            Some(&rec),
            "CHANGED",
            "fmhash",
            &data,
            &["index.html".to_string()],
            tmp.path(),
        ));
    }

    #[test]
    fn test_page_changed_frontmatter() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "index.html", "content");
        let rec = sample_record(vec!["index.html".to_string()]);
        let data = HashMap::from([("posts".to_string(), "dhash".to_string())]);
        assert!(page_changed(
            Some(&rec),
            "thash",
            "CHANGED",
            &data,
            &["index.html".to_string()],
            tmp.path(),
        ));
    }

    #[test]
    fn test_page_changed_data() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "index.html", "content");
        let rec = sample_record(vec!["index.html".to_string()]);
        let data = HashMap::from([("posts".to_string(), "CHANGED".to_string())]);
        assert!(page_changed(
            Some(&rec),
            "thash",
            "fmhash",
            &data,
            &["index.html".to_string()],
            tmp.path(),
        ));
    }

    #[test]
    fn test_page_changed_missing_output() {
        let tmp = TempDir::new().unwrap();
        // Don't write the file — it's "missing".
        let rec = sample_record(vec!["index.html".to_string()]);
        let data = HashMap::from([("posts".to_string(), "dhash".to_string())]);
        assert!(page_changed(
            Some(&rec),
            "thash",
            "fmhash",
            &data,
            &["index.html".to_string()],
            tmp.path(),
        ));
    }

    // --- manifest persistence ---

    #[test]
    fn test_manifest_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let mut m = BuildManifest::new_empty();
        m.config_hash = "config123".to_string();
        m.pages.insert(
            "/".to_string(),
            PageRecord {
                template_path: "index.html".to_string(),
                template_hash: "t".to_string(),
                frontmatter_hash: "f".to_string(),
                data_hashes: HashMap::new(),
                output_files: vec!["index.html".to_string()],
                url_path: "/".to_string(),
                is_index: true,
                is_dynamic: false,
            },
        );

        save_manifest(tmp.path(), &m).unwrap();
        let loaded = load_manifest(tmp.path()).expect("should load manifest");
        assert_eq!(loaded.config_hash, "config123");
        assert!(loaded.pages.contains_key("/"));
        assert_eq!(loaded.eigen_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_manifest_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        assert!(load_manifest(tmp.path()).is_none());
    }

    #[test]
    fn test_manifest_malformed_returns_none() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".eigen_cache");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("build_manifest.json"), "not json!!").unwrap();
        assert!(load_manifest(tmp.path()).is_none());
    }

    // --- delete_orphan_outputs ---

    fn make_page_record(output_files: Vec<String>) -> PageRecord {
        PageRecord {
            template_path: "test.html".to_string(),
            template_hash: "t".to_string(),
            frontmatter_hash: "f".to_string(),
            data_hashes: HashMap::new(),
            output_files,
            url_path: "/test.html".to_string(),
            is_index: false,
            is_dynamic: false,
        }
    }

    #[test]
    fn test_delete_orphan_outputs_removes_missing_page_files() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();

        // Create an output file for the orphaned page.
        write(tmp.path(), "dist/old-page.html", "old content");

        let mut prev = BuildManifest::new_empty();
        prev.pages.insert(
            "/old-page.html".to_string(),
            make_page_record(vec!["old-page.html".to_string()]),
        );

        // Current manifest does NOT include old-page.html.
        let current = BuildManifest::new_empty();

        let deleted = delete_orphan_outputs(&prev, &current, &dist).unwrap();
        assert_eq!(deleted, 1);
        assert!(!dist.join("old-page.html").exists());
    }

    #[test]
    fn test_delete_orphan_outputs_keeps_active_pages() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();

        write(tmp.path(), "dist/active.html", "active content");

        let mut prev = BuildManifest::new_empty();
        prev.pages.insert(
            "/active.html".to_string(),
            make_page_record(vec!["active.html".to_string()]),
        );

        // Current manifest also has active.html — not an orphan.
        let mut current = BuildManifest::new_empty();
        current.pages.insert(
            "/active.html".to_string(),
            make_page_record(vec!["active.html".to_string()]),
        );

        let deleted = delete_orphan_outputs(&prev, &current, &dist).unwrap();
        assert_eq!(deleted, 0);
        assert!(dist.join("active.html").exists());
    }

    #[test]
    fn test_delete_orphan_outputs_missing_file_is_noop() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();

        // Orphan page's file doesn't exist on disk — should not error.
        let mut prev = BuildManifest::new_empty();
        prev.pages.insert(
            "/gone.html".to_string(),
            make_page_record(vec!["gone.html".to_string()]),
        );

        let current = BuildManifest::new_empty();
        let deleted = delete_orphan_outputs(&prev, &current, &dist).unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_orphan_dynamic_slug_removed() {
        // A dynamic template had two slugs in the previous build; the current
        // build only rendered one. The removed slug's output file must be deleted.
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(dist.join("posts/old-post")).unwrap();
        fs::create_dir_all(dist.join("posts/kept-post")).unwrap();
        write(tmp.path(), "dist/posts/old-post/index.html", "old post");
        write(tmp.path(), "dist/posts/kept-post/index.html", "kept post");

        // Previous manifest: both slugs present.
        let mut prev = BuildManifest::new_empty();
        prev.pages.insert(
            "/posts/old-post/index.html".to_string(),
            PageRecord {
                template_path: "posts.html".to_string(),
                template_hash: "t".to_string(),
                frontmatter_hash: "f".to_string(),
                data_hashes: HashMap::new(),
                output_files: vec!["posts/old-post/index.html".to_string()],
                url_path: "/posts/old-post/index.html".to_string(),
                is_index: true,
                is_dynamic: true,
            },
        );
        prev.pages.insert(
            "/posts/kept-post/index.html".to_string(),
            PageRecord {
                template_path: "posts.html".to_string(),
                template_hash: "t".to_string(),
                frontmatter_hash: "f".to_string(),
                data_hashes: HashMap::new(),
                output_files: vec!["posts/kept-post/index.html".to_string()],
                url_path: "/posts/kept-post/index.html".to_string(),
                is_index: true,
                is_dynamic: true,
            },
        );

        // Current manifest: only kept-post was rendered this build.
        let mut current = BuildManifest::new_empty();
        current.pages.insert(
            "/posts/kept-post/index.html".to_string(),
            PageRecord {
                template_path: "posts.html".to_string(),
                template_hash: "t".to_string(),
                frontmatter_hash: "f".to_string(),
                data_hashes: HashMap::new(),
                output_files: vec!["posts/kept-post/index.html".to_string()],
                url_path: "/posts/kept-post/index.html".to_string(),
                is_index: true,
                is_dynamic: true,
            },
        );

        let deleted = delete_orphan_outputs(&prev, &current, &dist).unwrap();
        assert_eq!(deleted, 1);
        assert!(
            !dist.join("posts/old-post/index.html").exists(),
            "removed slug output must be deleted"
        );
        assert!(
            dist.join("posts/kept-post/index.html").exists(),
            "active slug output must be kept"
        );
        assert!(
            !current.pages.contains_key("/posts/old-post/index.html"),
            "removed slug must not be in current manifest"
        );
    }
}
