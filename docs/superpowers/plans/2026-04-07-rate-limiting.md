# Rate Limiting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add configurable per-second rate limiting for build-time HTTP requests to avoid overwhelming external APIs.

**Architecture:** A `RateLimiterPool` manages per-host `governor::RateLimiter` instances. Rate limits are configured globally in `[build]` and overridden per source in `[sources.*]`. Before each outbound HTTP request, the pool is consulted — if a limiter exists for the target host, it blocks until a token is available.

**Tech Stack:** `governor` crate (token bucket rate limiter), `url` crate (host extraction from URLs)

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/build/rate_limit.rs` | `RateLimiterPool` struct and logic |
| Modify | `src/build/mod.rs` | Add `pub mod rate_limit;` |
| Modify | `Cargo.toml` | Add `governor` and `url` dependencies |
| Modify | `src/config/mod.rs:54-128` | Add `rate_limit` field to `BuildConfig` |
| Modify | `src/config/mod.rs:694-698` | Add `rate_limit` field to `SourceConfig` |
| Modify | `src/data/fetcher.rs:16-46` | Accept `&RateLimiterPool`, call `wait()` before requests |
| Modify | `src/assets/rewrite.rs:46-53` | Accept `&RateLimiterPool`, call `wait()` before downloads |
| Modify | `src/assets/download.rs:28-53` | Accept `&RateLimiterPool`, call `wait()` before HTTP send |
| Modify | `src/build/source_asset.rs:83-90` | Accept `&RateLimiterPool`, call `wait()` before downloads |
| Modify | `src/build/render.rs:134-142` | Create `RateLimiterPool`, pass through to all callers |
| Create | `docs/rate_limiting.md` | Feature documentation |

---

### Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add `governor` to `[dependencies]`**

In `Cargo.toml`, add under `[dependencies]`:

```toml
governor = "0.8"
url = "2"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add governor and url dependencies for rate limiting"
```

---

### Task 2: Add `rate_limit` fields to config structs

**Files:**
- Modify: `src/config/mod.rs:54-128` (BuildConfig)
- Modify: `src/config/mod.rs:694-698` (SourceConfig)

- [ ] **Step 1: Write the failing test**

Add at the bottom of the existing `#[cfg(test)] mod tests` block in `src/config/mod.rs`:

```rust
#[test]
fn parse_rate_limit_global_and_per_source() {
    let toml_str = r#"
[site]
name = "test"
base_url = "https://example.com"

[build]
rate_limit = 10

[sources.api]
url = "https://api.example.com"
rate_limit = 5

[sources.cdn]
url = "https://cdn.example.com"
"#;
    let config: SiteConfig = toml::from_str(toml_str).expect("parse failed");
    assert_eq!(config.build.rate_limit, Some(10));
    assert_eq!(config.sources["api"].rate_limit, Some(5));
    assert_eq!(config.sources["cdn"].rate_limit, None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test parse_rate_limit_global_and_per_source -- --nocapture`
Expected: FAIL — `rate_limit` field does not exist on `BuildConfig` or `SourceConfig`.

- [ ] **Step 3: Add `rate_limit` field to `BuildConfig`**

In `src/config/mod.rs`, add to `BuildConfig` struct (after the `not_found` field at line ~107):

```rust
    /// Optional global rate limit for outbound HTTP requests (requests per second).
    /// When set, all build-time HTTP requests are throttled to this rate per host.
    #[serde(default)]
    pub rate_limit: Option<u32>,
```

And add to the `Default` impl for `BuildConfig` (after `not_found: true`):

```rust
            rate_limit: None,
```

- [ ] **Step 4: Add `rate_limit` field to `SourceConfig`**

In `src/config/mod.rs`, add to `SourceConfig` struct (after `headers` field at line ~697):

```rust
    /// Optional per-source rate limit (requests per second).
    /// Overrides the global `[build] rate_limit` for this source's host.
    #[serde(default)]
    pub rate_limit: Option<u32>,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test parse_rate_limit_global_and_per_source -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat: add rate_limit config fields to BuildConfig and SourceConfig"
```

---

### Task 3: Create `RateLimiterPool`

**Files:**
- Create: `src/build/rate_limit.rs`
- Modify: `src/build/mod.rs`

- [ ] **Step 1: Write the tests first**

Create `src/build/rate_limit.rs` with tests at the bottom:

```rust
//! Build-time HTTP rate limiting.
//!
//! `RateLimiterPool` manages per-host token-bucket rate limiters using the
//! `governor` crate.  Before each outbound request, call `pool.wait(url)` —
//! it blocks until a token is available for the target host.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::config::SourceConfig;

    fn source(url: &str, rate_limit: Option<u32>) -> SourceConfig {
        SourceConfig {
            url: url.to_string(),
            headers: HashMap::new(),
            rate_limit,
        }
    }

    #[test]
    fn no_config_means_no_wait() {
        let pool = RateLimiterPool::new(None, &HashMap::new());
        // Should return immediately — no limiter configured.
        pool.wait("https://api.example.com/data");
        pool.wait("https://cdn.example.com/image.png");
    }

    #[test]
    fn global_rate_limit_applies() {
        let pool = RateLimiterPool::new(Some(100), &HashMap::new());
        let start = std::time::Instant::now();
        // First request should be instant (token available).
        pool.wait("https://api.example.com/data");
        assert!(start.elapsed() < std::time::Duration::from_millis(50));
    }

    #[test]
    fn per_source_overrides_global() {
        let mut sources = HashMap::new();
        sources.insert("api".to_string(), source("https://api.example.com", Some(2)));
        let pool = RateLimiterPool::new(Some(100), &sources);

        // First request is free.
        pool.wait("https://api.example.com/data");
        let start = std::time::Instant::now();
        // Second request should be throttled at ~2/s = ~500ms wait.
        pool.wait("https://api.example.com/other");
        let elapsed = start.elapsed();
        assert!(
            elapsed >= std::time::Duration::from_millis(400),
            "Expected >=400ms wait for 2 req/s, got {:?}",
            elapsed
        );
    }

    #[test]
    fn different_hosts_have_independent_limiters() {
        let mut sources = HashMap::new();
        sources.insert("slow".to_string(), source("https://slow.example.com", Some(1)));
        let pool = RateLimiterPool::new(Some(100), &sources);

        pool.wait("https://slow.example.com/a");
        // A different host should not be affected by slow.example.com's limiter.
        let start = std::time::Instant::now();
        pool.wait("https://fast.example.com/b");
        assert!(start.elapsed() < std::time::Duration::from_millis(50));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib rate_limit -- --nocapture`
Expected: FAIL — `RateLimiterPool` not defined.

- [ ] **Step 3: Implement `RateLimiterPool`**

Add the implementation above the `#[cfg(test)]` block in `src/build/rate_limit.rs`:

```rust
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Mutex;

use governor::{Quota, RateLimiter as GovRateLimiter, clock::{Clock, DefaultClock}};
use url::Url;

use crate::config::SourceConfig;

/// A direct (non-keyed) governor rate limiter using default clock and state.
type Limiter = GovRateLimiter<
    governor::state::NotKeyed,
    governor::state::InMemoryState,
    DefaultClock,
>;

/// Manages per-host rate limiters for build-time HTTP requests.
///
/// Created once at build start, shared by reference across all request sites.
pub struct RateLimiterPool {
    /// Per-host limiters, created lazily on first request.
    limiters: Mutex<HashMap<String, Limiter>>,
    /// Clock used for rate limit timing.
    clock: DefaultClock,
    /// Global default rate limit (requests per second). `None` means no limit.
    global_rate: Option<u32>,
    /// Source host → per-source rate limit override.
    source_rates: HashMap<String, u32>,
}

impl RateLimiterPool {
    /// Create a new pool from the global rate limit and source configs.
    pub fn new(
        global_rate: Option<u32>,
        sources: &HashMap<String, SourceConfig>,
    ) -> Self {
        let mut source_rates = HashMap::new();
        for source in sources.values() {
            if let Some(rate) = source.rate_limit {
                if let Some(host) = extract_host(&source.url) {
                    source_rates.insert(host, rate);
                }
            }
        }

        Self {
            limiters: Mutex::new(HashMap::new()),
            clock: DefaultClock::default(),
            global_rate,
            source_rates,
        }
    }

    /// Block until a token is available for the given URL's host.
    ///
    /// If no rate limit is configured (neither global nor per-source) for the
    /// host, returns immediately.
    pub fn wait(&self, url: &str) {
        let host = match extract_host(url) {
            Some(h) => h,
            None => return,
        };

        // Determine the rate for this host.
        let rate = self.source_rates.get(&host).copied()
            .or(self.global_rate);

        let rate = match rate {
            Some(r) if r > 0 => r,
            _ => return,
        };

        let mut limiters = self.limiters.lock().expect("rate limiter lock poisoned");
        let limiter = limiters.entry(host).or_insert_with(|| {
            let quota = Quota::per_second(NonZeroU32::new(rate).expect("rate is non-zero"));
            GovRateLimiter::direct(quota)
        });

        // Spin on check() — sleep for the indicated duration when denied.
        loop {
            match limiter.check() {
                Ok(_) => break,
                Err(not_until) => {
                    let wait = not_until.wait_time_from(self.clock.now());
                    std::thread::sleep(wait);
                }
            }
        }
    }
}

/// Extract the host from a URL string.
fn extract_host(url: &str) -> Option<String> {
    Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.to_string()))
}
```

- [ ] **Step 4: Add the module declaration**

In `src/build/mod.rs`, add:

```rust
pub mod rate_limit;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib rate_limit -- --nocapture`
Expected: All 4 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/build/rate_limit.rs src/build/mod.rs
git commit -m "feat: add RateLimiterPool with per-host token bucket rate limiting"
```

---

### Task 4: Thread `RateLimiterPool` into data fetcher

**Files:**
- Modify: `src/data/fetcher.rs:16-46` (DataFetcher struct and `new()`)
- Modify: `src/data/fetcher.rs:148-166` (send_request)
- Modify: `src/build/render.rs:134-137` (DataFetcher construction)

- [ ] **Step 1: Add `RateLimiterPool` to `DataFetcher`**

In `src/data/fetcher.rs`, add the import at the top:

```rust
use crate::build::rate_limit::RateLimiterPool;
```

Add a field to the `DataFetcher` struct (after `data_cache`):

```rust
    /// Rate limiter for outbound HTTP requests.
    rate_limiter: &'a RateLimiterPool,
```

This requires adding a lifetime to the struct. Change the struct definition from:

```rust
pub struct DataFetcher {
```

to:

```rust
pub struct DataFetcher<'a> {
```

- [ ] **Step 2: Update `DataFetcher::new()` to accept the pool**

Change the `new()` signature to:

```rust
    pub fn new(
        sources: &HashMap<String, SourceConfig>,
        project_root: &Path,
        data_cache: Option<super::cache::DataCache>,
        rate_limiter: &'a RateLimiterPool,
    ) -> Self {
```

And add `rate_limiter` to the `Self` constructor:

```rust
            rate_limiter,
```

- [ ] **Step 3: Call `wait()` before HTTP requests in `send_request()`**

In `send_request()`, add before the match:

```rust
        self.rate_limiter.wait(url);
```

The full function becomes:

```rust
    fn send_request(
        &self,
        method: &HttpMethod,
        url: &str,
        headers: reqwest::header::HeaderMap,
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::blocking::Response> {
        self.rate_limiter.wait(url);
        let response = match method {
            HttpMethod::Get => self.client.get(url).headers(headers).send(),
            HttpMethod::Post => {
                let mut req = self.client.post(url).headers(headers);
                if let Some(b) = body {
                    req = req.json(b);
                }
                req.send()
            }
        }
        .wrap_err_with(|| format!("HTTP request failed for {}", url))?;
        Ok(response)
    }
```

- [ ] **Step 4: Update `DataFetcher::new()` call in `render.rs`**

In `src/build/render.rs`, at the `DataFetcher::new()` call site (~line 137), create the pool and pass it:

```rust
    // Rate limiter.
    let rate_limiter = RateLimiterPool::new(config.build.rate_limit, &config.sources);

    // Data fetcher.
    let data_cache = data::open_data_cache(project_root, fresh);
    let mut fetcher = DataFetcher::new(&config.sources, project_root, data_cache, &rate_limiter);
```

Add the import at the top of `render.rs`:

```rust
use crate::build::rate_limit::RateLimiterPool;
```

- [ ] **Step 5: Fix any lifetime propagation**

The lifetime on `DataFetcher<'a>` will propagate to any struct or function that holds it. Check all call sites reference `&fetcher` and that the `rate_limiter` outlives `fetcher`. Since both are stack locals in the same function in `render.rs`, this is satisfied automatically.

Run: `cargo check`
Expected: Compiles with no errors.

- [ ] **Step 6: Run existing tests**

Run: `cargo test`
Expected: All tests pass (existing behavior unchanged — no rate limit configured in test configs).

- [ ] **Step 7: Commit**

```bash
git add src/data/fetcher.rs src/build/render.rs
git commit -m "feat: thread RateLimiterPool into DataFetcher"
```

---

### Task 5: Thread `RateLimiterPool` into asset downloads

**Files:**
- Modify: `src/assets/download.rs:28-33` (download_asset_with_headers)
- Modify: `src/assets/rewrite.rs:46-53` (localize_assets)
- Modify: `src/build/source_asset.rs:83-90` (resolve_source_assets)
- Modify: `src/build/render.rs` (pass pool to asset functions)

- [ ] **Step 1: Add `pool` parameter to `download_asset_with_headers()`**

In `src/assets/download.rs`, add the import:

```rust
use crate::build::rate_limit::RateLimiterPool;
```

Change `download_asset_with_headers` signature to add a `pool` parameter:

```rust
pub fn download_asset_with_headers(
    client: &reqwest::blocking::Client,
    url: &str,
    cached_meta: Option<&AssetCacheMeta>,
    extra_headers: &reqwest::header::HeaderMap,
    pool: &RateLimiterPool,
) -> Result<DownloadResult> {
```

Add `pool.wait(url);` as the first line of the function body (before `let mut request = client.get(url);`).

- [ ] **Step 2: Update `download_asset()` to pass pool**

```rust
pub fn download_asset(
    client: &reqwest::blocking::Client,
    url: &str,
    cached_meta: Option<&AssetCacheMeta>,
    pool: &RateLimiterPool,
) -> Result<DownloadResult> {
    download_asset_with_headers(client, url, cached_meta, &reqwest::header::HeaderMap::new(), pool)
}
```

- [ ] **Step 3: Update `ensure_asset_with_headers()` to pass pool**

```rust
pub fn ensure_asset_with_headers(
    client: &reqwest::blocking::Client,
    cache: &mut AssetCache,
    url: &str,
    extra_headers: &reqwest::header::HeaderMap,
    pool: &RateLimiterPool,
) -> Result<String> {
```

Update both `download_asset_with_headers` calls inside this function to pass `pool`:

```rust
            match download_asset_with_headers(client, url, Some(meta), extra_headers, pool)? {
```

and:

```rust
    match download_asset_with_headers(client, url, None, extra_headers, pool)? {
```

- [ ] **Step 4: Update `ensure_asset()` to pass pool**

```rust
pub fn ensure_asset(
    client: &reqwest::blocking::Client,
    cache: &mut AssetCache,
    url: &str,
    pool: &RateLimiterPool,
) -> Result<String> {
    ensure_asset_with_headers(client, cache, url, &reqwest::header::HeaderMap::new(), pool)
}
```

- [ ] **Step 5: Update `localize_assets()` to accept and pass pool**

In `src/assets/rewrite.rs`, add import:

```rust
use crate::build::rate_limit::RateLimiterPool;
```

Change `localize_assets` signature to add `pool`:

```rust
pub fn localize_assets(
    html: &str,
    config: &AssetsConfig,
    cache: &mut AssetCache,
    client: &reqwest::blocking::Client,
    dist_dir: &Path,
    skip_urls: &HashSet<String>,
    pool: &RateLimiterPool,
) -> Result<String> {
```

Update the `ensure_asset` call inside (~line 98):

```rust
        match download::ensure_asset(client, cache, url, pool) {
```

- [ ] **Step 6: Update `resolve_source_assets()` to accept and pass pool**

In `src/build/source_asset.rs`, add import:

```rust
use crate::build::rate_limit::RateLimiterPool;
```

Change `resolve_source_assets` signature to add `pool`:

```rust
pub fn resolve_source_assets(
    html: &str,
    requests: &[SourceAssetRequest],
    sources: &HashMap<String, SourceConfig>,
    cache: &mut AssetCache,
    client: &reqwest::blocking::Client,
    dist_dir: &Path,
    pool: &RateLimiterPool,
) -> Result<String> {
```

Update the `ensure_asset_with_headers` call inside (~line 128):

```rust
        match download::ensure_asset_with_headers(client, cache, url, &headers, pool) {
```

- [ ] **Step 7: Update all call sites in `render.rs`**

Pass `&rate_limiter` to every `localize_assets` and `resolve_source_assets` call. There are three `localize_assets` calls and two `resolve_source_assets` calls. Add `&rate_limiter` as the last argument to each.

- [ ] **Step 8: Update tests in `download.rs` that call the modified functions**

In `src/assets/download.rs` tests, create a no-op pool and pass it:

```rust
use crate::build::rate_limit::RateLimiterPool;
use std::collections::HashMap;

let pool = RateLimiterPool::new(None, &HashMap::new());
```

Pass `&pool` as the last argument to `download_asset_with_headers`, `ensure_asset_with_headers`, etc. in every test.

- [ ] **Step 9: Compile and run all tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/assets/download.rs src/assets/rewrite.rs src/build/source_asset.rs src/build/render.rs
git commit -m "feat: thread RateLimiterPool into asset download pipeline"
```

---

### Task 6: Update dev rebuild path

**Files:**
- Modify: `src/dev/rebuild.rs` (if it calls render functions that now require `RateLimiterPool`)

- [ ] **Step 1: Check if `rebuild.rs` calls any modified functions**

Read `src/dev/rebuild.rs` and check if it calls `DataFetcher::new()` or the render entry point. If it does, thread `&rate_limiter` through the same way as in `render.rs`.

- [ ] **Step 2: Fix any compilation errors**

Run: `cargo check`

If `rebuild.rs` calls into the render pipeline, it already receives `SiteConfig` and can construct a `RateLimiterPool` the same way render does. Apply the same pattern.

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit (if changes were needed)**

```bash
git add src/dev/rebuild.rs
git commit -m "feat: thread RateLimiterPool into dev rebuild path"
```

---

### Task 7: Write feature documentation

**Files:**
- Create: `docs/rate_limiting.md`

- [ ] **Step 1: Write the docs**

```markdown
# Rate Limiting

Eigen can throttle outbound HTTP requests during build to avoid overwhelming external APIs. Rate limits are expressed as requests per second and use a token bucket algorithm for smooth throttling.

## Configuration

### Global rate limit

Set a default rate limit for all outbound HTTP requests in `[build]`:

\`\`\`toml
[build]
rate_limit = 10  # max 10 requests per second per host
\`\`\`

### Per-source rate limit

Override the global default for a specific source:

\`\`\`toml
[sources.strapi]
url = "https://strapi.example.com"
rate_limit = 5  # max 5 requests per second to this source
\`\`\`

### No rate limit (default)

If `rate_limit` is not set anywhere, requests are not throttled. This is the default behavior.

## Behavior

- Rate limits are applied **per host**. Requests to different hosts are throttled independently.
- Per-source `rate_limit` overrides the global default for that source's host.
- Asset localization downloads (images, media found in HTML) use the global rate limit.
- The token bucket algorithm allows brief bursts while maintaining the average rate.
- Rate limiting only affects build-time requests, not the dev server proxy.
```

- [ ] **Step 2: Commit**

```bash
git add docs/rate_limiting.md
git commit -m "docs: add rate limiting feature documentation"
```

---

### Task 8: Integration smoke test

**Files:**
- Modify: `example_site/site.toml` (temporarily, for manual testing only)

- [ ] **Step 1: Run a full build with no rate limit configured**

Run: `cargo run -- build example_site`
Expected: Build succeeds, same behavior as before.

- [ ] **Step 2: Run a full build with rate limit set**

Temporarily add `rate_limit = 5` to `[build]` in `example_site/site.toml`, run the build, and verify it completes (possibly slower). Then revert the change.

- [ ] **Step 3: Final test suite run**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit any remaining changes**

Only commit if integration tests were added. Otherwise, all code is already committed.
