//! Video optimization: transcoding, compression, and responsive resizing.
//!
//! Processes video files into VP9/WebM variants at multiple height tiers,
//! extracts a poster frame as WebP, and copies the original as a fallback.
//! Uses ffmpeg/ffprobe under the hood; falls back gracefully when unavailable.

use eyre::{Result, WrapErr, bail};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::config::VideoOptimConfig;

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

/// Derive a codec label from a file extension (best-effort, no probing).
fn codec_from_ext(ext: &str) -> String {
    match ext.to_lowercase().as_str() {
        "webm" => "vp9".into(),
        "mp4" | "m4v" | "mov" => "h264".into(),
        "avi" => "h264".into(),
        "mkv" => "h264".into(),
        "ogv" => "theora".into(),
        _ => "unknown".into(),
    }
}

/// Compute the set of output heights from the configured tiers and the
/// actual source height.
///
/// Rules:
/// - Keep only configured heights that are strictly less than `source_height`.
/// - Always include `source_height` itself.
/// - Return sorted descending.
pub fn compute_heights(configured: &[u32], source_height: u32) -> Vec<u32> {
    let mut heights: Vec<u32> = configured
        .iter()
        .copied()
        .filter(|&h| h < source_height)
        .collect();
    heights.push(source_height);
    heights.sort_unstable();
    heights.dedup();
    heights.reverse();
    heights
}

/// Probe the width and height of a video file using ffprobe.
pub async fn probe_dimensions(path: &Path) -> Result<(u32, u32)> {
    let output = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-select_streams",
            "v:0",
        ])
        .arg(path)
        .output()
        .await
        .wrap_err("Failed to run ffprobe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ffprobe failed for {}: {}", path.display(), stderr);
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).wrap_err("Failed to parse ffprobe JSON output")?;

    let stream = json["streams"]
        .as_array()
        .and_then(|s| s.first())
        .ok_or_else(|| eyre::eyre!("No video stream found in {}", path.display()))?;

    let width = stream["width"]
        .as_u64()
        .ok_or_else(|| eyre::eyre!("Missing width in ffprobe output"))? as u32;
    let height = stream["height"]
        .as_u64()
        .ok_or_else(|| eyre::eyre!("Missing height in ffprobe output"))? as u32;

    Ok((width, height))
}

/// Transcode a video to VP9/WebM at the given height tier.
///
/// Uses constant-quality mode (`-crf`).  When `height < source_height` the
/// video is scaled down (keeping aspect ratio via `-2`).
pub async fn transcode_vp9(
    input: &Path,
    output: &Path,
    height: u32,
    crf: u8,
    source_height: u32,
) -> Result<()> {
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.args(["-y", "-i"]);
    cmd.arg(input);
    cmd.args(["-c:v", "libvpx-vp9"]);
    cmd.args(["-crf", &crf.to_string()]);
    cmd.args(["-b:v", "0"]);

    if height < source_height {
        cmd.args(["-vf", &format!("scale=-2:{height}")]);
    }

    cmd.args(["-c:a", "libopus", "-b:a", "128k"]);
    cmd.arg(output);

    let result = cmd
        .output()
        .await
        .wrap_err("Failed to run ffmpeg for VP9 transcode")?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!(
            "ffmpeg VP9 transcode failed for {}: {}",
            input.display(),
            stderr,
        );
    }

    Ok(())
}

/// Extract the first frame of a video as a WebP poster image.
pub async fn extract_poster(input: &Path, output: &Path, quality: u8) -> Result<()> {
    let result = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(input)
        .args(["-vframes", "1", "-f", "image2", "-c:v", "libwebp"])
        .args(["-quality", &quality.to_string()])
        .arg(output)
        .output()
        .await
        .wrap_err("Failed to run ffmpeg for poster extraction")?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!(
            "ffmpeg poster extraction failed for {}: {}",
            input.display(),
            stderr,
        );
    }

    Ok(())
}

/// Write bytes to `path`, creating parent directories as needed.
pub fn write_variant_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create dir {}", parent.display()))?;
    }
    std::fs::write(path, data)
        .wrap_err_with(|| format!("Failed to write video variant {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Main public entry point
// ---------------------------------------------------------------------------

/// Process a single video: transcode to VP9 at multiple heights, extract a
/// poster frame, and copy the original as a fallback.
///
/// `src_path`   — path on disk (e.g. `dist/assets/clip.mp4`).
/// `url_prefix` — URL directory prefix (e.g. `/assets`).
///
/// Returns `VideoVariants` with all generated outputs.
pub async fn optimize_video(
    src_path: &Path,
    url_prefix: &str,
    config: &VideoOptimConfig,
    cache: &VideoCache,
    dist_dir: &Path,
) -> Result<VideoVariants> {
    let src_data = std::fs::read(src_path)
        .wrap_err_with(|| format!("Failed to read video {}", src_path.display()))?;

    let hash = source_hash(&src_data);

    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");

    let stem = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");

    // Probe source dimensions.
    let (original_width, original_height) = probe_dimensions(src_path).await?;

    // Determine output heights.
    let heights = compute_heights(&config.heights, original_height);

    let out_dir = dist_dir.join(url_prefix.trim_start_matches('/'));

    // --- VP9 variants -------------------------------------------------------
    let mut vp9_variants: Vec<VideoVariant> = Vec::new();

    for &h in &heights {
        let variant_filename = format!("{stem}-{h}p-{hash}.webm");
        let cache_key = &variant_filename;
        let out_path = out_dir.join(&variant_filename);
        let variant_url = format!("{url_prefix}/{variant_filename}");

        if let Some(cached) = cache.get(cache_key) {
            write_variant_file(&out_path, &cached)?;
        } else {
            // Transcode via ffmpeg into a temp file, then cache.
            let tmp = cache.temp_path(&format!("tmp-{variant_filename}"));
            transcode_vp9(src_path, &tmp, h, config.quality, original_height).await?;

            let data = std::fs::read(&tmp)
                .wrap_err_with(|| format!("Failed to read transcoded file {}", tmp.display()))?;

            // Clean up temp file (best effort).
            let _ = std::fs::remove_file(&tmp);

            cache.put(cache_key, &data)?;
            write_variant_file(&out_path, &data)?;
        }

        vp9_variants.push(VideoVariant {
            url_path: variant_url,
            height: h,
            mime_type: "video/webm",
            codec: "vp9".into(),
        });
    }

    // Ensure descending order by height.
    vp9_variants.sort_by(|a, b| b.height.cmp(&a.height));

    // --- Poster frame -------------------------------------------------------
    let poster_filename = format!("{stem}-poster-{hash}.webp");
    let poster_cache_key = &poster_filename;
    let poster_out_path = out_dir.join(&poster_filename);
    let poster_url = format!("{url_prefix}/{poster_filename}");

    if let Some(cached) = cache.get(poster_cache_key) {
        write_variant_file(&poster_out_path, &cached)?;
    } else {
        let tmp = cache.temp_path(&format!("tmp-{poster_filename}"));
        extract_poster(src_path, &tmp, config.poster_quality).await?;

        let data = std::fs::read(&tmp)
            .wrap_err_with(|| format!("Failed to read poster file {}", tmp.display()))?;
        let _ = std::fs::remove_file(&tmp);

        cache.put(poster_cache_key, &data)?;
        write_variant_file(&poster_out_path, &data)?;
    }

    // --- Original fallback --------------------------------------------------
    let orig_filename = format!("{stem}-{hash}.{ext}");
    let orig_out_path = out_dir.join(&orig_filename);
    let orig_url = format!("{url_prefix}/{orig_filename}");

    write_variant_file(&orig_out_path, &src_data)?;

    let original = VideoVariant {
        url_path: orig_url,
        height: original_height,
        mime_type: video_mime_type(ext),
        codec: codec_from_ext(ext),
    };

    Ok(VideoVariants {
        original_width,
        original_height,
        vp9: vp9_variants,
        original,
        poster_url,
    })
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

    #[test]
    fn test_compute_heights() {
        // 1080p source
        assert_eq!(
            compute_heights(&[480, 720, 1080], 1080),
            vec![1080, 720, 480],
        );
        // 900p source — 1080 is dropped, 900 added
        assert_eq!(compute_heights(&[480, 720, 1080], 900), vec![900, 720, 480],);
        // 360p source — everything above is dropped
        assert_eq!(compute_heights(&[480, 720, 1080], 360), vec![360],);
    }

    #[tokio::test]
    async fn test_probe_dimensions_bad_path() {
        let result = probe_dimensions(Path::new("/nonexistent/video.mp4")).await;
        assert!(result.is_err());
    }

    // --- Integration test (requires ffmpeg) ---------------------------------

    #[tokio::test]
    async fn test_optimize_video_with_ffmpeg() {
        // Skip if ffmpeg is not available.
        if check_ffmpeg().await.is_none() {
            eprintln!("skipping test_optimize_video_with_ffmpeg: ffmpeg not found");
            return;
        }

        let tmp = tempfile::TempDir::new().unwrap();
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

        // Generate a tiny 160x120 test video (1 second, with audio).
        let test_video = tmp.path().join("test_src.mp4");
        let gen_output = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x120:rate=10",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&test_video)
            .output()
            .await
            .unwrap();
        assert!(
            gen_output.status.success(),
            "Failed to generate test video: {}",
            String::from_utf8_lossy(&gen_output.stderr)
        );

        // Copy the test video into dist/assets/ (mimicking the normal flow).
        let src_path = dist_dir.join("assets/clip.mp4");
        std::fs::copy(&test_video, &src_path).unwrap();

        let config = VideoOptimConfig {
            optimize: true,
            format: "vp9".into(),
            quality: 50,                 // fast, low quality is fine for tests
            heights: vec![60, 120, 480], // 480 > 120, should be dropped
            exclude: vec![],
            poster_quality: 50,
        };

        let cache = VideoCache::open(tmp.path()).unwrap();

        let result = optimize_video(&src_path, "/assets", &config, &cache, &dist_dir)
            .await
            .unwrap();

        // Source is 160x120.
        assert_eq!(result.original_width, 160);
        assert_eq!(result.original_height, 120);

        // Heights: 480 is dropped (> 120), keep 60, add 120 -> [120, 60] descending.
        assert_eq!(
            result.vp9.len(),
            2,
            "Expected 2 VP9 variants, got {:?}",
            result.vp9
        );
        assert_eq!(result.vp9[0].height, 120);
        assert_eq!(result.vp9[1].height, 60);
        for v in &result.vp9 {
            assert_eq!(v.mime_type, "video/webm");
            assert_eq!(v.codec, "vp9");
        }

        // Original fallback.
        assert_eq!(result.original.mime_type, "video/mp4");
        assert!(result.original.url_path.ends_with(".mp4"));

        // Poster exists.
        assert!(result.poster_url.ends_with(".webp"));

        // All files exist on disk.
        for v in &result.vp9 {
            let p = dist_dir.join(v.url_path.trim_start_matches('/'));
            assert!(p.exists(), "Missing VP9 variant: {}", p.display());
        }

        let orig_path = dist_dir.join(result.original.url_path.trim_start_matches('/'));
        assert!(
            orig_path.exists(),
            "Missing original: {}",
            orig_path.display()
        );

        let poster_path = dist_dir.join(result.poster_url.trim_start_matches('/'));
        assert!(
            poster_path.exists(),
            "Missing poster: {}",
            poster_path.display()
        );
    }
}
