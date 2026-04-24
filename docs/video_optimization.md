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
quality = 30                 # VP9 CRF value, 0-63, lower=better (default: 30)
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

The browser picks the first `<source>` it can play (VP9 webm), falling back to the original mp4.

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
