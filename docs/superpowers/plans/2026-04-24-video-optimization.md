# Video Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add automatic video transcoding (VP9/WebM), poster extraction, and HTML rewriting to eigen's build pipeline via ffmpeg CLI subprocess.

**Architecture:** A new `videos` module parallel to `images` handles ffmpeg detection, transcoding, caching, and poster extraction. A new `video_rewrite` module handles HTML collection and rewriting with `lol_html`. Both integrate into the existing render pipeline after image optimization. No new Cargo dependencies.

**Tech Stack:** Rust, tokio::process::Command (ffmpeg/ffprobe CLI), lol_html (HTML rewriting), sha2 (caching), serde_json (ffprobe output parsing)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `flake.nix` | Modify | Add `ffmpeg` to nix dev shell packages |
| `src/config/mod.rs` | Modify | Add `VideoOptimConfig` struct and wire into `AssetsConfig` |
| `src/assets/videos.rs` | Create | FFmpeg detection, `VideoCache`, `VideoVariant`, `VideoVariants`, `optimize_video()`, poster extraction |
| `src/assets/video_rewrite.rs` | Create | `optimize_and_rewrite_videos()` — HTML collection and rewriting with `lol_html` |
| `src/assets/mod.rs` | Modify | Register new modules and re-export public API |
| `src/build/render.rs` | Modify | Add `VideoCache` + `ffmpeg_available` to `BuildContext`, insert video optimization step in pipeline |
| `docs/video_optimization.md` | Create | Feature documentation |

---

### Task 1: Add ffmpeg to nix flake

**Files:**
- Modify: `flake.nix:31-39`

- [ ] **Step 1: Add ffmpeg to packages list**

In `flake.nix`, add `ffmpeg` to the `packages` list inside `mkShell`:

```nix
packages = [
  nil
  just
  cargo-expand
  bacon
  dolt
  cargo-dist
  uv
  ffmpeg
];
```

- [ ] **Step 2: Verify ffmpeg is available**

Run: `direnv reload && ffmpeg -version`
Expected: ffmpeg version output (e.g., `ffmpeg version 7.x ...`)

- [ ] **Step 3: Commit**

```bash
git add flake.nix
git commit -m "chore: add ffmpeg to nix dev shell"
```

---

### Task 2: Add VideoOptimConfig to config

**Files:**
- Modify: `src/config/mod.rs:357-443`

- [ ] **Step 1: Write tests for VideoOptimConfig defaults**

Add at the bottom of the existing `#[cfg(test)] mod tests` block in `src/config/mod.rs` (or if there isn't one, create it):

```rust
#[cfg(test)]
mod video_config_tests {
    use super::*;

    #[test]
    fn test_video_optim_config_defaults() {
        let config = VideoOptimConfig::default();
        assert!(config.optimize);
        assert_eq!(config.format, "vp9");
        assert_eq!(config.quality, 30);
        assert_eq!(config.heights, vec![480, 720, 1080]);
        assert!(config.exclude.is_empty());
        assert_eq!(config.poster_quality, 80);
    }

    #[test]
    fn test_assets_config_has_videos() {
        let config = AssetsConfig::default();
        assert!(config.videos.optimize);
        assert_eq!(config.videos.format, "vp9");
    }

    #[test]
    fn test_video_config_deserialize_partial() {
        let toml_str = r#"
            optimize = false
            quality = 25
        "#;
        let config: VideoOptimConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.optimize);
        assert_eq!(config.quality, 25);
        // Defaults for unspecified fields.
        assert_eq!(config.format, "vp9");
        assert_eq!(config.heights, vec![480, 720, 1080]);
        assert_eq!(config.poster_quality, 80);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test video_config_tests`
Expected: FAIL — `VideoOptimConfig` does not exist yet.

- [ ] **Step 3: Add VideoOptimConfig struct and defaults**

In `src/config/mod.rs`, after the `default_image_exclude` function (line ~443), add:

```rust
/// Video optimization configuration.
///
/// Controls transcoding, compression, and resolution tiers for video
/// assets. Requires `ffmpeg` on PATH at build time.
#[derive(Debug, Clone, Deserialize)]
pub struct VideoOptimConfig {
    /// Master switch — set to `false` to disable all video optimization.
    #[serde(default = "default_true")]
    pub optimize: bool,
    /// Target codec. Currently only `"vp9"` is supported.
    #[serde(default = "default_video_format")]
    pub format: String,
    /// CRF quality value for VP9 (0–63, lower = better quality).
    #[serde(default = "default_video_quality")]
    pub quality: u8,
    /// Resolution tiers (heights in pixels) to generate.
    /// Tiers above the source resolution are skipped.
    #[serde(default = "default_video_heights")]
    pub heights: Vec<u32>,
    /// Glob patterns for video paths to exclude from optimization.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// WebP quality (1–100) for extracted poster frames.
    #[serde(default = "default_poster_quality")]
    pub poster_quality: u8,
}

impl Default for VideoOptimConfig {
    fn default() -> Self {
        Self {
            optimize: true,
            format: default_video_format(),
            quality: default_video_quality(),
            heights: default_video_heights(),
            exclude: Vec::new(),
            poster_quality: default_poster_quality(),
        }
    }
}

fn default_video_format() -> String {
    "vp9".to_string()
}

fn default_video_quality() -> u8 {
    30
}

fn default_video_heights() -> Vec<u32> {
    vec![480, 720, 1080]
}

fn default_poster_quality() -> u8 {
    80
}
```

Then add the `videos` field to `AssetsConfig` (line ~371):

```rust
pub struct AssetsConfig {
    #[serde(default = "default_true")]
    pub localize: bool,
    #[serde(default)]
    pub cdn_skip_hosts: Vec<String>,
    #[serde(default)]
    pub cdn_allow_hosts: Vec<String>,
    #[serde(default)]
    pub images: ImageOptimConfig,
    /// Video optimization configuration.
    #[serde(default)]
    pub videos: VideoOptimConfig,
}
```

And update `AssetsConfig::default()`:

```rust
impl Default for AssetsConfig {
    fn default() -> Self {
        Self {
            localize: true,
            cdn_skip_hosts: Vec::new(),
            cdn_allow_hosts: Vec::new(),
            images: ImageOptimConfig::default(),
            videos: VideoOptimConfig::default(),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test video_config_tests`
Expected: All 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat: add VideoOptimConfig to site config"
```

---

### Task 3: Create videos.rs — VideoCache, data structures, ffmpeg detection

**Files:**
- Create: `src/assets/videos.rs`

- [ ] **Step 1: Write tests for VideoCache and helpers**

Create `src/assets/videos.rs` with the test module first:

```rust
//! Video optimization: transcoding, compression, and multi-resolution encoding.
//!
//! Uses `ffmpeg` and `ffprobe` CLI tools (must be on PATH) to transcode
//! video files to VP9/WebM, extract poster frames, and generate multiple
//! resolution tiers.

use eyre::{Result, WrapErr, bail};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::config::VideoOptimConfig;

/// Describes a single generated video variant.
#[derive(Debug, Clone)]
pub struct VideoVariant {
    /// URL path relative to site root, e.g. `/assets/demo-720p.webm`.
    pub url_path: String,
    /// The pixel height of this variant.
    pub height: u32,
    /// MIME type, e.g. `video/webm`.
    pub mime_type: String,
    /// Codec name, e.g. `vp9`.
    pub codec: String,
}

/// The full set of variants generated for a single source video.
#[derive(Debug, Clone)]
pub struct VideoVariants {
    /// Original video width.
    pub original_width: u32,
    /// Original video height.
    pub original_height: u32,
    /// VP9/WebM variants sorted by height descending (for HTML source order).
    pub vp9: Vec<VideoVariant>,
    /// Original file as fallback.
    pub original: VideoVariant,
    /// URL path to the poster WebP image.
    pub poster_url: String,
}

/// On-disk cache for transcoded video files.
///
/// Lives under `.eigen_cache/videos/`.
pub struct VideoCache {
    cache_dir: PathBuf,
}

impl VideoCache {
    pub fn open(project_root: &Path) -> Result<Self> {
        let cache_dir = project_root.join(".eigen_cache").join("videos");
        std::fs::create_dir_all(&cache_dir)
            .wrap_err_with(|| format!("Failed to create video cache dir {}", cache_dir.display()))?;
        Ok(Self { cache_dir })
    }

    fn variant_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(key)
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.variant_path(key);
        std::fs::read(&path).ok()
    }

    pub fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let path = self.variant_path(key);
        std::fs::write(&path, data)
            .wrap_err_with(|| format!("Failed to write video cache entry {}", path.display()))?;
        Ok(())
    }
}

/// Hash source video bytes to create a stable cache key prefix.
fn source_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result[..8]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

/// Check whether `ffmpeg` is available on PATH.
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
    let version_line = stdout.lines().next().unwrap_or("ffmpeg");
    Some(version_line.to_string())
}

/// Check whether a video path should be excluded from optimization.
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

/// Guess the MIME type from a video file extension.
fn video_mime_type(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mkv" => "video/x-matroska",
        "ogv" => "video/ogg",
        _ => "video/mp4",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_hash_deterministic() {
        let data = b"video data";
        assert_eq!(source_hash(data), source_hash(data));
        assert_eq!(source_hash(data).len(), 16);
    }

    #[test]
    fn test_source_hash_different_data() {
        assert_ne!(source_hash(b"video1"), source_hash(b"video2"));
    }

    #[test]
    fn test_video_cache_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = VideoCache::open(tmp.path()).unwrap();

        assert!(cache.get("nonexistent").is_none());

        cache.put("test-720p-vp9.webm", b"fake video data").unwrap();
        let data = cache.get("test-720p-vp9.webm").unwrap();
        assert_eq!(data, b"fake video data");
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
        let patterns = vec!["**/*.gif".to_string(), "promo/*".to_string()];
        assert!(is_excluded("videos/anim.gif", &patterns));
        assert!(is_excluded("promo/intro.mp4", &patterns));
        assert!(!is_excluded("videos/demo.mp4", &patterns));
    }

    #[test]
    fn test_video_mime_type() {
        assert_eq!(video_mime_type("mp4"), "video/mp4");
        assert_eq!(video_mime_type("webm"), "video/webm");
        assert_eq!(video_mime_type("mov"), "video/quicktime");
        assert_eq!(video_mime_type("MP4"), "video/mp4");
    }

    #[tokio::test]
    async fn test_check_ffmpeg() {
        // This test passes if ffmpeg is on PATH (nix shell), returns Some.
        // If not installed, returns None — test still passes either way.
        let result = check_ffmpeg().await;
        if let Some(ref version) = result {
            assert!(version.contains("ffmpeg"));
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib assets::videos::tests`
Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/assets/videos.rs
git commit -m "feat: add VideoCache, data structures, and ffmpeg detection"
```

---

### Task 4: Add ffprobe and transcode functions to videos.rs

**Files:**
- Modify: `src/assets/videos.rs`

- [ ] **Step 1: Write tests for probe_dimensions and compute_heights**

Add to the `tests` module in `src/assets/videos.rs`:

```rust
#[test]
fn test_compute_heights() {
    // Source is 1080p — all tiers below + source height included.
    let configured = vec![480, 720, 1080];
    let heights = compute_heights(&configured, 1080);
    assert_eq!(heights, vec![1080, 720, 480]);

    // Source is 900p — 1080 skipped, source height added.
    let heights = compute_heights(&configured, 900);
    assert_eq!(heights, vec![900, 720, 480]);

    // Source is 360p — all configured tiers skipped, only source height.
    let heights = compute_heights(&configured, 360);
    assert_eq!(heights, vec![360]);
}

#[tokio::test]
async fn test_probe_dimensions_bad_path() {
    let result = probe_dimensions(Path::new("/nonexistent/video.mp4")).await;
    assert!(result.is_err());
}
```

- [ ] **Step 2: Implement compute_heights and probe_dimensions**

Add above the `#[cfg(test)]` block in `src/assets/videos.rs`:

```rust
/// Compute which height tiers to generate, sorted descending (for HTML source order).
///
/// Includes the source height as a tier. Skips configured heights >= source.
fn compute_heights(configured: &[u32], source_height: u32) -> Vec<u32> {
    let mut heights: Vec<u32> = configured
        .iter()
        .copied()
        .filter(|&h| h < source_height)
        .collect();
    heights.push(source_height);
    heights.sort();
    heights.dedup();
    heights.reverse();
    heights
}

/// Probe video dimensions using ffprobe.
async fn probe_dimensions(video_path: &Path) -> Result<(u32, u32)> {
    let output = tokio::process::Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            "-select_streams", "v:0",
        ])
        .arg(video_path)
        .output()
        .await
        .wrap_err("Failed to run ffprobe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ffprobe failed for {}: {}", video_path.display(), stderr);
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .wrap_err("Failed to parse ffprobe JSON output")?;

    let stream = json["streams"]
        .as_array()
        .and_then(|s| s.first())
        .ok_or_else(|| eyre::eyre!("No video stream found in {}", video_path.display()))?;

    let width = stream["width"]
        .as_u64()
        .ok_or_else(|| eyre::eyre!("Missing width in ffprobe output"))? as u32;
    let height = stream["height"]
        .as_u64()
        .ok_or_else(|| eyre::eyre!("Missing height in ffprobe output"))? as u32;

    Ok((width, height))
}

/// Transcode a video to VP9/WebM at a specific height.
async fn transcode_vp9(
    input_path: &Path,
    output_path: &Path,
    height: u32,
    crf: u8,
    source_height: u32,
) -> Result<()> {
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.args(["-y", "-i"]);
    cmd.arg(input_path);
    cmd.args(["-c:v", "libvpx-vp9"]);
    cmd.args(["-crf", &crf.to_string()]);
    cmd.args(["-b:v", "0"]);

    if height < source_height {
        cmd.args(["-vf", &format!("scale=-2:{}", height)]);
    }

    cmd.args(["-c:a", "libopus", "-b:a", "128k"]);
    cmd.arg(output_path);

    let output = cmd.output().await.wrap_err("Failed to run ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "ffmpeg transcode failed for {} → {}p: {}",
            input_path.display(),
            height,
            stderr
        );
    }

    Ok(())
}

/// Extract the first frame as a WebP poster image.
async fn extract_poster(
    input_path: &Path,
    output_path: &Path,
    quality: u8,
) -> Result<()> {
    let output = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(input_path)
        .args(["-vframes", "1", "-f", "image2", "-c:v", "libwebp"])
        .args(["-quality", &quality.to_string()])
        .arg(output_path)
        .output()
        .await
        .wrap_err("Failed to run ffmpeg for poster extraction")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg poster extraction failed: {}", stderr);
    }

    Ok(())
}

/// Write bytes to disk, creating parent directories.
fn write_variant_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create dir {}", parent.display()))?;
    }
    std::fs::write(path, data)
        .wrap_err_with(|| format!("Failed to write video variant {}", path.display()))?;
    Ok(())
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib assets::videos::tests`
Expected: All tests PASS (including the new `compute_heights` and `probe_dimensions_bad_path` tests).

- [ ] **Step 4: Commit**

```bash
git add src/assets/videos.rs
git commit -m "feat: add ffprobe, transcode, and poster extraction helpers"
```

---

### Task 5: Implement optimize_video() in videos.rs

**Files:**
- Modify: `src/assets/videos.rs`

- [ ] **Step 1: Write integration test for optimize_video**

Add to the `tests` module in `src/assets/videos.rs`:

```rust
/// Integration test that requires ffmpeg on PATH.
/// Creates a tiny test video, optimizes it, and verifies outputs.
#[tokio::test]
async fn test_optimize_video_with_ffmpeg() {
    // Skip if ffmpeg is not available.
    if check_ffmpeg().await.is_none() {
        eprintln!("Skipping test: ffmpeg not found on PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let dist_dir = tmp.path().join("dist");
    std::fs::create_dir_all(dist_dir.join("assets")).unwrap();

    // Generate a tiny 4x4 test video (1 second, minimal) using ffmpeg.
    let src_path = dist_dir.join("assets").join("test.mp4");
    let gen_output = tokio::process::Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i",
            "color=c=red:size=160x120:duration=0.5:rate=1",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
        ])
        .arg(&src_path)
        .output()
        .await
        .expect("Failed to generate test video");
    assert!(gen_output.status.success(), "Failed to create test video");

    let config = VideoOptimConfig {
        optimize: true,
        format: "vp9".to_string(),
        quality: 50,
        heights: vec![60, 120, 480],
        exclude: vec![],
        poster_quality: 80,
    };

    let cache = VideoCache::open(tmp.path()).unwrap();

    let result = optimize_video(
        &src_path,
        "/assets",
        &config,
        &cache,
        &dist_dir,
    )
    .await
    .unwrap();

    // Source is 160x120.
    assert_eq!(result.original_width, 160);
    assert_eq!(result.original_height, 120);

    // VP9 variants: 60p + 120p (source height). 480 > 120, so skipped.
    assert_eq!(result.vp9.len(), 2);
    assert_eq!(result.vp9[0].height, 120); // Descending order.
    assert_eq!(result.vp9[1].height, 60);

    // Original fallback exists.
    assert_eq!(result.original.mime_type, "video/mp4");

    // Poster exists.
    assert!(!result.poster_url.is_empty());

    // Verify files exist on disk.
    for v in &result.vp9 {
        let path = dist_dir.join(v.url_path.trim_start_matches('/'));
        assert!(path.exists(), "Missing VP9 variant: {}", path.display());
    }
    let poster_path = dist_dir.join(result.poster_url.trim_start_matches('/'));
    assert!(poster_path.exists(), "Missing poster: {}", poster_path.display());
    let orig_path = dist_dir.join(result.original.url_path.trim_start_matches('/'));
    assert!(orig_path.exists(), "Missing original: {}", orig_path.display());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib assets::videos::tests::test_optimize_video_with_ffmpeg`
Expected: FAIL — `optimize_video` does not exist yet.

- [ ] **Step 3: Implement optimize_video**

Add above the `#[cfg(test)]` block in `src/assets/videos.rs`:

```rust
/// Process a single video: generate VP9 variants at configured height tiers,
/// extract a poster frame, and copy the original as fallback.
///
/// `src_path` is the path on disk (e.g., `dist/assets/demo.mp4`).
/// `url_prefix` is the URL directory prefix (e.g., `/assets`).
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
    let stem = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let orig_ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");

    let (orig_width, orig_height) = probe_dimensions(src_path).await?;

    tracing::info!(
        "Processing video: {} ({}x{} → VP9)",
        src_path.display(),
        orig_width,
        orig_height,
    );

    let heights = compute_heights(&config.heights, orig_height);
    let out_dir = dist_dir.join(url_prefix.trim_start_matches('/'));
    std::fs::create_dir_all(&out_dir)
        .wrap_err_with(|| format!("Failed to create output dir {}", out_dir.display()))?;

    let mut vp9_variants = Vec::new();

    for &h in &heights {
        let variant_filename = format!("{}-{}p-{}.webm", stem, h, hash);
        let cache_key = &variant_filename;
        let out_path = out_dir.join(&variant_filename);
        let variant_url = format!("{}/{}", url_prefix, variant_filename);

        if let Some(cached_data) = cache.get(cache_key) {
            tracing::debug!("Video cache hit: {}", cache_key);
            write_variant_file(&out_path, &cached_data)?;
        } else {
            // Transcode to a temp file, then read + cache.
            let tmp_path = cache.cache_dir.join(format!("tmp-{}", variant_filename));
            transcode_vp9(src_path, &tmp_path, h, config.quality, orig_height).await?;
            let data = std::fs::read(&tmp_path)
                .wrap_err_with(|| format!("Failed to read transcoded file {}", tmp_path.display()))?;
            cache.put(cache_key, &data)?;
            write_variant_file(&out_path, &data)?;
            let _ = std::fs::remove_file(&tmp_path);
        }

        vp9_variants.push(VideoVariant {
            url_path: variant_url,
            height: h,
            mime_type: "video/webm".to_string(),
            codec: "vp9".to_string(),
        });
    }

    // Extract poster frame.
    let poster_filename = format!("{}-poster-{}.webp", stem, hash);
    let poster_cache_key = &poster_filename;
    let poster_out_path = out_dir.join(&poster_filename);
    let poster_url = format!("{}/{}", url_prefix, poster_filename);

    if let Some(cached_data) = cache.get(poster_cache_key) {
        tracing::debug!("Poster cache hit: {}", poster_cache_key);
        write_variant_file(&poster_out_path, &cached_data)?;
    } else {
        let tmp_poster = cache.cache_dir.join(format!("tmp-{}", poster_filename));
        extract_poster(src_path, &tmp_poster, config.poster_quality).await?;
        let data = std::fs::read(&tmp_poster)
            .wrap_err_with(|| format!("Failed to read poster {}", tmp_poster.display()))?;
        cache.put(poster_cache_key, &data)?;
        write_variant_file(&poster_out_path, &data)?;
        let _ = std::fs::remove_file(&tmp_poster);
    }

    // Copy original to dist as fallback.
    let orig_filename = format!("{}-{}.{}", stem, hash, orig_ext);
    let orig_out_path = out_dir.join(&orig_filename);
    let orig_url = format!("{}/{}", url_prefix, orig_filename);
    write_variant_file(&orig_out_path, &src_data)?;

    let original = VideoVariant {
        url_path: orig_url,
        height: orig_height,
        mime_type: video_mime_type(orig_ext).to_string(),
        codec: "original".to_string(),
    };

    Ok(VideoVariants {
        original_width: orig_width,
        original_height: orig_height,
        vp9: vp9_variants,
        original,
        poster_url,
    })
}
```

Add a `temp_path` method to `VideoCache` to keep encapsulation (instead of exposing `cache_dir`):

```rust
impl VideoCache {
    // ... existing methods ...

    /// Return a temporary file path within the cache directory.
    pub fn temp_path(&self, name: &str) -> PathBuf {
        self.cache_dir.join(format!("tmp-{}", name))
    }
}
```

Then use `cache.temp_path(&variant_filename)` instead of `cache.cache_dir.join(...)` in `optimize_video`.

Also note: the original video file is copied to dist with a content-hash filename
(e.g., `demo-a1b2c3.mp4`) as a fallback. If asset localization already placed the
file at `dist/assets/demo.mp4`, this creates a duplicate. The hash-named copy is
needed for cache-busting. If this duplication is unacceptable for large videos,
a future optimization could symlink instead of copy, or use the existing path
as the fallback URL directly. For now, the copy approach is correct and simple.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib assets::videos::tests`
Expected: All tests PASS (including the ffmpeg integration test — requires ffmpeg from nix shell).

- [ ] **Step 5: Commit**

```bash
git add src/assets/videos.rs
git commit -m "feat: implement optimize_video with VP9 transcoding and poster extraction"
```

---

### Task 6: Create video_rewrite.rs — HTML collection and source builder

**Files:**
- Create: `src/assets/video_rewrite.rs`

Uses `scraper` (already a dependency) for the collection phase to build an ordered list
of video elements in document order. Each entry records the source URL and which form
it uses (Form 1: `<video src>`, Form 2: `<video><source src>`). This ordered list
drives a single-pass `lol_html` rewrite in Task 7 with an index counter, so each
`<video>` element gets the correct poster and sources — even with multiple Form 2
videos on the same page.

- [ ] **Step 1: Write tests for video HTML collection and source builder**

Create `src/assets/video_rewrite.rs` with tests first:

```rust
//! HTML rewriting for optimized video elements.
//!
//! Uses `scraper` for document-order collection and `lol_html` for
//! streaming HTML rewriting. This two-tool approach solves the problem
//! of associating parent `<video>` elements with their child `<source>`
//! URLs — scraper gives us the full DOM tree for collection, and
//! lol_html gives us efficient streaming rewrite.

use eyre::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::config::VideoOptimConfig;
use super::videos::{VideoCache, VideoVariant, VideoVariants, is_excluded, optimize_video};

/// Mapping from original video URL path → generated variants.
type VideoVariantMap = HashMap<String, VideoVariants>;

/// A video element discovered during HTML collection.
/// Records the source URL and which HTML form it uses.
#[derive(Debug, Clone)]
struct VideoEntry {
    /// The video source URL (e.g., `/videos/demo.mp4`).
    src: String,
    /// True if the source came from `<video src="...">` (Form 1).
    /// False if it came from `<video><source src="...">` (Form 2).
    is_form1: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_video_entries_form1() {
        let html = r#"<html><body><video src="/videos/demo.mp4" controls></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].src, "/videos/demo.mp4");
        assert!(entries[0].is_form1);
    }

    #[test]
    fn test_collect_video_entries_form2() {
        let html = r#"<html><body><video controls><source src="/videos/demo.mp4" type="video/mp4"></video></body></html>"#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].src, "/videos/demo.mp4");
        assert!(!entries[0].is_form1);
    }

    #[test]
    fn test_collect_video_entries_skips_external() {
        let html = r#"<video src="https://cdn.example.com/video.mp4"></video>"#;
        let entries = collect_video_entries(html, &[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_collect_video_entries_skips_data_no_optimize() {
        let html = r#"<video src="/videos/demo.mp4" data-no-optimize></video>"#;
        let entries = collect_video_entries(html, &[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_collect_video_entries_multiple_form2() {
        let html = r#"
            <video controls><source src="/videos/a.mp4" type="video/mp4"></video>
            <video controls><source src="/videos/b.mp4" type="video/mp4"></video>
        "#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].src, "/videos/a.mp4");
        assert_eq!(entries[1].src, "/videos/b.mp4");
        assert!(!entries[0].is_form1);
        assert!(!entries[1].is_form1);
    }

    #[test]
    fn test_collect_video_entries_mixed_forms() {
        let html = r#"
            <video src="/videos/inline.mp4" controls></video>
            <video controls><source src="/videos/sourced.mp4" type="video/mp4"></video>
        "#;
        let entries = collect_video_entries(html, &[]);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_form1);
        assert!(!entries[1].is_form1);
    }

    #[test]
    fn test_collect_video_entries_respects_exclude() {
        let html = r#"<video src="/promo/intro.mp4"></video>"#;
        let entries = collect_video_entries(html, &["promo/*".to_string()]);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_build_sources_html() {
        let variants = VideoVariants {
            original_width: 1920,
            original_height: 1080,
            vp9: vec![
                VideoVariant {
                    url_path: "/assets/demo-1080p-abc.webm".to_string(),
                    height: 1080,
                    mime_type: "video/webm".to_string(),
                    codec: "vp9".to_string(),
                },
                VideoVariant {
                    url_path: "/assets/demo-720p-abc.webm".to_string(),
                    height: 720,
                    mime_type: "video/webm".to_string(),
                    codec: "vp9".to_string(),
                },
            ],
            original: VideoVariant {
                url_path: "/assets/demo-abc.mp4".to_string(),
                height: 1080,
                mime_type: "video/mp4".to_string(),
                codec: "original".to_string(),
            },
            poster_url: "/assets/demo-poster-abc.webp".to_string(),
        };

        let sources_html = build_sources_html(&variants);
        assert!(sources_html.contains(r#"src="/assets/demo-1080p-abc.webm""#));
        assert!(sources_html.contains(r#"src="/assets/demo-720p-abc.webm""#));
        assert!(sources_html.contains(r#"src="/assets/demo-abc.mp4""#));
        assert!(sources_html.contains(r#"type="video/webm; codecs=&quot;vp9&quot;""#));
        assert!(sources_html.contains(r#"type="video/mp4""#));

        // VP9 sources come before original fallback.
        let vp9_pos = sources_html.find("demo-1080p").unwrap();
        let orig_pos = sources_html.find("demo-abc.mp4").unwrap();
        assert!(vp9_pos < orig_pos);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib assets::video_rewrite::tests`
Expected: FAIL — functions don't exist yet.

- [ ] **Step 3: Implement collect_video_entries using scraper**

Add above the `#[cfg(test)]` block:

```rust
/// Collect all `<video>` elements from HTML in document order.
///
/// Uses `scraper` for DOM parsing so we can associate parent `<video>`
/// elements with their child `<source>` URLs — something lol_html's
/// streaming model can't do reliably for Form 2 videos.
///
/// Returns an ordered list of `VideoEntry` where each entry records:
/// - The source URL
/// - Whether it's Form 1 (`<video src>`) or Form 2 (`<video><source src>`)
///
/// Skips: external URLs, data-no-optimize elements, excluded paths.
fn collect_video_entries(html: &str, exclude_patterns: &[String]) -> Vec<VideoEntry> {
    let document = scraper::Html::parse_document(html);
    let video_selector = scraper::Selector::parse("video").unwrap();

    let mut entries = Vec::new();

    for video_el in document.select(&video_selector) {
        // Skip data-no-optimize.
        if video_el.value().attr("data-no-optimize").is_some() {
            continue;
        }

        // Form 1: <video src="...">
        if let Some(src) = video_el.value().attr("src") {
            if should_skip_url(src) {
                continue;
            }
            let check_path = src.trim_start_matches('/');
            if is_excluded(check_path, exclude_patterns) {
                continue;
            }
            entries.push(VideoEntry {
                src: src.to_string(),
                is_form1: true,
            });
            continue;
        }

        // Form 2: <video><source src="...">
        let source_selector = scraper::Selector::parse("source").unwrap();
        for source_el in video_el.select(&source_selector) {
            if let Some(src) = source_el.value().attr("src") {
                if should_skip_url(src) {
                    continue;
                }
                let check_path = src.trim_start_matches('/');
                if is_excluded(check_path, exclude_patterns) {
                    continue;
                }
                entries.push(VideoEntry {
                    src: src.to_string(),
                    is_form1: false,
                });
                break; // Only use the first <source> per <video>.
            }
        }
    }

    entries
}

fn should_skip_url(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("data:")
}

/// Build the inner `<source>` elements HTML string for a rewritten `<video>`.
fn build_sources_html(variants: &VideoVariants) -> String {
    let mut html = String::new();

    // VP9 sources, highest resolution first.
    for v in &variants.vp9 {
        html.push_str(&format!(
            r#"<source src="{}" type="video/webm; codecs=&quot;vp9&quot;">"#,
            v.url_path,
        ));
    }

    // Original fallback.
    html.push_str(&format!(
        r#"<source src="{}" type="{}">"#,
        variants.original.url_path, variants.original.mime_type,
    ));

    html
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib assets::video_rewrite::tests`
Expected: All tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/assets/video_rewrite.rs
git commit -m "feat: add video HTML collection (scraper) and source element builder"
```

---

### Task 7: Implement optimize_and_rewrite_videos() — single-pass indexed rewrite

**Files:**
- Modify: `src/assets/video_rewrite.rs`

The rewrite phase uses a single lol_html pass with an index counter that increments
for each `<video>` element encountered. The counter maps to the ordered `VideoEntry`
list from `collect_video_entries()` (Task 6). This ensures each `<video>` gets the
correct poster and sources — even with multiple Form 2 videos on the same page.

- [ ] **Step 1: Write integration tests**

Add to the `tests` module in `src/assets/video_rewrite.rs`:

```rust
#[tokio::test]
async fn test_optimize_and_rewrite_form1_with_ffmpeg() {
    if super::super::videos::check_ffmpeg().await.is_none() {
        eprintln!("Skipping test: ffmpeg not found on PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let dist_dir = tmp.path().join("dist");
    std::fs::create_dir_all(dist_dir.join("videos")).unwrap();

    let src_path = dist_dir.join("videos").join("demo.mp4");
    let gen = tokio::process::Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i",
            "color=c=blue:size=160x120:duration=0.5:rate=1",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
        ])
        .arg(&src_path)
        .output()
        .await
        .expect("Failed to generate test video");
    assert!(gen.status.success());

    let html = r#"<html><body><video src="/videos/demo.mp4" controls></video></body></html>"#;
    let config = VideoOptimConfig {
        optimize: true,
        format: "vp9".to_string(),
        quality: 50,
        heights: vec![60, 120],
        exclude: vec![],
        poster_quality: 80,
    };
    let cache = VideoCache::open(tmp.path()).unwrap();

    let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir).await.unwrap();

    assert!(result.contains("poster="));
    assert!(result.contains(r#"preload="none""#));
    assert!(result.contains("vp9"));
    assert!(result.contains("video/mp4"));
    // Original src removed.
    assert!(!result.contains(r#"src="/videos/demo.mp4""#));
    assert!(result.contains("controls"));
}

#[tokio::test]
async fn test_rewrite_skips_when_disabled() {
    let config = VideoOptimConfig {
        optimize: false,
        ..VideoOptimConfig::default()
    };
    let tmp = tempfile::TempDir::new().unwrap();
    let cache = VideoCache::open(tmp.path()).unwrap();
    let dist_dir = tmp.path().join("dist");

    let html = r#"<video src="/videos/demo.mp4" controls></video>"#;
    let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir).await.unwrap();
    assert_eq!(result, html);
}

#[tokio::test]
async fn test_rewrite_preserves_explicit_preload() {
    if super::super::videos::check_ffmpeg().await.is_none() {
        eprintln!("Skipping test: ffmpeg not found on PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let dist_dir = tmp.path().join("dist");
    std::fs::create_dir_all(dist_dir.join("videos")).unwrap();

    let src_path = dist_dir.join("videos").join("bg.mp4");
    let gen = tokio::process::Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i",
            "color=c=green:size=80x60:duration=0.5:rate=1",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
        ])
        .arg(&src_path)
        .output()
        .await
        .expect("Failed to generate test video");
    assert!(gen.status.success());

    let html = r#"<video src="/videos/bg.mp4" preload="auto" autoplay muted loop></video>"#;
    let config = VideoOptimConfig {
        optimize: true,
        format: "vp9".to_string(),
        quality: 50,
        heights: vec![60],
        exclude: vec![],
        poster_quality: 80,
    };
    let cache = VideoCache::open(tmp.path()).unwrap();
    let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir).await.unwrap();

    assert!(result.contains(r#"preload="auto""#));
    assert!(result.contains("autoplay"));
    assert!(result.contains("muted"));
    assert!(result.contains("loop"));
}

/// Critical test: multiple Form 2 videos on same page must get correct posters.
#[tokio::test]
async fn test_rewrite_multiple_form2_correct_posters() {
    if super::super::videos::check_ffmpeg().await.is_none() {
        eprintln!("Skipping test: ffmpeg not found on PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let dist_dir = tmp.path().join("dist");
    std::fs::create_dir_all(dist_dir.join("videos")).unwrap();

    // Generate two distinct test videos.
    for (name, color) in [("a", "red"), ("b", "blue")] {
        let path = dist_dir.join("videos").join(format!("{}.mp4", name));
        let gen = tokio::process::Command::new("ffmpeg")
            .args([
                "-y", "-f", "lavfi", "-i",
                &format!("color=c={}:size=80x60:duration=0.5:rate=1", color),
                "-c:v", "libx264", "-pix_fmt", "yuv420p",
            ])
            .arg(&path)
            .output()
            .await
            .expect("Failed to generate test video");
        assert!(gen.status.success());
    }

    let html = r#"<html><body>
        <video controls><source src="/videos/a.mp4" type="video/mp4"></video>
        <video controls><source src="/videos/b.mp4" type="video/mp4"></video>
    </body></html>"#;

    let config = VideoOptimConfig {
        optimize: true,
        format: "vp9".to_string(),
        quality: 50,
        heights: vec![60],
        exclude: vec![],
        poster_quality: 80,
    };
    let cache = VideoCache::open(tmp.path()).unwrap();
    let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir).await.unwrap();

    // Both videos should have poster attributes.
    let poster_count = result.matches("poster=").count();
    assert_eq!(poster_count, 2, "Expected 2 poster attributes, got {}", poster_count);

    // Poster URLs should be different (different source videos).
    let doc = scraper::Html::parse_document(&result);
    let sel = scraper::Selector::parse("video").unwrap();
    let posters: Vec<String> = doc.select(&sel)
        .filter_map(|el| el.value().attr("poster").map(String::from))
        .collect();
    assert_eq!(posters.len(), 2);
    assert_ne!(posters[0], posters[1], "Form 2 videos got the same poster — bug!");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib assets::video_rewrite::tests::test_optimize_and_rewrite`
Expected: FAIL — `optimize_and_rewrite_videos` does not exist.

- [ ] **Step 3: Implement optimize_and_rewrite_videos with single-pass indexed rewrite**

Add above the `#[cfg(test)]` block in `src/assets/video_rewrite.rs`:

```rust
/// Optimize videos and rewrite `<video>` elements in HTML.
///
/// Phase 1 (scraper): Collect all `<video>` entries in document order, recording
/// the source URL and form (Form 1 vs Form 2) for each.
///
/// Phase 2: Optimize each unique video (transcode, poster, cache).
///
/// Phase 3 (lol_html): Single-pass rewrite using an index counter. Each `<video>`
/// element encountered increments the counter and looks up `entries[index]` to
/// decide what attributes and sources to emit. This guarantees correct poster
/// assignment even with multiple Form 2 videos on the same page.
pub async fn optimize_and_rewrite_videos(
    html: &str,
    config: &VideoOptimConfig,
    cache: &VideoCache,
    dist_dir: &Path,
) -> Result<String> {
    if !config.optimize {
        return Ok(html.to_string());
    }

    // Phase 1: Collect video entries in document order.
    let entries = collect_video_entries(html, &config.exclude);
    if entries.is_empty() {
        return Ok(html.to_string());
    }

    // Phase 2: Optimize each unique video source.
    let mut variant_map: VideoVariantMap = HashMap::new();
    for entry in &entries {
        if variant_map.contains_key(&entry.src) {
            continue;
        }

        let fs_path = super::images::url_to_dist_path(&entry.src, dist_dir);
        if !fs_path.exists() {
            tracing::warn!("Video file not found, skipping: {}", fs_path.display());
            continue;
        }

        let url_prefix = super::images::url_dir_prefix(&entry.src);

        match optimize_video(&fs_path, &url_prefix, config, cache, dist_dir).await {
            Ok(variants) => {
                variant_map.insert(entry.src.clone(), variants);
            }
            Err(e) => {
                tracing::warn!("Failed to optimize video {}: {}", entry.src, e);
            }
        }
    }

    if variant_map.is_empty() {
        return Ok(html.to_string());
    }

    // Phase 3: Single-pass rewrite with indexed lookup.
    rewrite_video_elements(html, &entries, &variant_map)
}

/// Rewrite `<video>` elements in a single lol_html pass.
///
/// Uses an index counter that increments for each `<video>` element.
/// The counter maps to the ordered `entries` list from `collect_video_entries()`,
/// which tells us the source URL and form for each video.
fn rewrite_video_elements(
    html: &str,
    entries: &[VideoEntry],
    variant_map: &VideoVariantMap,
) -> Result<String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let output_write = Rc::clone(&output);

    let video_index: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
    let vi_video = Rc::clone(&video_index);
    let vi_source = Rc::clone(&video_index);

    let entries_rc: Rc<Vec<VideoEntry>> = Rc::new(entries.to_vec());
    let entries_video = Rc::clone(&entries_rc);
    let entries_source = Rc::clone(&entries_rc);

    let vm = Rc::new(variant_map.clone());
    let vm_video = Rc::clone(&vm);
    let vm_source = Rc::clone(&vm);

    // Track whether the current <video> was handled (Form 1 with variants).
    let current_handled: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let ch_video = Rc::clone(&current_handled);
    let ch_source = Rc::clone(&current_handled);

    let mut rewriter = lol_html::HtmlRewriter::new(
        lol_html::Settings {
            element_content_handlers: vec![
                lol_html::element!("video", move |el| {
                    let idx = *vi_video.borrow();
                    *vi_video.borrow_mut() = idx + 1;
                    *ch_video.borrow_mut() = false;

                    if idx >= entries_video.len() {
                        return Ok(());
                    }

                    let entry = &entries_video[idx];
                    let variants = match vm_video.get(&entry.src) {
                        Some(v) => v,
                        None => return Ok(()),
                    };

                    if entry.is_form1 {
                        // Form 1: remove src, add poster/preload, prepend sources.
                        el.remove_attribute("src");
                        el.set_attribute("poster", &variants.poster_url)
                            .map_err(|e| eyre::eyre!("{}", e))?;
                        if el.get_attribute("preload").is_none() {
                            el.set_attribute("preload", "none")
                                .map_err(|e| eyre::eyre!("{}", e))?;
                        }
                        el.remove_attribute("data-no-optimize");
                        let sources = build_sources_html(variants);
                        el.prepend(&sources, lol_html::html_content::ContentType::Html);
                        *ch_video.borrow_mut() = true;
                    } else {
                        // Form 2: add poster/preload to parent <video>.
                        // Child <source> replacement happens in the source handler.
                        el.set_attribute("poster", &variants.poster_url)
                            .map_err(|e| eyre::eyre!("{}", e))?;
                        if el.get_attribute("preload").is_none() {
                            el.set_attribute("preload", "none")
                                .map_err(|e| eyre::eyre!("{}", e))?;
                        }
                        el.remove_attribute("data-no-optimize");
                    }

                    Ok(())
                }),
                lol_html::element!("video source", move |el| {
                    if *ch_source.borrow() {
                        // Parent was Form 1 — remove original <source> children.
                        el.remove();
                        return Ok(());
                    }

                    // Form 2: check if this <source> matches the current entry.
                    // Index was already incremented in the <video> handler,
                    // so the current entry is at idx - 1.
                    let idx = vi_source.borrow().checked_sub(1).unwrap_or(0);
                    if idx >= entries_source.len() {
                        return Ok(());
                    }

                    let entry = &entries_source[idx];
                    if let Some(src) = el.get_attribute("src") {
                        if src == entry.src {
                            if let Some(variants) = vm_source.get(&entry.src) {
                                let sources = build_sources_html(variants);
                                el.replace(&sources, lol_html::html_content::ContentType::Html);
                            }
                        }
                    }
                    Ok(())
                }),
            ],
            ..lol_html::Settings::new()
        },
        move |chunk: &[u8]| {
            output_write.borrow_mut().extend_from_slice(chunk);
        },
    );

    rewriter
        .write(html.as_bytes())
        .map_err(|e| eyre::eyre!("HTML rewrite error: {}", e))?;
    rewriter
        .end()
        .map_err(|e| eyre::eyre!("HTML rewrite finalize error: {}", e))?;

    let bytes = output.borrow().clone();
    String::from_utf8(bytes).wrap_err("HTML rewrite produced invalid UTF-8")
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib assets::video_rewrite::tests`
Expected: All tests PASS, including `test_rewrite_multiple_form2_correct_posters`.

- [ ] **Step 5: Commit**

```bash
git add src/assets/video_rewrite.rs
git commit -m "feat: implement optimize_and_rewrite_videos with single-pass indexed rewrite"
```

---

### Task 8: Wire modules into assets/mod.rs

**Files:**
- Modify: `src/assets/mod.rs`

- [ ] **Step 1: Add module declarations and re-exports**

Update `src/assets/mod.rs` to:

```rust
//! Asset localization: download remote images/videos/audio referenced in
//! rendered HTML, save them to `dist/assets/`, and rewrite `src` attributes
//! to point to the local copies.
//!
//! Supports:
//! - `<img>`, `<video>`, `<source>`, `<audio>` `src` attributes
//! - CSS `background-image: url(...)` in inline `style` attributes and
//!   `<style>` blocks
//!
//! Skips:
//! - Relative URLs (no scheme)
//! - URLs already under `/assets/`
//! - Known CDN hostnames (configurable via `site.toml`)

pub mod cache;
pub mod download;
pub mod html_rewrite;
pub mod images;
mod rewrite;
pub mod video_rewrite;
pub mod videos;

pub use html_rewrite::optimize_and_rewrite_images;
pub use html_rewrite::rewrite_css_background_images;
pub use rewrite::localize_assets;
pub use rewrite::{check_asset_cache, download_missing_assets, store_and_rewrite_assets};
pub use video_rewrite::optimize_and_rewrite_videos;
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/assets/mod.rs
git commit -m "feat: register video modules and re-export optimize_and_rewrite_videos"
```

---

### Task 9: Integrate into the build pipeline

**Files:**
- Modify: `src/build/render.rs:17,67-90,250-323,580-593,691-696,1302-1329`

- [ ] **Step 1: Add VideoCache and ffmpeg_available to BuildContext**

In `src/build/render.rs`, add the import (near line 17):

```rust
use crate::assets::videos::VideoCache;
```

Add fields to `BuildContext` (after `image_cache`, around line 80):

```rust
pub video_cache: Arc<VideoCache>,
pub ffmpeg_available: bool,
```

- [ ] **Step 2: Initialize VideoCache and check ffmpeg at build start**

In the `build()` function, after the image cache initialization block (around line 270), add:

```rust
// Video optimization.
let video_cache = VideoCache::open(project_root)
    .wrap_err("Failed to open video cache")?;
let ffmpeg_available = if config.assets.videos.optimize {
    match crate::assets::videos::check_ffmpeg().await {
        Some(version) => {
            tracing::info!("Video optimization enabled ({}).", version);
            true
        }
        None => {
            tracing::warn!("ffmpeg not found on PATH, video optimization disabled.");
            false
        }
    }
} else {
    false
};
```

Add the fields to the `BuildContext` construction (around line 316):

```rust
video_cache: Arc::new(video_cache),
ffmpeg_available,
```

- [ ] **Step 3: Add video optimization step to finalize_page_html**

In `finalize_page_html()`, after the CSS background image rewriting (around line 593), add:

```rust
// Video optimization: transcode + rewrite <video> → multi-<source>.
let full_html = if ctx.ffmpeg_available && ctx.config.assets.videos.optimize {
    assets::optimize_and_rewrite_videos(
        &full_html,
        &ctx.config.assets.videos,
        &ctx.video_cache,
        dist_dir,
    ).await
    .wrap_err_with(|| format!("Failed to optimize videos for {}", input.label))?
} else {
    full_html
};
```

Note: `finalize_page_html` needs to be `async` for this `.await`. Check if it's already async — if so, this works as-is. If not, it will need to be made async, and its callers updated.

- [ ] **Step 4: Add video optimization to fragment processing**

In `optimize_fragment_images()` (around line 1302), rename it to `optimize_fragment_assets()` and make it `async`. **Do not use `block_on`** — calling `tokio::runtime::Handle::current().block_on()` from an async context will deadlock. Since the caller (`finalize_page_html`) is already async, just `.await` the result:

```rust
async fn optimize_fragment_assets(
    frags: &[fragments::Fragment],
    image_config: &crate::config::ImageOptimConfig,
    image_cache: &ImageCache,
    video_config: &crate::config::VideoOptimConfig,
    video_cache: &VideoCache,
    ffmpeg_available: bool,
    dist_dir: &Path,
) -> Result<Vec<fragments::Fragment>> {
    let mut result = Vec::with_capacity(frags.len());
    for frag in frags {
        let optimized_html = assets::optimize_and_rewrite_images(
            &frag.html,
            image_config,
            image_cache,
            dist_dir,
            None,
        )?;
        let optimized_html = assets::rewrite_css_background_images(
            &optimized_html,
            image_config,
            image_cache,
            dist_dir,
        )?;
        let optimized_html = if ffmpeg_available && video_config.optimize {
            assets::optimize_and_rewrite_videos(
                &optimized_html,
                video_config,
                video_cache,
                dist_dir,
            ).await?
        } else {
            optimized_html
        };
        result.push(fragments::Fragment {
            block_name: frag.block_name.clone(),
            html: optimized_html,
        });
    }
    Ok(result)
}
```

Update the call site (around line 691) to `.await` the result and pass the additional parameters.

- [ ] **Step 5: Verify compilation and existing tests pass**

Run: `cargo check && cargo test`
Expected: Compiles and all existing tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/build/render.rs
git commit -m "feat: integrate video optimization into build pipeline"
```

---

### Task 10: Write feature documentation

**Files:**
- Create: `docs/video_optimization.md`

- [ ] **Step 1: Write documentation**

Create `docs/video_optimization.md`:

```markdown
# Video Optimization

Eigen automatically optimizes `<video>` elements in your templates by transcoding to VP9/WebM, generating multiple resolution tiers, and extracting poster frames.

## Requirements

- **ffmpeg** must be installed and available on PATH
- If ffmpeg is not found, eigen logs a warning and skips video optimization (no build failure)

## Configuration

In `site.toml`:

```toml
[assets.videos]
optimize = true              # Master switch (default: true)
format = "vp9"               # Target codec (default: "vp9")
quality = 30                 # CRF value, 0-63, lower=better (default: 30)
heights = [480, 720, 1080]   # Resolution tiers in pixels (default: [480, 720, 1080])
exclude = []                 # Glob patterns to exclude
poster_quality = 80          # WebP quality for poster frames (default: 80)
```

## How It Works

### Input

Either form is supported:

```html
<video src="/videos/demo.mp4" controls></video>

<video controls>
  <source src="/videos/demo.mp4" type="video/mp4">
</video>
```

### Output

Eigen rewrites the element with VP9 sources (highest resolution first), the original as fallback, a poster frame, and `preload="none"`:

```html
<video poster="/assets/demo-poster-a1b2c3.webp" preload="none" controls>
  <source src="/assets/demo-1080p-a1b2c3.webm" type="video/webm; codecs=&quot;vp9&quot;">
  <source src="/assets/demo-720p-a1b2c3.webm" type="video/webm; codecs=&quot;vp9&quot;">
  <source src="/assets/demo-480p-a1b2c3.webm" type="video/webm; codecs=&quot;vp9&quot;">
  <source src="/assets/demo-a1b2c3.mp4" type="video/mp4">
</video>
```

### Resolution Tiers

Configured heights above the source video's resolution are skipped. The source resolution is always included as a tier.

### Poster Frame

The first frame is extracted as a WebP image and set as the `poster` attribute. The browser displays this immediately while the video loads.

## Excluding Videos

### Per-element

Add `data-no-optimize` to skip a specific video:

```html
<video src="/videos/hero.mp4" data-no-optimize controls></video>
```

The attribute is stripped from the output.

### Per-pattern

Use glob patterns in config:

```toml
[assets.videos]
exclude = ["videos/raw/*", "**/*.gif"]
```

## Caching

Transcoded variants are cached at `.eigen_cache/videos/` with content-hash filenames. Unchanged source videos are not re-transcoded on subsequent builds.

## Attribute Preservation

All attributes on the original `<video>` element are preserved: `controls`, `autoplay`, `muted`, `loop`, `class`, `id`, `width`, `height`, etc.

If `preload` is explicitly set on the original element, that value is preserved. Otherwise, `preload="none"` is set to prevent unnecessary video loading.

## External Videos

Videos with external URLs (`http://`, `https://`) are not processed. They pass through unchanged.
```

- [ ] **Step 2: Commit**

```bash
git add docs/video_optimization.md
git commit -m "docs: add video optimization feature documentation"
```

---

### Task 11: Run full test suite and verify

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass, including new video tests.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Test with a real site (if available)**

If a test site is available with `<video>` elements, run `cargo run -- build` and verify:
- VP9 variants appear in `dist/assets/`
- Poster frames appear in `dist/assets/`
- HTML contains rewritten `<video>` elements
- Original fallback is present

- [ ] **Step 4: Final commit if any fixups needed**

```bash
git add -A && git commit -m "fix: address clippy warnings and test fixups"
```
