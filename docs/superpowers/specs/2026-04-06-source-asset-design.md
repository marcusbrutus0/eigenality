# Design Spec: `source_asset` — Authenticated Asset Downloads

**Date:** 2026-04-06
**Status:** Draft

## Problem

Data sources (CMSes, APIs) return JSON containing image URLs. Those images
often require the same authentication headers as the data source itself. Today,
eigen's asset localization pipeline downloads remote images with a plain HTTP
client — no auth. Templates have no way to say "this image URL belongs to
source X, use its credentials."

## Solution

A new template function `source_asset(source_name, url)` that:

- **Build time:** downloads the image using the named source's headers, saves
  it to `dist/assets/`, and returns the local `/assets/...` path. Integrates
  with the existing `localize_assets` pipeline.
- **Dev time:** returns a `/_proxy/{source_name}/...` URL so the browser
  fetches through the existing auth-injecting dev proxy.

No automatic hostname matching. The template author explicitly names which
source owns the URL. Simple, predictable, no magic.

## Template Usage

```jinja
{# Explicit: name the source that owns the image #}
<img src="{{ source_asset('my_cms', item.image_url) }}">

{# Works with any expression that produces a URL #}
<img src="{{ source_asset('my_cms', item.hero.formats.large.url) }}">
```

**Arguments:**
1. `source_name` (string, required) — must match a key in `[sources.*]`
2. `url` (string, required) — absolute URL to the image/asset

**Returns:** a string path — either a local `/assets/...` path (build) or
a `/_proxy/...` URL (dev).

**Errors:**
- Unknown source name → template render error with available source names
- Empty or non-HTTP URL → template render error

## Architecture

### Build-Time Flow

```
Template renders source_asset("my_cms", "https://cms.example.com/img/photo.jpg")
  │
  ├─ Look up "my_cms" in sources config → get headers (Authorization, etc.)
  │
  ├─ Check asset cache (keyed by URL)
  │   ├─ Cache hit + fresh → return cached local path
  │   └─ Cache miss or stale ↓
  │
  ├─ Download with source headers (ETag/If-None-Match for conditional)
  │
  ├─ Save to asset cache → copy to dist/assets/
  │
  └─ Return "/assets/{local_filename}"
```

The function needs access to:
- `SiteConfig.sources` — to look up headers
- `AssetCache` + `reqwest::blocking::Client` — to download
- The dist directory path — to copy assets into

Since minijinja closures must be `Fn` (not `FnMut`), and downloading requires
mutable cache state, the function collects *requests* during rendering and a
post-render pass does the actual downloads. This matches how `localize_assets`
already works — it runs after rendering.

**Two-phase approach:**

1. **During template rendering:** `source_asset()` records the `(source_name,
   url)` pair into a shared `Arc<Mutex<Vec<SourceAssetRequest>>>` and returns a
   placeholder URL (the original URL, unchanged).

2. **After rendering:** a new `resolve_source_assets()` function iterates the
   collected requests, downloads each with the correct source headers, saves to
   `dist/assets/`, and rewrites the URLs in the rendered HTML — same string
   replacement approach as `localize_assets`.

This keeps the template function pure and side-effect-free during rendering.

### Dev-Time Flow

```
Template renders source_asset("my_cms", "https://cms.example.com/img/photo.jpg")
  │
  ├─ Look up "my_cms" in sources config (validate it exists)
  │
  ├─ Extract the path portion of the URL
  │   "https://cms.example.com/img/photo.jpg" → "/img/photo.jpg"
  │
  └─ Return "/_proxy/my_cms/img/photo.jpg"
```

In dev mode, no downloading happens. The browser requests
`/_proxy/my_cms/img/photo.jpg`, and the existing `proxy_handler` forwards it to
`https://cms.example.com/img/photo.jpg` with the configured auth headers. The
proxy already handles this correctly — no changes needed to the proxy itself.

For URLs on a different host than the source base URL (e.g., source URL is
`https://api.example.com` but image is on `https://media.example.com`), the
proxy must use the **full original URL** rather than appending a path to the
source base. The dev proxy URL encodes this as:

```
/_proxy/my_cms/__source_asset__/https://media.example.com/img/photo.jpg
```

The proxy handler detects the `__source_asset__/` prefix and uses the remainder
as the full target URL instead of appending to the source base URL. The double-
underscore prefix avoids collision with real API paths.

### Mode Detection

`register_functions` doesn't currently know whether it's a dev or production
build. A new boolean parameter `dev_mode: bool` is added:

```rust
pub fn register_functions(
    env: &mut Environment<'_>,
    config: &SiteConfig,
    manifest: Option<Arc<AssetManifest>>,
    dev_mode: bool,
)
```

- **Build path** (`render.rs`): passes `false`
- **Dev path** (`rebuild.rs`): passes `true`

In dev mode, `source_asset()` returns the proxy URL directly — no collection,
no post-render pass.

In build mode, `source_asset()` records the request and returns the original
URL as a placeholder for the post-render rewrite pass.

## Components

### 1. `SourceAssetRequest` struct

```rust
pub struct SourceAssetRequest {
    pub source_name: String,
    pub url: String,
}
```

Collected during template rendering (build mode only).

### 2. `source_asset` template function

Registered in `register_functions`. Closure captures:
- `sources: HashMap<String, SourceConfig>` (cloned from config)
- `dev_mode: bool`
- `requests: Arc<Mutex<Vec<SourceAssetRequest>>>` (build mode only)

### 3. `resolve_source_assets()` function

New function in a new file `src/build/source_asset.rs`:

```rust
pub fn resolve_source_assets(
    html: &str,
    requests: &[SourceAssetRequest],
    sources: &HashMap<String, SourceConfig>,
    cache: &mut AssetCache,
    client: &reqwest::blocking::Client,
    dist_dir: &Path,
) -> Result<String>
```

- Deduplicates requests by URL
- For each unique URL, builds headers from the named source
- Downloads via `download::ensure_asset`-style logic but with auth headers
- Copies to `dist/assets/`
- Rewrites URLs in the HTML string

Called after `localize_assets` in the render pipeline.

### 4. Dev proxy update

Small change to `proxy_handler` in `src/dev/proxy.rs`: detect the `__source_asset__/`
path prefix. When present, use the remainder as the full target URL instead of
appending to the source base URL.

```
/_proxy/my_cms/__source_asset__/https://media.example.com/path → GET https://media.example.com/path
/_proxy/my_cms/api/items                            → GET https://api.example.com/api/items (existing behavior)
```

## Integration Points

### Render pipeline (`src/build/render.rs`)

After the existing `localize_assets` call, add `resolve_source_assets`:

```rust
// Existing: localize unauthenticated assets
html = assets::localize_assets(&html, &config.assets, &mut cache, &client, dist_dir)?;

// New: resolve authenticated source assets
html = source_asset::resolve_source_assets(
    &html, &collected_requests, &config.sources, &mut cache, &client, dist_dir,
)?;
```

### Template environment (`src/template/environment.rs`)

`setup_environment` gains `dev_mode: bool` parameter, passes it through to
`register_functions`.

### Dev rebuild (`src/dev/rebuild.rs`)

Passes `dev_mode: true` to `setup_environment`. No post-render
`resolve_source_assets` call needed — proxy URLs are final.

## Config

No new configuration required. `source_asset()` uses the existing
`[sources.*]` config as-is:

```toml
[sources.my_cms]
url = "https://cms.example.com"
headers = { Authorization = "Bearer ${CMS_TOKEN}" }
```

## Error Handling

| Condition | Behavior |
|---|---|
| Unknown source name | Template render error: `"Unknown source 'foo'. Available: my_cms, blog_api"` |
| Empty URL | Template render error: `"source_asset: url must be a non-empty HTTP(S) URL"` |
| Non-HTTP URL | Template render error (same as above) |
| Download fails (build) | Warning log, original URL left in place (same as `localize_assets`) |
| Proxy fails (dev) | 502 Bad Gateway from proxy (existing behavior) |

## Testing

1. **Unit tests for `source_asset` function** — dev mode returns proxy URL,
   build mode records request and returns original URL
2. **Unit tests for `resolve_source_assets`** — URL rewriting, deduplication,
   handles missing source gracefully
3. **Unit tests for proxy `_full/` routing** — full URL forwarding
4. **Integration test** — end-to-end with a mock HTTP server that requires
   auth, verifying the image ends up in `dist/assets/` with correct content

## Files to Create/Modify

| File | Change |
|---|---|
| `src/build/source_asset.rs` | **New.** `SourceAssetRequest`, `resolve_source_assets()` |
| `src/build/mod.rs` | Add `pub mod source_asset;` |
| `src/template/functions.rs` | Add `source_asset` function registration, `dev_mode` param |
| `src/template/environment.rs` | Add `dev_mode` param, pass through |
| `src/build/render.rs` | Call `resolve_source_assets` after `localize_assets` |
| `src/dev/rebuild.rs` | Pass `dev_mode: true` to `setup_environment` |
| `src/dev/proxy.rs` | Handle `__source_asset__/` prefix for cross-host proxying |
| `src/assets/download.rs` | Extract/expose header-accepting download variant |
| `docs/source_asset.md` | User-facing documentation |
