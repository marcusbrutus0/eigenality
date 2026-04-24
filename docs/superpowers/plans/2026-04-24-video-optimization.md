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

Note: the `cache_dir` field on `VideoCache` needs to be `pub(crate)` for `optimize_video` to use it for temp files. Update the field visibility:

```rust
pub struct VideoCache {
    pub(crate) cache_dir: PathBuf,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib assets::videos::tests`
Expected: All tests PASS (including the ffmpeg integration test — requires ffmpeg from nix shell).

- [ ] **Step 5: Commit**

```bash
git add src/assets/videos.rs
git commit -m "feat: implement optimize_video with VP9 transcoding and poster extraction"
```

---

### Task 6: Create video_rewrite.rs — HTML collection and rewriting

**Files:**
- Create: `src/assets/video_rewrite.rs`

- [ ] **Step 1: Write tests for video HTML rewriting**

Create `src/assets/video_rewrite.rs` with tests first:

```rust
//! HTML rewriting for optimized video elements.
//!
//! Uses `lol_html` to collect `<video>` elements, optimize them via
//! the `videos` module, and rewrite the HTML with `<source>` elements,
//! `poster`, and `preload` attributes.

use eyre::Result;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::config::VideoOptimConfig;
use super::videos::{VideoCache, VideoVariants, is_excluded, optimize_video};

/// Mapping from original video URL path → generated variants.
type VideoVariantMap = HashMap<String, VideoVariants>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_video_srcs_form1() {
        let html = r#"<html><body><video src="/videos/demo.mp4" controls></video></body></html>"#;
        let srcs = collect_video_srcs(html).unwrap();
        assert_eq!(srcs, vec!["/videos/demo.mp4"]);
    }

    #[test]
    fn test_collect_video_srcs_form2() {
        let html = r#"<html><body><video controls><source src="/videos/demo.mp4" type="video/mp4"></video></body></html>"#;
        let srcs = collect_video_srcs(html).unwrap();
        assert_eq!(srcs, vec!["/videos/demo.mp4"]);
    }

    #[test]
    fn test_collect_video_srcs_skips_external() {
        let html = r#"<video src="https://cdn.example.com/video.mp4"></video>"#;
        let srcs = collect_video_srcs(html).unwrap();
        assert!(srcs.is_empty());
    }

    #[test]
    fn test_collect_video_srcs_skips_data_no_optimize() {
        let html = r#"<video src="/videos/demo.mp4" data-no-optimize></video>"#;
        let srcs = collect_video_srcs(html).unwrap();
        assert!(srcs.is_empty());
    }

    #[test]
    fn test_collect_video_srcs_deduplicates() {
        let html = r#"
            <video src="/videos/demo.mp4"></video>
            <video src="/videos/demo.mp4"></video>
        "#;
        let srcs = collect_video_srcs(html).unwrap();
        assert_eq!(srcs, vec!["/videos/demo.mp4"]);
    }

    #[test]
    fn test_build_video_replacement_html() {
        let variants = VideoVariants {
            original_width: 1920,
            original_height: 1080,
            vp9: vec![
                super::super::videos::VideoVariant {
                    url_path: "/assets/demo-1080p-abc.webm".to_string(),
                    height: 1080,
                    mime_type: "video/webm".to_string(),
                    codec: "vp9".to_string(),
                },
                super::super::videos::VideoVariant {
                    url_path: "/assets/demo-720p-abc.webm".to_string(),
                    height: 720,
                    mime_type: "video/webm".to_string(),
                    codec: "vp9".to_string(),
                },
            ],
            original: super::super::videos::VideoVariant {
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

- [ ] **Step 3: Implement collect_video_srcs**

Add above the `#[cfg(test)]` block:

```rust
/// Collect all local video URLs from `<video>` elements in HTML.
///
/// Handles both `<video src="...">` and `<video><source src="...">` forms.
/// Skips external URLs, data-no-optimize elements, and deduplicates.
fn collect_video_srcs(html: &str) -> Result<Vec<String>> {
    let srcs: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let seen: Rc<RefCell<std::collections::HashSet<String>>> =
        Rc::new(RefCell::new(std::collections::HashSet::new()));

    // Track whether current <video> has data-no-optimize.
    let skip_flag: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let srcs_video = Rc::clone(&srcs);
    let seen_video = Rc::clone(&seen);
    let skip_video = Rc::clone(&skip_flag);

    let srcs_source = Rc::clone(&srcs);
    let seen_source = Rc::clone(&seen);
    let skip_source = Rc::clone(&skip_flag);

    let skip_end = Rc::clone(&skip_flag);

    let mut rewriter = lol_html::HtmlRewriter::new(
        lol_html::Settings {
            element_content_handlers: vec![
                lol_html::element!("video", move |el| {
                    // Check data-no-optimize on <video>.
                    if el.has_attribute("data-no-optimize") {
                        *skip_video.borrow_mut() = true;
                        return Ok(());
                    }
                    *skip_video.borrow_mut() = false;

                    if let Some(src) = el.get_attribute("src") {
                        if !src.starts_with("http://")
                            && !src.starts_with("https://")
                            && !src.starts_with("data:")
                        {
                            let mut seen = seen_video.borrow_mut();
                            if !seen.contains(&src) {
                                seen.insert(src.clone());
                                srcs_video.borrow_mut().push(src);
                            }
                        }
                    }
                    Ok(())
                }),
                lol_html::element!("video source", move |el| {
                    if *skip_source.borrow() {
                        return Ok(());
                    }
                    if let Some(src) = el.get_attribute("src") {
                        if !src.starts_with("http://")
                            && !src.starts_with("https://")
                            && !src.starts_with("data:")
                        {
                            let mut seen = seen_source.borrow_mut();
                            if !seen.contains(&src) {
                                seen.insert(src.clone());
                                srcs_source.borrow_mut().push(src);
                            }
                        }
                    }
                    Ok(())
                }),
            ],
            ..lol_html::Settings::new()
        },
        |_: &[u8]| {},
    );

    rewriter
        .write(html.as_bytes())
        .map_err(|e| eyre::eyre!("HTML parse error collecting video srcs: {}", e))?;
    rewriter
        .end()
        .map_err(|e| eyre::eyre!("HTML parse error finalizing video src collection: {}", e))?;

    let result = srcs.borrow().clone();
    Ok(result)
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
git commit -m "feat: add video HTML collection and source element builder"
```

---

### Task 7: Implement optimize_and_rewrite_videos() — the main public function

**Files:**
- Modify: `src/assets/video_rewrite.rs`

- [ ] **Step 1: Write integration test**

Add to the `tests` module in `src/assets/video_rewrite.rs`:

```rust
#[tokio::test]
async fn test_optimize_and_rewrite_videos_with_ffmpeg() {
    use super::super::videos::{check_ffmpeg, VideoCache};
    use crate::config::VideoOptimConfig;

    if check_ffmpeg().await.is_none() {
        eprintln!("Skipping test: ffmpeg not found on PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let dist_dir = tmp.path().join("dist");
    std::fs::create_dir_all(dist_dir.join("videos")).unwrap();

    // Generate a tiny test video.
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

    // Should have poster attribute.
    assert!(result.contains("poster="));
    // Should have preload="none".
    assert!(result.contains(r#"preload="none""#));
    // Should have VP9 source elements.
    assert!(result.contains(r#"codecs=&quot;vp9&quot;"#) || result.contains("codecs"));
    // Should have original fallback.
    assert!(result.contains("video/mp4"));
    // Original src attribute should be removed.
    assert!(!result.contains(r#"src="/videos/demo.mp4""#));
    // controls preserved.
    assert!(result.contains("controls"));
}

#[tokio::test]
async fn test_rewrite_skips_when_disabled() {
    let config = VideoOptimConfig {
        optimize: false,
        ..VideoOptimConfig::default()
    };
    let cache_dir = tempfile::TempDir::new().unwrap();
    let cache = super::super::videos::VideoCache::open(cache_dir.path()).unwrap();
    let dist_dir = cache_dir.path().join("dist");

    let html = r#"<video src="/videos/demo.mp4" controls></video>"#;
    let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir).await.unwrap();
    assert_eq!(result, html);
}

#[tokio::test]
async fn test_rewrite_preserves_explicit_preload() {
    use super::super::videos::check_ffmpeg;

    if check_ffmpeg().await.is_none() {
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

    let cache = super::super::videos::VideoCache::open(tmp.path()).unwrap();
    let result = optimize_and_rewrite_videos(html, &config, &cache, &dist_dir).await.unwrap();

    // Explicit preload="auto" should be preserved, not overridden.
    assert!(result.contains(r#"preload="auto""#));
    // Other attributes preserved.
    assert!(result.contains("autoplay"));
    assert!(result.contains("muted"));
    assert!(result.contains("loop"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib assets::video_rewrite::tests::test_optimize_and_rewrite`
Expected: FAIL — `optimize_and_rewrite_videos` does not exist.

- [ ] **Step 3: Implement optimize_and_rewrite_videos**

Add above the `#[cfg(test)]` block in `src/assets/video_rewrite.rs`:

```rust
/// Optimize videos and rewrite `<video>` elements in HTML.
///
/// Two-phase approach:
/// 1. Collect all local `<video>` src URLs.
/// 2. Optimize each video (transcode, poster, cache) and rewrite HTML.
pub async fn optimize_and_rewrite_videos(
    html: &str,
    config: &VideoOptimConfig,
    cache: &VideoCache,
    dist_dir: &Path,
) -> Result<String> {
    if !config.optimize {
        return Ok(html.to_string());
    }

    // Phase 1: Collect video sources.
    let video_srcs = collect_video_srcs(html)?;
    if video_srcs.is_empty() {
        return Ok(html.to_string());
    }

    // Filter by exclusion patterns and resolve to filesystem.
    let mut variant_map: VideoVariantMap = HashMap::new();
    for src in &video_srcs {
        let check_path = src.trim_start_matches('/');
        if is_excluded(check_path, &config.exclude) {
            tracing::debug!("Video excluded from optimization: {}", src);
            continue;
        }

        let fs_path = super::images::url_to_dist_path(src, dist_dir);
        if !fs_path.exists() {
            tracing::warn!("Video file not found, skipping: {}", fs_path.display());
            continue;
        }

        let url_prefix = super::images::url_dir_prefix(src);

        match optimize_video(&fs_path, &url_prefix, config, cache, dist_dir).await {
            Ok(variants) => {
                variant_map.insert(src.clone(), variants);
            }
            Err(e) => {
                tracing::warn!("Failed to optimize video {}: {}", src, e);
            }
        }
    }

    if variant_map.is_empty() {
        return Ok(html.to_string());
    }

    // Phase 2: Rewrite HTML.
    rewrite_video_elements(html, &variant_map)
}

/// Rewrite `<video>` elements in HTML using the variant map.
fn rewrite_video_elements(html: &str, variant_map: &VideoVariantMap) -> Result<String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let output_write = Rc::clone(&output);

    // Track which <video> elements we're currently rewriting.
    let active_src: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let active_video = Rc::clone(&active_src);
    let active_source = Rc::clone(&active_src);

    let variant_map_rc: Rc<VideoVariantMap> = Rc::new(variant_map.clone());
    let vm_video = Rc::clone(&variant_map_rc);
    let vm_source = Rc::clone(&variant_map_rc);

    let mut rewriter = lol_html::HtmlRewriter::new(
        lol_html::Settings {
            element_content_handlers: vec![
                lol_html::element!("video", move |el| {
                    // Determine the video src (from attribute or child source).
                    let src = el.get_attribute("src");

                    let matching_src = src.as_ref().and_then(|s| {
                        if vm_video.contains_key(s.as_str()) {
                            Some(s.clone())
                        } else {
                            None
                        }
                    });

                    if let Some(ref src_val) = matching_src {
                        let variants = &vm_video[src_val.as_str()];

                        // Remove src attribute (content moves to <source> children).
                        el.remove_attribute("src");

                        // Add poster.
                        el.set_attribute("poster", &variants.poster_url)
                            .map_err(|e| eyre::eyre!("Failed to set poster: {}", e))?;

                        // Set preload="none" unless explicitly set.
                        if el.get_attribute("preload").is_none() {
                            el.set_attribute("preload", "none")
                                .map_err(|e| eyre::eyre!("Failed to set preload: {}", e))?;
                        }

                        // Strip data-no-optimize if present.
                        el.remove_attribute("data-no-optimize");

                        // Prepend source elements as first children.
                        let sources = build_sources_html(variants);
                        el.prepend(&sources, lol_html::html_content::ContentType::Html);

                        *active_video.borrow_mut() = Some(src_val.clone());
                    } else {
                        // No src attribute — might have <source> children.
                        // We'll check in the source handler.
                        *active_video.borrow_mut() = None;
                    }

                    Ok(())
                }),
                // Handle <source> children inside <video> — for Form 2 and cleanup.
                lol_html::element!("video source", move |el| {
                    if let Some(src) = el.get_attribute("src") {
                        if let Some(variants) = vm_source.get(src.as_str()) {
                            // This is Form 2: <video><source src="..."></video>.
                            // If we haven't already handled this via <video src>,
                            // we need to set attributes on the parent.
                            // But lol_html doesn't give us parent access from a child handler.
                            // Instead, we replace this <source> with the full source set
                            // and handle the parent <video> in a second pass if needed.

                            // For now: if active_src is set, this source is already
                            // covered by the prepend — remove it.
                            if active_source.borrow().is_some() {
                                el.remove();
                                return Ok(());
                            }

                            // Form 2: replace this <source> with the generated sources.
                            let sources = build_sources_html(variants);
                            el.replace(&sources, lol_html::html_content::ContentType::Html);
                            *active_source.borrow_mut() = Some(src);
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

Note: For Form 2 (video with `<source>` children), we need a second pass to add `poster` and `preload` to the parent `<video>`. The simplest approach: if no `<video src>` matched but a child `<source>` did, do a lightweight second rewrite pass to add attributes to the parent `<video>`. Update `optimize_and_rewrite_videos` to handle this:

After the first rewrite, check if any Form 2 videos need parent attribute updates. The simplest correct implementation: during `collect_video_srcs`, also record which form each video uses. Then in the rewrite pass, handle Form 2 `<video>` elements by matching on their child source's URL. Here's the updated approach — replace the `rewrite_video_elements` function with one that handles both forms in a single pass using a lookup by source URL:

```rust
fn rewrite_video_elements(html: &str, variant_map: &VideoVariantMap) -> Result<String> {
    // For Form 2 handling, we need to know which parent <video> elements
    // contain <source> children that map to our variant map.
    // Strategy: two-pass.
    // Pass 1: rewrite <video src="..."> (Form 1) and <source> elements.
    // Pass 2: add poster/preload to <video> parents of rewritten Form 2 sources.

    let form2_srcs: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let output1 = Rc::new(RefCell::new(Vec::new()));
    let output1_write = Rc::clone(&output1);

    let vm1 = Rc::new(variant_map.clone());
    let vm1_video = Rc::clone(&vm1);
    let vm1_source = Rc::clone(&vm1);
    let form2_collect = Rc::clone(&form2_srcs);

    // Track whether the current <video> was handled via Form 1.
    let handled_form1: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let hf1_video = Rc::clone(&handled_form1);
    let hf1_source = Rc::clone(&handled_form1);

    let mut rewriter = lol_html::HtmlRewriter::new(
        lol_html::Settings {
            element_content_handlers: vec![
                lol_html::element!("video", move |el| {
                    *hf1_video.borrow_mut() = false;

                    if let Some(src) = el.get_attribute("src") {
                        if let Some(variants) = vm1_video.get(src.as_str()) {
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
                            *hf1_video.borrow_mut() = true;
                        }
                    }
                    Ok(())
                }),
                lol_html::element!("video source", move |el| {
                    if *hf1_source.borrow() {
                        // Parent was Form 1 — remove original <source> children.
                        el.remove();
                        return Ok(());
                    }

                    if let Some(src) = el.get_attribute("src") {
                        if let Some(variants) = vm1_source.get(src.as_str()) {
                            let sources = build_sources_html(variants);
                            el.replace(&sources, lol_html::html_content::ContentType::Html);
                            form2_collect.borrow_mut().push(src);
                        }
                    }
                    Ok(())
                }),
            ],
            ..lol_html::Settings::new()
        },
        move |chunk: &[u8]| {
            output1_write.borrow_mut().extend_from_slice(chunk);
        },
    );

    rewriter
        .write(html.as_bytes())
        .map_err(|e| eyre::eyre!("HTML rewrite error: {}", e))?;
    rewriter
        .end()
        .map_err(|e| eyre::eyre!("HTML rewrite finalize error: {}", e))?;

    let pass1_bytes = output1.borrow().clone();
    let pass1_html = String::from_utf8(pass1_bytes)
        .wrap_err("HTML rewrite produced invalid UTF-8")?;

    // Pass 2: add poster/preload to Form 2 parent <video> elements.
    let form2_list = form2_srcs.borrow();
    if form2_list.is_empty() {
        return Ok(pass1_html);
    }

    let vm2 = variant_map.clone();
    let output2 = Rc::new(RefCell::new(Vec::new()));
    let output2_write = Rc::clone(&output2);

    // For Form 2, the <video> element itself was not modified in pass 1.
    // We need to find <video> elements that don't have poster yet and whose
    // content contains our rewritten sources.
    // Simplest approach: any <video> without poster that doesn't have src.
    let form2_variants: HashMap<String, &VideoVariants> = form2_list
        .iter()
        .filter_map(|src| vm2.get(src.as_str()).map(|v| (src.clone(), v)))
        .collect();

    // We know there's exactly one set of variants per Form 2 video,
    // so we can use the first form2 source's poster as a simple heuristic.
    // More precisely: match by checking if the rewritten content contains
    // the variant URLs.
    let form2_posters: Vec<(&str, &str)> = form2_variants
        .values()
        .map(|v| (v.vp9.first().map(|vv| vv.url_path.as_str()).unwrap_or(""), v.poster_url.as_str()))
        .collect();

    let mut rewriter2 = lol_html::HtmlRewriter::new(
        lol_html::Settings {
            element_content_handlers: vec![
                lol_html::element!("video", move |el| {
                    if el.get_attribute("poster").is_none()
                        && el.get_attribute("src").is_none()
                    {
                        // This is likely a Form 2 video. Find the matching poster.
                        for (_, poster) in &form2_posters {
                            el.set_attribute("poster", poster)
                                .map_err(|e| eyre::eyre!("{}", e))?;
                            if el.get_attribute("preload").is_none() {
                                el.set_attribute("preload", "none")
                                    .map_err(|e| eyre::eyre!("{}", e))?;
                            }
                            el.remove_attribute("data-no-optimize");
                            break;
                        }
                    }
                    Ok(())
                }),
            ],
            ..lol_html::Settings::new()
        },
        move |chunk: &[u8]| {
            output2_write.borrow_mut().extend_from_slice(chunk);
        },
    );

    rewriter2
        .write(pass1_html.as_bytes())
        .map_err(|e| eyre::eyre!("HTML rewrite pass 2 error: {}", e))?;
    rewriter2
        .end()
        .map_err(|e| eyre::eyre!("HTML rewrite pass 2 finalize error: {}", e))?;

    let bytes = output2.borrow().clone();
    String::from_utf8(bytes).wrap_err("HTML rewrite pass 2 produced invalid UTF-8")
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib assets::video_rewrite::tests`
Expected: All tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/assets/video_rewrite.rs
git commit -m "feat: implement optimize_and_rewrite_videos with two-phase HTML rewriting"
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

In `optimize_fragment_images()` (around line 1302), rename it to `optimize_fragment_assets()` or add a separate function. Add video rewriting after image rewriting for each fragment:

```rust
fn optimize_fragment_assets(
    frags: &[fragments::Fragment],
    image_config: &crate::config::ImageOptimConfig,
    image_cache: &ImageCache,
    video_config: &crate::config::VideoOptimConfig,
    video_cache: &VideoCache,
    ffmpeg_available: bool,
    dist_dir: &Path,
) -> Result<Vec<fragments::Fragment>> {
    let rt = tokio::runtime::Handle::current();
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
            rt.block_on(assets::optimize_and_rewrite_videos(
                &optimized_html,
                video_config,
                video_cache,
                dist_dir,
            ))?
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

Update the call site (around line 691) to pass the additional parameters.

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
