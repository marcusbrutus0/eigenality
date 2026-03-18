//! Content hashing for static assets.
//!
//! Fingerprints files copied from `static/` to `dist/` by embedding a
//! content hash in the filename (e.g. `style.css` -> `style.a1b2c3d4e5f67890.css`).
//! This enables immutable caching (`Cache-Control: max-age=31536000, immutable`).
//!
//! Three phases:
//! - Phase 1 (`build_manifest`): hash and rename files in dist, build manifest.
//! - Phase 2: `asset()` template function uses manifest for render-time resolution.
//! - Phase 3 (`rewrite_references`): post-render string replacement in HTML/CSS/JS.

use eyre::{Result, WrapErr};
use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use crate::config::ContentHashConfig;

/// Mapping from original asset URL paths to content-hashed URL paths.
///
/// Built during `copy_static_assets` when content hashing is enabled.
/// Shared across the build pipeline via `Arc`.
///
/// All paths are URL paths relative to site root with a leading slash,
/// e.g. `/css/style.css` -> `/css/style.a1b2c3d4e5f67890.css`.
pub struct AssetManifest {
    /// Forward map: original path -> hashed path.
    entries: HashMap<String, String>,
}

impl AssetManifest {
    /// Create a new empty manifest.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Create with pre-allocated capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(cap),
        }
    }

    /// Insert a mapping.
    pub fn insert(&mut self, original: String, hashed: String) {
        self.entries.insert(original, hashed);
    }

    /// Look up the hashed path for an original path.
    /// Returns the original path unchanged if not found in the manifest.
    pub fn resolve<'a>(&'a self, original: &'a str) -> &'a str {
        self.entries
            .get(original)
            .map(|s| s.as_str())
            .unwrap_or(original)
    }

    /// Return all (original, hashed) pairs sorted by original path
    /// length descending.
    ///
    /// Longest-match-first ordering prevents partial replacements during
    /// string rewriting.
    pub fn pairs_longest_first(&self) -> Vec<(&str, &str)> {
        let mut pairs: Vec<(&str, &str)> = self
            .entries
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        pairs
    }

    /// Whether the manifest is empty (no assets were hashed).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Compute the content hash for a file's bytes.
///
/// Returns a 16-character lowercase hex string (SHA-256 truncated to 8
/// bytes). This matches the hash length used by image optimization.
fn content_hash(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hex = String::with_capacity(16);
    for b in &result[..8] {
        // write! to a String never fails.
        let _ = write!(hex, "{:02x}", b);
    }
    hex
}

/// Construct a hashed filename from the original filename and hash.
///
/// Inserts the hash between the stem and extension:
///   `style.css` + `a1b2c3d4` -> `style.a1b2c3d4.css`
///   `app.min.js` + `a1b2c3d4` -> `app.min.a1b2c3d4.js`
///
/// Files without an extension get the hash appended:
///   `LICENSE` + `a1b2c3d4` -> `LICENSE.a1b2c3d4`
fn hashed_filename(original_filename: &str, hash: &str) -> String {
    match original_filename.rfind('.') {
        Some(dot_pos) => {
            let stem = &original_filename[..dot_pos];
            let ext = &original_filename[dot_pos + 1..];
            format!("{stem}.{hash}.{ext}")
        }
        None => {
            format!("{original_filename}.{hash}")
        }
    }
}

/// Check whether a file should be excluded from content hashing.
///
/// Uses `glob::Pattern` matching against the relative path from `static/`.
fn is_excluded(relative_path: &str, exclude_patterns: &[String]) -> bool {
    for pattern_str in exclude_patterns {
        match glob::Pattern::new(pattern_str) {
            Ok(pattern) => {
                if pattern.matches(relative_path) {
                    return true;
                }
            }
            Err(e) => {
                tracing::warn!("Invalid exclude pattern '{}': {}", pattern_str, e);
            }
        }
    }
    false
}

/// Build the asset manifest by hashing and renaming static assets in
/// dist_dir.
///
/// Walks all files in dist_dir that correspond to files from static_dir,
/// computes content hashes, renames files to include the hash, and
/// returns the manifest.
///
/// Files matching the exclude patterns are left at their original names.
pub fn build_manifest(
    _dist_dir: &Path,
    _static_dir: &Path,
    _config: &ContentHashConfig,
) -> Result<AssetManifest> {
    // Implemented in Task 3.
    Ok(AssetManifest::new())
}

/// Rewrite all references to static assets in rendered HTML, CSS, and
/// JS files in dist_dir.
///
/// Walks all `.html`, `.css`, and `.js` files in dist_dir and replaces
/// occurrences of original asset paths with their hashed equivalents.
///
/// Fragment HTML files in `_fragments/` ARE rewritten.
pub fn rewrite_references(
    _dist_dir: &Path,
    _manifest: &AssetManifest,
) -> Result<()> {
    // Implemented in Task 5.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- content_hash ---

    #[test]
    fn test_content_hash_deterministic() {
        let data = b"hello world";
        let h1 = content_hash(data);
        let h2 = content_hash(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_content_hash_different_data() {
        let h1 = content_hash(b"hello");
        let h2 = content_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_content_hash_length() {
        let h = content_hash(b"test data");
        assert_eq!(h.len(), 16, "hash should be 16 hex chars (8 bytes)");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // --- hashed_filename ---

    #[test]
    fn test_hashed_filename_with_ext() {
        let result = hashed_filename("style.css", "a1b2c3d4");
        assert_eq!(result, "style.a1b2c3d4.css");
    }

    #[test]
    fn test_hashed_filename_multi_dot() {
        let result = hashed_filename("app.min.js", "a1b2c3d4");
        assert_eq!(result, "app.min.a1b2c3d4.js");
    }

    #[test]
    fn test_hashed_filename_no_ext() {
        let result = hashed_filename("LICENSE", "a1b2c3d4");
        assert_eq!(result, "LICENSE.a1b2c3d4");
    }

    #[test]
    fn test_hashed_filename_dotfile() {
        let result = hashed_filename(".htaccess", "a1b2c3d4");
        assert_eq!(result, ".a1b2c3d4.htaccess");
    }

    // --- is_excluded ---

    #[test]
    fn test_is_excluded_default_patterns() {
        let patterns = vec![
            "favicon.ico".into(),
            "robots.txt".into(),
            "CNAME".into(),
            "_headers".into(),
            "_redirects".into(),
            ".well-known/**".into(),
        ];
        assert!(is_excluded("favicon.ico", &patterns));
        assert!(is_excluded("robots.txt", &patterns));
        assert!(is_excluded("CNAME", &patterns));
        assert!(is_excluded("_headers", &patterns));
        assert!(is_excluded("_redirects", &patterns));
    }

    #[test]
    fn test_is_excluded_glob_wellknown() {
        let patterns = vec![".well-known/**".into()];
        assert!(is_excluded(".well-known/security.txt", &patterns));
        assert!(is_excluded(".well-known/assetlinks.json", &patterns));
    }

    #[test]
    fn test_is_excluded_no_match() {
        let patterns = vec!["favicon.ico".into(), "robots.txt".into()];
        assert!(!is_excluded("css/style.css", &patterns));
        assert!(!is_excluded("js/app.js", &patterns));
        assert!(!is_excluded("images/logo.png", &patterns));
    }

    // --- AssetManifest ---

    #[test]
    fn test_manifest_resolve_found() {
        let mut m = AssetManifest::new();
        m.insert("/css/style.css".into(), "/css/style.abc123.css".into());
        assert_eq!(m.resolve("/css/style.css"), "/css/style.abc123.css");
    }

    #[test]
    fn test_manifest_resolve_not_found() {
        let m = AssetManifest::new();
        assert_eq!(m.resolve("/unknown.css"), "/unknown.css");
    }

    #[test]
    fn test_manifest_pairs_longest_first() {
        let mut m = AssetManifest::new();
        m.insert("/a.css".into(), "/a.h1.css".into());
        m.insert("/css/style.css".into(), "/css/style.h2.css".into());
        m.insert("/css/b.css".into(), "/css/b.h3.css".into());

        let pairs = m.pairs_longest_first();
        // Longest original path first.
        assert_eq!(pairs[0].0, "/css/style.css");
        assert!(pairs[0].0.len() >= pairs[1].0.len());
        assert!(pairs[1].0.len() >= pairs[2].0.len());
    }

    #[test]
    fn test_manifest_empty() {
        let m = AssetManifest::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn test_manifest_len() {
        let mut m = AssetManifest::new();
        m.insert("/a.css".into(), "/a.h.css".into());
        m.insert("/b.js".into(), "/b.h.js".into());
        assert!(!m.is_empty());
        assert_eq!(m.len(), 2);
    }
}
