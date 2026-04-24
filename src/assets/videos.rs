//! Video optimization: transcoding, compression, and responsive resizing.
//!
//! Processes video files into VP9/WebM variants at multiple height tiers,
//! extracts a poster frame as WebP, and copies the original as a fallback.
//! Uses ffmpeg/ffprobe under the hood; falls back gracefully when unavailable.

use eyre::{Result, WrapErr};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single video variant (one height × one codec).
#[derive(Debug, Clone)]
pub struct VideoVariant {
    /// URL path relative to site root, e.g. `/assets/clip-720p-ab12cd34.webm`.
    pub url_path: String,
    /// Pixel height of this variant.
    pub height: u32,
    /// MIME type, e.g. `video/webm`.
    pub mime_type: &'static str,
    /// Codec label, e.g. `vp9`, `h264`.
    pub codec: String,
}

/// The full set of variants generated for a single source video.
#[derive(Debug, Clone)]
pub struct VideoVariants {
    /// Original video width.
    pub original_width: u32,
    /// Original video height.
    pub original_height: u32,
    /// VP9/WebM variants, sorted by height descending.
    pub vp9: Vec<VideoVariant>,
    /// The original file (copied with hash in filename for cache-busting).
    pub original: VideoVariant,
    /// URL of the extracted poster frame (WebP).
    pub poster_url: String,
}

// ---------------------------------------------------------------------------
// On-disk cache
// ---------------------------------------------------------------------------

/// On-disk cache for optimized videos.
///
/// Lives under `.eigen_cache/videos/`.  Each processed variant is stored
/// with a content-hash filename so that unchanged sources are not
/// reprocessed.
pub struct VideoCache {
    cache_dir: PathBuf,
}

impl VideoCache {
    /// Create (or open) the video cache directory.
    pub fn open(project_root: &Path) -> Result<Self> {
        let cache_dir = project_root.join(".eigen_cache").join("videos");
        std::fs::create_dir_all(&cache_dir).wrap_err_with(|| {
            format!("Failed to create video cache dir {}", cache_dir.display())
        })?;
        Ok(Self { cache_dir })
    }

    /// Return cached bytes for `key`, or `None` on miss.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.cache_dir.join(key);
        std::fs::read(&path).ok()
    }

    /// Store `data` under `key`.
    pub fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let path = self.cache_dir.join(key);
        std::fs::write(&path, data)
            .wrap_err_with(|| format!("Failed to write video cache entry {}", path.display()))?;
        Ok(())
    }

    /// Return a path inside the cache dir suitable for temporary files.
    pub fn temp_path(&self, name: &str) -> PathBuf {
        self.cache_dir.join(name)
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// SHA-256 hash of `data`, truncated to the first 8 bytes (16 hex chars).
fn source_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// Check whether ffmpeg is available.  Returns the first line of
/// `ffmpeg -version` on success, or `None` if the binary is missing.
pub async fn check_ffmpeg() -> Option<String> {
    let output = tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(|l| l.to_string())
}

/// Check whether a path should be excluded from optimization.
pub fn is_excluded(path: &str, exclude_patterns: &[String]) -> bool {
    for pattern in exclude_patterns {
        if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
            if glob_pattern.matches(path) {
                return true;
            }
        }
    }
    false
}

/// Map a video file extension to its MIME type.
pub fn video_mime_type(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "ogv" => "video/ogg",
        _ => "application/octet-stream",
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_hash_deterministic() {
        let data = b"video content bytes";
        let h1 = source_hash(data);
        let h2 = source_hash(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn test_source_hash_different_data() {
        assert_ne!(source_hash(b"aaa"), source_hash(b"bbb"));
    }

    #[test]
    fn test_video_cache_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = VideoCache::open(tmp.path()).unwrap();

        let data = b"cached video bytes";
        cache.put("test-key.webm", data).unwrap();

        let got = cache.get("test-key.webm").unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn test_video_cache_creates_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_dir = tmp.path().join(".eigen_cache").join("videos");
        assert!(!cache_dir.exists());

        let _cache = VideoCache::open(tmp.path()).unwrap();
        assert!(cache_dir.exists());
    }

    #[test]
    fn test_is_excluded() {
        let patterns = vec!["**/*.gif".to_string(), "raw/*".to_string()];
        assert!(is_excluded("assets/anim.gif", &patterns));
        assert!(is_excluded("raw/clip.mp4", &patterns));
        assert!(!is_excluded("assets/clip.mp4", &patterns));
    }

    #[test]
    fn test_video_mime_type() {
        assert_eq!(video_mime_type("mp4"), "video/mp4");
        assert_eq!(video_mime_type("webm"), "video/webm");
        assert_eq!(video_mime_type("mov"), "video/quicktime");
        assert_eq!(video_mime_type("avi"), "video/x-msvideo");
        assert_eq!(video_mime_type("mkv"), "video/x-matroska");
        assert_eq!(video_mime_type("ogv"), "video/ogg");
        assert_eq!(video_mime_type("xyz"), "application/octet-stream");
    }

    #[tokio::test]
    async fn test_check_ffmpeg() {
        let result = check_ffmpeg().await;
        // Passes whether ffmpeg is installed or not.
        if let Some(line) = &result {
            assert!(line.contains("ffmpeg"));
        }
    }
}
