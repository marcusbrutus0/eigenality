# Video Optimization Design

## Summary

Add automatic video compression and optimization to eigen-generated websites. When a `<video>` element is found in rendered HTML and not excluded, eigen:

1. Transcodes the video to VP9/WebM at multiple resolution tiers using ffmpeg.
2. Extracts the first frame as a WebP poster image.
3. Rewrites the `<video>` element with `poster`, `preload="none"`, VP9 `<source>` elements, and the original file as fallback.

FFmpeg is a runtime dependency (not bundled). If absent, eigen warns once and skips all video optimization.

## Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| FFmpeg integration | CLI subprocess (`tokio::process::Command`) | Zero compile-time dependency, simpler builds, graceful degradation |
| Poster strategy | Native `poster` attribute + `preload="none"` | No JS required, semantically correct, browser-native |
| Output codec | VP9 only, original as fallback | Good compression, near-universal support, keeps scope tight |
| Resolution strategy | Configurable height list, default `[480, 720, 1080]` | Mirrors image pipeline pattern, skips tiers above source |
| Exclusion mechanism | Config globs + `data-no-optimize` attribute | Consistent with image exclusion patterns |
| HTML normalization | Both `<video src>` and `<video><source>` → multi-`<source>` output | Template authors don't need to think about form |
| Caching | Content-hash at `.eigen_cache/videos/` | Mirrors image cache, avoids expensive re-encodes |
| Architecture | Parallel module alongside images (Approach A) | Proven patterns, no refactoring of stable code |

## Configuration

In `site.toml` under `[assets.videos]`:

```toml
[assets.videos]
optimize = true              # Master switch (default: true)
format = "vp9"               # Target codec (default: "vp9")
quality = 30                 # CRF value for VP9, 0-63, lower=better (default: 30)
heights = [480, 720, 1080]   # Resolution tiers in pixels (default: [480, 720, 1080])
exclude = []                 # Glob patterns to exclude (default: [])
poster_quality = 80          # WebP quality for poster frames, 1-100 (default: 80)
```

Added to `AssetsConfig`:

```rust
pub struct AssetsConfig {
    pub localize: bool,
    pub cdn_skip_hosts: Vec<String>,
    pub cdn_allow_hosts: Vec<String>,
    pub images: ImageOptimConfig,
    pub videos: VideoOptimConfig,  // new
}
```

## Video Processing Module (`src/assets/videos.rs`)

### FFmpeg Detection

- Run `ffmpeg -version` via `tokio::process::Command` once at build start.
- Store result as `ffmpeg_available: bool` in build context.
- If absent: `warn!("ffmpeg not found on PATH, video optimization disabled")`.

### Data Structures

```rust
pub struct VideoVariant {
    pub url_path: String,     // e.g., /assets/demo-720p.webm
    pub height: u32,
    pub mime_type: String,    // video/webm
    pub codec: String,        // vp9
}

pub struct VideoVariants {
    pub original_width: u32,
    pub original_height: u32,
    pub vp9: Vec<VideoVariant>,    // sorted by height ascending
    pub original: VideoVariant,     // original file as fallback
    pub poster_url: String,         // poster webp path
}
```

### VideoCache

- Location: `.eigen_cache/videos/`
- Key format: `{source_hash}-{height}p-{codec}.{ext}` (e.g., `a1b2c3-720p-vp9.webm`)
- Poster key: `{source_hash}-poster-{height}p.webp` (height of poster matches source or largest tier)
- API: `get(key) -> Option<Vec<u8>>`, `put(key, &[u8]) -> Result<()>`

### `optimize_video()` Function

1. Hash source file bytes (`sha2`) for cache key prefix.
2. Probe source dimensions: `ffprobe -v quiet -print_format json -show_streams {src}`.
3. For each configured height (skip if >= source height; always include source height as a tier):
   - Check cache → if hit, copy to dist and continue.
   - Transcode:
     ```
     ffmpeg -i {src} -c:v libvpx-vp9 -crf {quality} -b:v 0 \
       -vf scale=-2:{height} -c:a libopus -b:a 128k {out.webm}
     ```
   - `-b:v 0` with `-crf` = constant-quality mode (standard VP9 approach).
   - `-scale=-2:{height}` keeps aspect ratio, width divisible by 2.
   - Cache the result.
4. Extract poster frame:
   ```
   ffmpeg -i {src} -vframes 1 -f image2 -c:v libwebp -quality {poster_quality} {out.webp}
   ```
5. Copy original file to dist as fallback.
6. Return `VideoVariants`.

## HTML Rewriting (`src/assets/video_rewrite.rs`)

### Two-Phase Approach

**Phase 1 — Collect:** Scan HTML with `lol_html` for `<video>` elements.

For each `<video>`:
- Extract video path from `src` attribute (Form 1) or child `<source src>` (Form 2).
- Skip if: external URL (http/https), path matches exclude globs, `data-no-optimize` present.
- Collect into processing list.

**Phase 2 — Process & Rewrite:** For each collected video:
- Call `optimize_video()`.
- Rewrite the `<video>` element.

### Rewriting Rules

**Input (either form):**
```html
<video src="/videos/demo.mp4" controls></video>
<!-- or -->
<video controls>
  <source src="/videos/demo.mp4" type="video/mp4">
</video>
```

**Output:**
```html
<video poster="/assets/demo-poster.webp" preload="none" controls>
  <source src="/assets/demo-1080p.webm" type='video/webm; codecs="vp9"'>
  <source src="/assets/demo-720p.webm" type='video/webm; codecs="vp9"'>
  <source src="/assets/demo-480p.webm" type='video/webm; codecs="vp9"'>
  <source src="/assets/demo.mp4" type="video/mp4">
</video>
```

**Attribute handling:**
- Remove `src` from `<video>` if present (content moves to `<source>` children).
- Add `poster` attribute with extracted first-frame WebP path.
- Set `preload="none"` unless user explicitly set a different `preload` value.
- Remove existing `<source>` children, replace with generated set.
- VP9 sources ordered highest-to-lowest resolution; original fallback last.
- Preserve all other attributes (`controls`, `autoplay`, `muted`, `loop`, `class`, `id`, `width`, `height`, etc.).
- Strip `data-no-optimize` from excluded elements (clean output).

## Pipeline Integration

### Render Pipeline Position

In `src/build/render.rs`, after CSS background image rewriting, before plugin hooks:

```
image optimization (existing)
CSS background image rewriting (existing)
→ video optimization (new)
plugin post-render hooks (existing)
```

### Build Context

- Add `VideoCache` to shared build context (alongside `ImageCache`).
- Add `ffmpeg_available: bool` flag, checked once at startup.
- Both initialized at build start.

### Fragment Handling

Video optimization runs on fragments too, same as images.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| ffmpeg not found | Single warning at build start, all video optimization skipped |
| ffmpeg fails on specific video | `warn!` with path and stderr, skip that video, leave `<video>` untouched |
| ffprobe fails | Same as above — skip video, leave untouched |
| No videos on page | No-op, zero cost |
| Corrupt/unsupported source | Warn and skip |

## Logging

- Build start: `info!("Video optimization: ffmpeg found at {path}")` or `warn!("ffmpeg not found...")`
- Per video: `info!("Processing video: {path} ({w}x{h} → {tiers} VP9)")`
- Cache hit: `debug!("Video cache hit: {key}")`
- Completion: `info!("Video optimization: {n} videos processed for {page}")`

## Files

### New Files

| File | Purpose |
|------|---------|
| `src/assets/videos.rs` | VideoCache, VideoVariant, VideoVariants, optimize_video(), ffmpeg detection, poster extraction |
| `src/assets/video_rewrite.rs` | optimize_and_rewrite_videos(), HTML collection and rewriting with lol_html |
| `docs/video_optimization.md` | Feature documentation |

### Modified Files

| File | Change |
|------|--------|
| `src/assets/mod.rs` | Add `pub mod videos; pub mod video_rewrite;`, re-export `optimize_and_rewrite_videos` |
| `src/config/mod.rs` | Add `VideoOptimConfig`, add `videos` field to `AssetsConfig`, default functions |
| `src/build/render.rs` | Add video optimization step after CSS background image rewriting |
| `src/build/mod.rs` (or build context) | Initialize `VideoCache` and `ffmpeg_available` at build start |
| `flake.nix` | Add `ffmpeg` to `packages` list |

### No New Cargo Dependencies

FFmpeg is called via `tokio::process::Command`. All other dependencies (`lol_html`, `sha2`, `tokio`) are already present.

### No Changes To

- Image pipeline
- Template rendering
- Plugin system
- Any other existing asset processing
