# `source_asset` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `source_asset(source_name, url_or_path)` template function that downloads authenticated assets at build time and proxies them through the dev server at dev time.

**Architecture:** A two-phase approach: during template rendering, `source_asset()` collects download requests (build mode) or returns proxy URLs (dev mode). After rendering, a post-render pass downloads assets with source auth headers and rewrites URLs. The existing dev proxy is extended with a `__source_asset__/` prefix for cross-host image forwarding.

**Tech Stack:** Rust, minijinja (template functions), reqwest (HTTP), axum (dev proxy)

---

## File Map

| File | Responsibility |
|---|---|
| `src/build/source_asset.rs` | **New.** `SourceAssetRequest` struct, `SourceAssetCollector` (Arc+Mutex wrapper), `resolve_source_assets()` post-render function |
| `src/build/mod.rs` | Add `pub mod source_asset;` |
| `src/template/functions.rs` | Register `source_asset` function, add `dev_mode` parameter |
| `src/template/environment.rs` | Thread `dev_mode` parameter through to `register_functions` |
| `src/build/render.rs` | Create collector, pass to environment setup, call `resolve_source_assets` after `localize_assets` |
| `src/dev/rebuild.rs` | Pass `dev_mode: true` to `setup_environment` |
| `src/dev/proxy.rs` | Handle `__source_asset__/` prefix for full-URL forwarding |
| `src/assets/download.rs` | Add `download_asset_with_headers()` variant that accepts extra headers |
| `docs/source_asset.md` | User-facing documentation |

---

### Task 1: Auth-aware asset download function

Add a variant of `download_asset` that accepts custom headers (for source auth).

**Files:**
- Modify: `src/assets/download.rs:24-87` (refactor `download_asset`)

- [ ] **Step 1: Write the failing test**

In `src/assets/download.rs`, add at the end of the file (before any existing `#[cfg(test)]` block, or create one):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    #[test]
    fn download_asset_with_headers_sends_custom_headers() {
        // Start a mock server that requires Authorization.
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let url = format!("http://{}/image.png", addr);

        let handle = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            let auth = req.headers().iter()
                .find(|h| h.field.as_str().eq_ignore_ascii_case("authorization"))
                .map(|h| h.value.to_string());
            assert_eq!(auth.as_deref(), Some("Bearer secret123"));
            let response = tiny_http::Response::from_data(b"PNG_DATA".to_vec());
            req.respond(response).unwrap();
        });

        let client = reqwest::blocking::Client::new();
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer secret123"),
        );

        let result = download_asset_with_headers(&client, &url, None, &headers).unwrap();
        match result {
            DownloadResult::Downloaded { data, .. } => {
                assert_eq!(data, b"PNG_DATA");
            }
            DownloadResult::NotModified => panic!("Expected Downloaded"),
        }

        handle.join().unwrap();
    }
}
```

- [ ] **Step 2: Check that tiny_http is available as a dev dependency**

Run: `grep tiny_http Cargo.toml`

If not present, add it:

```bash
cargo add tiny_http --dev
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib assets::download::tests::download_asset_with_headers_sends_custom_headers -- --nocapture`

Expected: FAIL — `download_asset_with_headers` does not exist.

- [ ] **Step 4: Implement `download_asset_with_headers`**

In `src/assets/download.rs`, add after `download_asset`:

```rust
/// Download an asset URL with extra headers (e.g. source auth).
///
/// Behaves identically to `download_asset` but merges `extra_headers`
/// into the request. Source-configured headers (Authorization, API keys)
/// are passed here.
pub fn download_asset_with_headers(
    client: &reqwest::blocking::Client,
    url: &str,
    cached_meta: Option<&AssetCacheMeta>,
    extra_headers: &reqwest::header::HeaderMap,
) -> Result<DownloadResult> {
    let mut request = client.get(url);

    // Add auth / custom headers.
    for (name, value) in extra_headers {
        request = request.header(name.clone(), value.clone());
    }

    // Add conditional headers if we have cached metadata.
    if let Some(meta) = cached_meta {
        if let Some(ref etag) = meta.etag {
            request = request.header("If-None-Match", etag.as_str());
        }
        if let Some(ref last_mod) = meta.last_modified {
            request = request.header("If-Modified-Since", last_mod.as_str());
        }
    }

    let response = request.send()
        .wrap_err_with(|| format!("Failed to download asset: {}", url))?;

    let status = response.status();

    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(DownloadResult::NotModified);
    }

    if !status.is_success() {
        bail!("HTTP {} downloading asset: {}", status, url);
    }

    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let last_modified = response
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());

    let data = response.bytes()
        .wrap_err_with(|| format!("Failed to read asset body: {}", url))?
        .to_vec();

    let local_filename = local_filename_for_url(url);

    Ok(DownloadResult::Downloaded {
        data,
        local_filename,
        etag,
        last_modified,
        content_type,
    })
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib assets::download::tests::download_asset_with_headers_sends_custom_headers -- --nocapture`

Expected: PASS

- [ ] **Step 6: Also add `ensure_asset_with_headers`**

In `src/assets/download.rs`, add after `ensure_asset`:

```rust
/// Like `ensure_asset` but passes extra headers (source auth) to the download.
pub fn ensure_asset_with_headers(
    client: &reqwest::blocking::Client,
    cache: &mut AssetCache,
    url: &str,
    extra_headers: &reqwest::header::HeaderMap,
) -> Result<String> {
    let cached_meta = cache.get(url).cloned();

    if let Some(ref meta) = cached_meta {
        if cache.has_file(url) {
            match download_asset_with_headers(client, url, Some(meta), extra_headers)? {
                DownloadResult::NotModified => {
                    tracing::debug!("  Source asset not modified (304): {}", url);
                    return Ok(meta.local_filename.clone());
                }
                DownloadResult::Downloaded {
                    data,
                    local_filename,
                    etag,
                    last_modified,
                    content_type,
                } => {
                    tracing::debug!("  Source asset re-downloaded: {}", url);
                    let final_name = cache.store(url, &data, &local_filename, etag, last_modified, content_type)?;
                    return Ok(final_name);
                }
            }
        }
    }

    match download_asset_with_headers(client, url, None, extra_headers)? {
        DownloadResult::Downloaded {
            data,
            local_filename,
            etag,
            last_modified,
            content_type,
        } => {
            tracing::debug!("  Source asset downloaded: {} → {}", url, local_filename);
            let final_name = cache.store(url, &data, &local_filename, etag, last_modified, content_type)?;
            Ok(final_name)
        }
        DownloadResult::NotModified => {
            bail!("Unexpected 304 for {}", url);
        }
    }
}
```

- [ ] **Step 7: Run full test suite for assets module**

Run: `cargo test --lib assets`

Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/assets/download.rs Cargo.toml Cargo.lock
git commit -m "feat(assets): add auth-aware download_asset_with_headers and ensure_asset_with_headers"
```

---

### Task 2: SourceAssetRequest and SourceAssetCollector

Create the core types for collecting source asset requests during rendering.

**Files:**
- Create: `src/build/source_asset.rs`
- Modify: `src/build/mod.rs:1-25`

- [ ] **Step 1: Write the test**

Create `src/build/source_asset.rs` with the types and tests:

```rust
//! Authenticated asset downloads from configured data sources.
//!
//! During template rendering, `source_asset()` collects download requests.
//! After rendering, `resolve_source_assets()` downloads each asset with the
//! source's auth headers and rewrites URLs in the rendered HTML.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A request to download an asset using a named source's auth headers.
#[derive(Debug, Clone)]
pub struct SourceAssetRequest {
    /// Source name (key in `[sources.*]`).
    pub source_name: String,
    /// Fully resolved absolute URL.
    pub url: String,
}

/// Thread-safe collector for source asset requests during rendering.
///
/// Shared via `Arc` between the template function closure and the caller.
#[derive(Debug, Clone, Default)]
pub struct SourceAssetCollector {
    requests: Arc<Mutex<Vec<SourceAssetRequest>>>,
}

impl SourceAssetCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a request. Called from the `source_asset` template function.
    pub fn push(&self, source_name: String, url: String) {
        let mut reqs = self.requests.lock().expect("collector lock poisoned");
        reqs.push(SourceAssetRequest { source_name, url });
    }

    /// Drain all collected requests.
    pub fn drain(&self) -> Vec<SourceAssetRequest> {
        let mut reqs = self.requests.lock().expect("collector lock poisoned");
        std::mem::take(&mut *reqs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_push_and_drain() {
        let collector = SourceAssetCollector::new();
        collector.push("cms".into(), "https://cms.example.com/img.jpg".into());
        collector.push("cms".into(), "https://cms.example.com/img2.jpg".into());

        let requests = collector.drain();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].source_name, "cms");
        assert_eq!(requests[0].url, "https://cms.example.com/img.jpg");

        // drain returns empty after first call
        assert!(collector.drain().is_empty());
    }

    #[test]
    fn collector_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SourceAssetCollector>();
    }
}
```

- [ ] **Step 2: Register the module**

In `src/build/mod.rs`, add after `pub mod sitemap;` (line 21):

```rust
pub mod source_asset;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib build::source_asset`

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/build/source_asset.rs src/build/mod.rs
git commit -m "feat(build): add SourceAssetRequest and SourceAssetCollector types"
```

---

### Task 3: resolve_source_assets post-render function

The function that downloads collected assets and rewrites URLs in rendered HTML.

**Files:**
- Modify: `src/build/source_asset.rs`

- [ ] **Step 1: Write the test**

Add to the `tests` module in `src/build/source_asset.rs`:

```rust
    #[test]
    fn resolve_rewrites_urls_in_html() {
        use crate::assets::cache::AssetCache;
        use crate::config::SourceConfig;

        // Start a mock server that requires auth.
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let base_url = format!("http://{}", addr);
        let image_url = format!("{}/uploads/photo.jpg", base_url);

        let handle = {
            let image_url_clone = image_url.clone();
            std::thread::spawn(move || {
                let req = server.recv().unwrap();
                // Verify auth header was sent.
                let auth = req.headers().iter()
                    .find(|h| h.field.as_str().eq_ignore_ascii_case("authorization"))
                    .map(|h| h.value.to_string());
                assert_eq!(auth.as_deref(), Some("Bearer tok123"));

                let response = tiny_http::Response::from_data(b"JPEG_DATA".to_vec())
                    .with_header(
                        tiny_http::Header::from_bytes("content-type", "image/jpeg").unwrap(),
                    );
                req.respond(response).unwrap();
            })
        };

        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path();
        let dist_dir = project_root.join("dist");
        std::fs::create_dir_all(&dist_dir).unwrap();

        let mut cache = AssetCache::open(project_root).unwrap();
        let client = reqwest::blocking::Client::new();

        let mut sources = HashMap::new();
        sources.insert("my_cms".to_string(), SourceConfig {
            url: base_url.clone(),
            headers: HashMap::from([
                ("Authorization".to_string(), "Bearer tok123".to_string()),
            ]),
        });

        let html = format!(r#"<html><body><img src="{}"></body></html>"#, image_url);
        let requests = vec![SourceAssetRequest {
            source_name: "my_cms".to_string(),
            url: image_url.clone(),
        }];

        let result = resolve_source_assets(
            &html, &requests, &sources, &mut cache, &client, &dist_dir,
        ).unwrap();

        handle.join().unwrap();

        // The URL should have been rewritten to /assets/...
        assert!(!result.contains(&image_url));
        assert!(result.contains("/assets/"));

        // The file should exist in dist/assets/.
        let assets_dir = dist_dir.join("assets");
        assert!(assets_dir.exists());
        let files: Vec<_> = std::fs::read_dir(&assets_dir).unwrap().collect();
        assert_eq!(files.len(), 1);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib build::source_asset::tests::resolve_rewrites_urls_in_html -- --nocapture`

Expected: FAIL — `resolve_source_assets` does not exist.

- [ ] **Step 3: Implement `resolve_source_assets`**

Add to `src/build/source_asset.rs`, after `SourceAssetCollector`:

```rust
use std::path::Path;
use eyre::{Result, WrapErr};

use crate::assets::cache::AssetCache;
use crate::assets::download;
use crate::config::SourceConfig;

/// Download collected source assets and rewrite their URLs in the HTML.
///
/// Called after `localize_assets` in the render pipeline. Each request is
/// downloaded with the named source's configured headers (auth, API keys).
pub fn resolve_source_assets(
    html: &str,
    requests: &[SourceAssetRequest],
    sources: &HashMap<String, SourceConfig>,
    cache: &mut AssetCache,
    client: &reqwest::blocking::Client,
    dist_dir: &Path,
) -> Result<String> {
    if requests.is_empty() {
        return Ok(html.to_string());
    }

    let dist_assets_dir = dist_dir.join("assets");

    // Deduplicate by URL, keeping the first source_name for each.
    let mut seen = HashMap::new();
    for req in requests {
        seen.entry(req.url.clone())
            .or_insert_with(|| req.source_name.clone());
    }

    let mut url_map: HashMap<String, String> = HashMap::new();

    for (url, source_name) in &seen {
        let source = match sources.get(source_name) {
            Some(s) => s,
            None => {
                tracing::warn!(
                    "source_asset: source '{}' not found, skipping URL: {}",
                    source_name, url
                );
                continue;
            }
        };

        // Build header map from source config.
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, val) in &source.headers {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(val),
            ) {
                headers.insert(name, value);
            }
        }

        match download::ensure_asset_with_headers(client, cache, url, &headers) {
            Ok(local_filename) => {
                cache.copy_to_dist(url, &dist_assets_dir)
                    .wrap_err_with(|| {
                        format!("Failed to copy source asset to dist: {}", url)
                    })?;
                let local_path = format!("/assets/{}", local_filename);
                url_map.insert(url.clone(), local_path);
            }
            Err(e) => {
                tracing::warn!("Failed to download source asset {}: {:#}", url, e);
            }
        }
    }

    if url_map.is_empty() {
        return Ok(html.to_string());
    }

    // Simple string replacement (same approach as localize_assets rewrite_urls).
    let mut result = html.to_string();
    for (original, local) in &url_map {
        result = result.replace(original, local);
    }
    Ok(result)
}
```

- [ ] **Step 4: Add required imports at the top of the file**

Update the imports at the top of `src/build/source_asset.rs`:

```rust
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use eyre::{Result, WrapErr};

use crate::assets::cache::AssetCache;
use crate::assets::download;
use crate::config::SourceConfig;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib build::source_asset::tests::resolve_rewrites_urls_in_html -- --nocapture`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/build/source_asset.rs
git commit -m "feat(build): add resolve_source_assets post-render function"
```

---

### Task 4: URL resolution helper

Resolve relative paths against source base URL.

**Files:**
- Modify: `src/build/source_asset.rs`

- [ ] **Step 1: Write the tests**

Add to the `tests` module in `src/build/source_asset.rs`:

```rust
    #[test]
    fn resolve_url_absolute_passthrough() {
        let result = resolve_url("https://cdn.example.com/img.jpg", "https://cms.example.com");
        assert_eq!(result, "https://cdn.example.com/img.jpg");
    }

    #[test]
    fn resolve_url_relative_with_leading_slash() {
        let result = resolve_url("/uploads/photo.jpg", "https://cms.example.com");
        assert_eq!(result, "https://cms.example.com/uploads/photo.jpg");
    }

    #[test]
    fn resolve_url_relative_without_leading_slash() {
        let result = resolve_url("uploads/photo.jpg", "https://cms.example.com");
        assert_eq!(result, "https://cms.example.com/uploads/photo.jpg");
    }

    #[test]
    fn resolve_url_base_with_trailing_slash() {
        let result = resolve_url("/img.jpg", "https://cms.example.com/");
        assert_eq!(result, "https://cms.example.com/img.jpg");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib build::source_asset::tests::resolve_url -- --nocapture`

Expected: FAIL — `resolve_url` does not exist.

- [ ] **Step 3: Implement `resolve_url`**

Add to `src/build/source_asset.rs`:

```rust
/// Resolve a URL or path against a source's base URL.
///
/// - Absolute URLs (`https://...`) pass through unchanged.
/// - Relative paths (`/uploads/foo` or `uploads/foo`) are joined with the base.
pub fn resolve_url(url_or_path: &str, base_url: &str) -> String {
    if url_or_path.starts_with("http://") || url_or_path.starts_with("https://") {
        return url_or_path.to_string();
    }

    let base = base_url.trim_end_matches('/');
    if url_or_path.starts_with('/') {
        format!("{}{}", base, url_or_path)
    } else {
        format!("{}/{}", base, url_or_path)
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib build::source_asset::tests::resolve_url -- --nocapture`

Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/build/source_asset.rs
git commit -m "feat(build): add resolve_url helper for source-relative paths"
```

---

### Task 5: Thread `dev_mode` through template environment

Add `dev_mode` parameter to `register_functions` and `setup_environment`.

**Files:**
- Modify: `src/template/functions.rs:19-87`
- Modify: `src/template/environment.rs:34-100`

- [ ] **Step 1: Update `register_functions` signature**

In `src/template/functions.rs`, change the signature at line 19:

From:
```rust
pub fn register_functions(
    env: &mut Environment<'_>,
    config: &SiteConfig,
    manifest: Option<Arc<AssetManifest>>,
) {
```

To:
```rust
pub fn register_functions(
    env: &mut Environment<'_>,
    config: &SiteConfig,
    manifest: Option<Arc<AssetManifest>>,
    dev_mode: bool,
) {
```

- [ ] **Step 2: Update `setup_environment` signature**

In `src/template/environment.rs`, change the signature at line 34:

From:
```rust
pub fn setup_environment(
    project_root: &Path,
    config: &SiteConfig,
    pages: &[PageDef],
    plugin_registry: Option<&PluginRegistry>,
    manifest: Option<Arc<AssetManifest>>,
) -> Result<Environment<'static>> {
```

To:
```rust
pub fn setup_environment(
    project_root: &Path,
    config: &SiteConfig,
    pages: &[PageDef],
    plugin_registry: Option<&PluginRegistry>,
    manifest: Option<Arc<AssetManifest>>,
    dev_mode: bool,
) -> Result<Environment<'static>> {
```

- [ ] **Step 3: Pass `dev_mode` to `register_functions` in `setup_environment`**

In `src/template/environment.rs`, change line 93:

From:
```rust
    functions::register_functions(&mut env, config, manifest);
```

To:
```rust
    functions::register_functions(&mut env, config, manifest, dev_mode);
```

- [ ] **Step 4: Update the call in `render.rs` (build path)**

In `src/build/render.rs`, change the `setup_environment` call at line 116:

From:
```rust
    let env = template::setup_environment(
        project_root,
        &config,
        &pages,
        Some(&plugin_registry),
        if config.build.content_hash.enabled {
            Some(manifest.clone())
        } else {
            None
        },
    )?;
```

To:
```rust
    let env = template::setup_environment(
        project_root,
        &config,
        &pages,
        Some(&plugin_registry),
        if config.build.content_hash.enabled {
            Some(manifest.clone())
        } else {
            None
        },
        false, // not dev mode
    )?;
```

- [ ] **Step 5: Update the call in `rebuild.rs` (dev path)**

In `src/dev/rebuild.rs`, change the `setup_environment` call at line 195:

From:
```rust
        let env = template::setup_environment(
            project_root,
            config,
            &pages,
            Some(&self.plugin_registry),
            None, // No content hashing in dev mode.
        )?;
```

To:
```rust
        let env = template::setup_environment(
            project_root,
            config,
            &pages,
            Some(&self.plugin_registry),
            None, // No content hashing in dev mode.
            true, // dev mode
        )?;
```

- [ ] **Step 6: Fix all test calls to `register_functions`**

In `src/template/functions.rs` tests, update every call from:
```rust
register_functions(&mut env, &config, None);
```
To:
```rust
register_functions(&mut env, &config, None, false);
```

And calls with manifests from:
```rust
register_functions(&mut env, &config, Some(manifest));
```
To:
```rust
register_functions(&mut env, &config, Some(manifest), false);
```

- [ ] **Step 7: Fix all test calls to `setup_environment`**

In `src/template/environment.rs` tests, update every call from:
```rust
setup_environment(root, &config, &pages, None, None).unwrap();
```
To:
```rust
setup_environment(root, &config, &pages, None, None, false).unwrap();
```

And with registry:
```rust
setup_environment(root, &config, &pages, Some(&registry), None).unwrap();
```
To:
```rust
setup_environment(root, &config, &pages, Some(&registry), None, false).unwrap();
```

- [ ] **Step 8: Run the full test suite**

Run: `cargo test`

Expected: All existing tests pass. No functional changes yet.

- [ ] **Step 9: Commit**

```bash
git add src/template/functions.rs src/template/environment.rs src/build/render.rs src/dev/rebuild.rs
git commit -m "refactor: thread dev_mode through template environment and register_functions"
```

---

### Task 6: Register `source_asset` template function

Wire up the template function that collects requests (build) or returns proxy URLs (dev).

**Files:**
- Modify: `src/template/functions.rs:19-87`
- Modify: `src/template/environment.rs:34-100`

- [ ] **Step 1: Write tests for the template function**

Add to `src/template/functions.rs` tests module:

```rust
    #[test]
    fn source_asset_dev_mode_returns_proxy_url_relative_path() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true);
        env.add_template("test", r#"{{ source_asset("my_cms", "/uploads/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "/_proxy/my_cms/uploads/photo.jpg");
    }

    #[test]
    fn source_asset_dev_mode_returns_proxy_url_absolute_same_host() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true);
        env.add_template("test", r#"{{ source_asset("my_cms", "https://cms.example.com/uploads/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "/_proxy/my_cms/uploads/photo.jpg");
    }

    #[test]
    fn source_asset_dev_mode_returns_full_proxy_url_different_host() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true);
        env.add_template("test", r#"{{ source_asset("my_cms", "https://media.example.com/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "/_proxy/my_cms/__source_asset__/https://media.example.com/photo.jpg");
    }

    #[test]
    fn source_asset_build_mode_returns_original_url() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, false);
        env.add_template("test", r#"{{ source_asset("my_cms", "/uploads/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        // In build mode, returns the resolved absolute URL (placeholder for post-render rewrite).
        assert_eq!(result, "https://cms.example.com/uploads/photo.jpg");
    }

    #[test]
    fn source_asset_unknown_source_errors() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true);
        env.add_template("test", r#"{{ source_asset("nope", "/img.jpg") }}"#).unwrap();
        let err = env.get_template("test").unwrap().render(()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nope"), "Error should name the bad source: {}", msg);
    }
```

Also add the helper function in the tests module:

```rust
    fn make_config_with_source(name: &str, url: &str, auth: &str) -> SiteConfig {
        let mut config = SiteConfig::default();
        config.sources.insert(name.to_string(), crate::config::SourceConfig {
            url: url.to_string(),
            headers: std::collections::HashMap::from([
                ("Authorization".to_string(), auth.to_string()),
            ]),
        });
        config
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib template::functions::tests::source_asset -- --nocapture`

Expected: FAIL — `source_asset` function not registered.

- [ ] **Step 3: Implement the `source_asset` registration**

In `src/template/functions.rs`, add to `register_functions` (before the `site` global at line 82), and add the necessary imports:

Add to imports:
```rust
use crate::build::source_asset::{SourceAssetCollector, resolve_url};
```

Add to function body:
```rust
    // source_asset(source_name, url_or_path)
    let sources = config.sources.clone();
    let collector = SourceAssetCollector::new();
    env.add_function(
        "source_asset",
        move |source_name: &str, url_or_path: &str| -> Result<String, minijinja::Error> {
            if url_or_path.is_empty() {
                return Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "source_asset: url_or_path must be a non-empty string",
                ));
            }

            let source = sources.get(source_name).ok_or_else(|| {
                let available = sources.keys().cloned().collect::<Vec<_>>().join(", ");
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!(
                        "Unknown source '{}'. Available: {}",
                        source_name, available,
                    ),
                )
            })?;

            let resolved = resolve_url(url_or_path, &source.url);

            if dev_mode {
                // Dev mode: return a proxy URL.
                Ok(build_proxy_url(source_name, &resolved, &source.url))
            } else {
                // Build mode: record request, return resolved URL as placeholder.
                collector.push(source_name.to_string(), resolved.clone());
                Ok(resolved)
            }
        },
    );
```

Also add the `build_proxy_url` helper (private, outside `register_functions`):

```rust
/// Build a dev proxy URL for a source asset.
///
/// - Same-host URLs: extract the path and use `/_proxy/{source}/path`.
/// - Cross-host URLs: use `/_proxy/{source}/__source_asset__/{full_url}`.
fn build_proxy_url(source_name: &str, resolved_url: &str, source_base_url: &str) -> String {
    let base_host = extract_host(source_base_url);
    let url_host = extract_host(resolved_url);

    if base_host == url_host {
        // Same host — extract path portion.
        let path = resolved_url
            .find("://")
            .and_then(|i| resolved_url[i + 3..].find('/'))
            .map(|i| {
                let scheme_end = resolved_url.find("://").unwrap() + 3;
                &resolved_url[scheme_end + i..]
            })
            .unwrap_or("/");
        let path = path.trim_start_matches('/');
        format!("/_proxy/{}/{}", source_name, path)
    } else {
        // Different host — pass full URL through sentinel.
        format!("/_proxy/{}/__source_asset__/{}", source_name, resolved_url)
    }
}

/// Extract hostname from a URL (without port).
fn extract_host(url: &str) -> &str {
    url.find("://")
        .map(|i| &url[i + 3..])
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib template::functions::tests::source_asset -- --nocapture`

Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/template/functions.rs
git commit -m "feat(template): register source_asset function with dev/build mode handling"
```

---

### Task 7: Expose collector from setup_environment

The collector must be created outside the template engine and shared, so the caller can drain it after rendering.

**Files:**
- Modify: `src/template/functions.rs:19-87`
- Modify: `src/template/environment.rs:34-100`

- [ ] **Step 1: Update `register_functions` to accept an optional collector**

Change the signature:

From:
```rust
pub fn register_functions(
    env: &mut Environment<'_>,
    config: &SiteConfig,
    manifest: Option<Arc<AssetManifest>>,
    dev_mode: bool,
) {
```

To:
```rust
pub fn register_functions(
    env: &mut Environment<'_>,
    config: &SiteConfig,
    manifest: Option<Arc<AssetManifest>>,
    dev_mode: bool,
    source_asset_collector: Option<SourceAssetCollector>,
) {
```

Update the `source_asset` registration to use the passed-in collector:

```rust
    // source_asset(source_name, url_or_path)
    if !config.sources.is_empty() {
        let sources = config.sources.clone();
        let collector = source_asset_collector.unwrap_or_default();
        env.add_function(
            "source_asset",
            // ... same closure as before ...
        );
    }
```

- [ ] **Step 2: Update `setup_environment` to accept and pass through collector**

Change the signature:

```rust
pub fn setup_environment(
    project_root: &Path,
    config: &SiteConfig,
    pages: &[PageDef],
    plugin_registry: Option<&PluginRegistry>,
    manifest: Option<Arc<AssetManifest>>,
    dev_mode: bool,
    source_asset_collector: Option<SourceAssetCollector>,
) -> Result<Environment<'static>> {
```

Update line 93:
```rust
    functions::register_functions(&mut env, config, manifest, dev_mode, source_asset_collector);
```

- [ ] **Step 3: Update all callers**

In `src/build/render.rs` (line 116), add `None` for now (we'll wire it up in Task 8):
```rust
    let env = template::setup_environment(
        project_root,
        &config,
        &pages,
        Some(&plugin_registry),
        if config.build.content_hash.enabled {
            Some(manifest.clone())
        } else {
            None
        },
        false,
        None, // collector wired in Task 8
    )?;
```

In `src/dev/rebuild.rs` (line 195):
```rust
        let env = template::setup_environment(
            project_root,
            config,
            &pages,
            Some(&self.plugin_registry),
            None,
            true,
            None, // dev mode doesn't need collector
        )?;
```

- [ ] **Step 4: Update all test calls**

In `src/template/functions.rs` tests, update every call:
```rust
register_functions(&mut env, &config, None, false, None);
```
And for dev mode tests:
```rust
register_functions(&mut env, &config, None, true, None);
```

In `src/template/environment.rs` tests, update every call:
```rust
setup_environment(root, &config, &pages, None, None, false, None).unwrap();
```

- [ ] **Step 5: Run full test suite**

Run: `cargo test`

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/template/functions.rs src/template/environment.rs src/build/render.rs src/dev/rebuild.rs
git commit -m "refactor: thread SourceAssetCollector through template environment setup"
```

---

### Task 8: Wire collector into build pipeline

Create the collector in render.rs, pass it to the environment, drain after rendering, and call `resolve_source_assets`.

**Files:**
- Modify: `src/build/render.rs:50-250` (build function and render_static_page)

- [ ] **Step 1: Create collector and pass to environment**

In `src/build/render.rs`, add import:
```rust
use crate::build::source_asset::{self, SourceAssetCollector};
```

In the `build` function, before the `setup_environment` call, create the collector:
```rust
    // Source asset collector for authenticated image downloads.
    let source_asset_collector = SourceAssetCollector::new();
```

Update `setup_environment` call:
```rust
    let env = template::setup_environment(
        project_root,
        &config,
        &pages,
        Some(&plugin_registry),
        if config.build.content_hash.enabled {
            Some(manifest.clone())
        } else {
            None
        },
        false,
        Some(source_asset_collector.clone()),
    )?;
```

- [ ] **Step 2: Add `resolve_source_assets` call after `localize_assets` in `render_static_page`**

In `render_static_page` (around line 444-450), the `source_asset_collector` must be passed as a parameter. Add it to the function signature:

```rust
fn render_static_page(
    page: &PageDef,
    env: &minijinja::Environment<'_>,
    fetcher: &mut DataFetcher,
    global_data: &HashMap<String, serde_json::Value>,
    config: &SiteConfig,
    dist_dir: &Path,
    build_time: &str,
    output_paths: &mut HashSet<String>,
    data_query_count: &mut u32,
    asset_cache: &mut AssetCache,
    asset_client: &reqwest::blocking::Client,
    plugin_registry: &PluginRegistry,
    image_cache: &ImageCache,
    css_cache: &mut critical_css::StylesheetCache,
    manifest: &std::sync::Arc<content_hash::AssetManifest>,
    source_asset_collector: &SourceAssetCollector,
) -> Result<RenderedPage> {
```

After the `localize_assets` call (line 444-450), add:

```rust
    // Resolve authenticated source assets.
    let source_requests = source_asset_collector.drain();
    let full_html = if !source_requests.is_empty() {
        source_asset::resolve_source_assets(
            &full_html,
            &source_requests,
            &config.sources,
            asset_cache,
            asset_client,
            dist_dir,
        ).wrap_err_with(|| format!("Failed to resolve source assets for '{}'", tmpl_name))?
    } else {
        full_html
    };
```

- [ ] **Step 3: Do the same for `render_dynamic_page`**

Add `source_asset_collector: &SourceAssetCollector` to `render_dynamic_page` signature and add the same `resolve_source_assets` call after `localize_assets` inside the per-item render loop.

- [ ] **Step 4: Update all call sites in `build()` to pass the collector**

In the `build` function's `for page in &pages` loop, pass `&source_asset_collector` to both `render_static_page` and `render_dynamic_page`.

- [ ] **Step 5: Run full test suite**

Run: `cargo test`

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/build/render.rs
git commit -m "feat(build): wire source_asset collector into build pipeline"
```

---

### Task 9: Dev proxy `__source_asset__/` support

Extend the dev proxy to handle cross-host authenticated asset requests.

**Files:**
- Modify: `src/dev/proxy.rs:30-108`

- [ ] **Step 1: Write the test**

The proxy is async (axum), so we test it via integration. Add a unit test for the URL extraction logic:

Add a `#[cfg(test)]` module at the end of `src/dev/proxy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_source_asset_url() {
        let rest = "__source_asset__/https://media.example.com/img/photo.jpg";
        assert_eq!(
            parse_proxy_rest(rest),
            ProxyTarget::FullUrl("https://media.example.com/img/photo.jpg".to_string()),
        );
    }

    #[test]
    fn normal_path_passthrough() {
        let rest = "api/items/1";
        assert_eq!(
            parse_proxy_rest(rest),
            ProxyTarget::RelativePath("api/items/1".to_string()),
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib dev::proxy::tests -- --nocapture`

Expected: FAIL — `parse_proxy_rest` and `ProxyTarget` do not exist.

- [ ] **Step 3: Implement `ProxyTarget` and `parse_proxy_rest`**

Add to `src/dev/proxy.rs`:

```rust
/// Parsed proxy target from the URL path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProxyTarget {
    /// Normal relative path — append to source base URL.
    RelativePath(String),
    /// Full URL from `__source_asset__/` prefix — use directly.
    FullUrl(String),
}

const SOURCE_ASSET_PREFIX: &str = "__source_asset__/";

/// Parse the `rest` path segment from `/_proxy/{source}/*rest`.
fn parse_proxy_rest(rest: &str) -> ProxyTarget {
    if let Some(full_url) = rest.strip_prefix(SOURCE_ASSET_PREFIX) {
        ProxyTarget::FullUrl(full_url.to_string())
    } else {
        ProxyTarget::RelativePath(rest.to_string())
    }
}
```

- [ ] **Step 4: Update `proxy_handler` to use `parse_proxy_rest`**

Replace the URL construction logic in `proxy_handler`:

From:
```rust
    let base = state.source.url.trim_end_matches('/');
    let rest_path = if rest.starts_with('/') {
        rest.clone()
    } else {
        format!("/{}", rest)
    };
    let url = format!("{}{}", base, rest_path);
```

To:
```rust
    let url = match parse_proxy_rest(&rest) {
        ProxyTarget::FullUrl(full) => full,
        ProxyTarget::RelativePath(path) => {
            let base = state.source.url.trim_end_matches('/');
            let path = if path.starts_with('/') {
                path
            } else {
                format!("/{}", path)
            };
            format!("{}{}", base, path)
        }
    };
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib dev::proxy::tests -- --nocapture`

Expected: All PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`

Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/dev/proxy.rs
git commit -m "feat(dev): support __source_asset__/ prefix in proxy for cross-host auth"
```

---

### Task 10: Integration test

End-to-end test with a mock auth server.

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Write the integration test**

Add to `tests/integration_test.rs`:

```rust
#[test]
fn source_asset_downloads_with_auth_headers() {
    // Start a mock server that requires an auth header.
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_ip().unwrap();
    let base_url = format!("http://{}", addr);

    let handle = std::thread::spawn(move || {
        let req = server.recv().unwrap();
        // Verify the Authorization header is present.
        let auth = req.headers().iter()
            .find(|h| h.field.as_str().eq_ignore_ascii_case("authorization"))
            .map(|h| h.value.to_string());
        assert_eq!(auth.as_deref(), Some("Bearer test-token-123"));

        let png_bytes = b"\x89PNG\r\n\x1a\n"; // minimal PNG signature
        let response = tiny_http::Response::from_data(png_bytes.to_vec())
            .with_header(
                tiny_http::Header::from_bytes("content-type", "image/png").unwrap(),
            );
        req.respond(response).unwrap();
    });

    // Create a project with source_asset in the template.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // site.toml
    let config = format!(
        r#"[site]
name = "Source Asset Test"
base_url = "https://example.com"

[build]
fragments = false

[sources.test_cms]
url = "{}"
headers = {{ Authorization = "Bearer test-token-123" }}
"#,
        base_url,
    );
    std::fs::write(root.join("site.toml"), config).unwrap();

    // Template
    std::fs::create_dir_all(root.join("templates")).unwrap();
    let template = format!(
        r#"<!DOCTYPE html>
<html><body>
<img src='{{{{ source_asset("test_cms", "/uploads/hero.png") }}}}'>
</body></html>"#,
    );
    std::fs::write(root.join("templates/index.html"), template).unwrap();

    // Static dir (required).
    std::fs::create_dir_all(root.join("static")).unwrap();

    // Build.
    eigen::build::build(root, false, true).unwrap();

    handle.join().unwrap();

    // Verify the image was downloaded to dist/assets/.
    let dist_assets = root.join("dist/assets");
    assert!(dist_assets.exists(), "dist/assets/ should exist");
    let files: Vec<_> = std::fs::read_dir(&dist_assets)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!files.is_empty(), "Should have at least one downloaded asset");

    // Verify the HTML was rewritten.
    let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(
        html.contains("/assets/"),
        "HTML should reference local /assets/ path, got: {}",
        html,
    );
    assert!(
        !html.contains(&base_url),
        "HTML should not contain original URL, got: {}",
        html,
    );
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test integration_test source_asset_downloads_with_auth_headers -- --nocapture`

Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration test for source_asset authenticated downloads"
```

---

### Task 11: User-facing documentation

**Files:**
- Create: `docs/source_asset.md`

- [ ] **Step 1: Write the documentation**

Create `docs/source_asset.md`:

```markdown
# Authenticated Source Assets

Download images and other assets from authenticated data sources using the
`source_asset()` template function.

## Problem

Data sources (CMSes, APIs) often return JSON containing image URLs that require
the same authentication as the API itself. Standard `<img>` tags can't send
auth headers, and eigen's asset localization doesn't know which source an image
belongs to.

## Usage

```jinja
{# Relative path — resolved against the source's base URL #}
<img src="{{ source_asset('my_cms', '/uploads/' ~ item.image.hash) }}">

{# Absolute URL from data #}
<img src="{{ source_asset('my_cms', item.image_url) }}">
```

### Arguments

| Argument | Type | Description |
|---|---|---|
| `source_name` | string | Must match a `[sources.*]` key in `site.toml` |
| `url_or_path` | string | Absolute URL or path relative to source base URL |

### URL Resolution

- Starts with `http://` or `https://` → used as-is (absolute)
- Otherwise → joined with the source's configured `url`

### Return Value

- **Build time:** Downloads the asset with the source's auth headers, saves to
  `dist/assets/`, and returns the local `/assets/...` path.
- **Dev time:** Returns a `/_proxy/{source_name}/...` URL. The dev server's
  proxy forwards the request with auth headers — no download needed.

## Configuration

No new configuration. Uses your existing `[sources.*]` setup:

```toml
[sources.my_cms]
url = "https://cms.example.com"
headers = { Authorization = "Bearer ${CMS_TOKEN}" }
```

## Error Handling

| Condition | Behavior |
|---|---|
| Unknown source name | Template render error listing available sources |
| Empty URL | Template render error |
| Download fails (build) | Warning logged, original URL left in place |
| Proxy fails (dev) | 502 Bad Gateway from dev proxy |

## Examples

### Strapi CMS with image hashes

```toml
# site.toml
[sources.strapi]
url = "https://strapi.mysite.com"
headers = { Authorization = "Bearer ${STRAPI_TOKEN}" }
```

```jinja
{# In your template #}
{% for post in posts %}
  <article>
    <img src="{{ source_asset('strapi', '/uploads/' ~ post.cover.hash ~ post.cover.ext) }}">
    <h2>{{ post.title }}</h2>
  </article>
{% endfor %}
```

### API with images on a different CDN

When images are hosted on a different domain but use the same auth:

```jinja
{# The API returns full URLs like https://media.example.com/photo.jpg #}
<img src="{{ source_asset('my_api', item.photo_url) }}">
```

In dev mode this proxies through `/_proxy/my_api/__source_asset__/https://media.example.com/photo.jpg`,
forwarding your API's auth headers. At build time, the image is downloaded with
auth and saved locally.
```

- [ ] **Step 2: Commit**

```bash
git add docs/source_asset.md
git commit -m "docs: add source_asset authenticated asset downloads documentation"
```

---

### Task 12: Run /simplify

Per project workflow, run the simplify skill on the new code before final commit.

- [ ] **Step 1: Run `/simplify`**

Review all new and modified files for code quality, duplication, and readability.

- [ ] **Step 2: Fix any issues found**

- [ ] **Step 3: Run full test suite to confirm nothing broke**

Run: `cargo test`

Expected: All pass.

- [ ] **Step 4: Commit any simplification changes**

```bash
git add -A
git commit -m "refactor: simplify source_asset implementation"
```
