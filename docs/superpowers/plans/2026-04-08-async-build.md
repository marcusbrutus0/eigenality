# Async Build Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert `build::build()` from synchronous to fully async, enabling concurrent page rendering, concurrent HTTP fetching, and native tokio integration with the dev server.

**Architecture:** Bottom-up conversion: async I/O layer first (DataFetcher, assets), then lift render pipeline with `BuildContext` struct and `buffer_unordered` concurrency, then simplify dev server. Shared mutable state uses `Arc<tokio::sync::Mutex<T>>`.

**Tech Stack:** Rust, tokio, reqwest (async), futures::stream, Arc/Mutex

---

## File Map

**Modified files:**
- `src/build/rate_limit.rs` — `wait()` becomes `async fn`
- `src/data/fetcher.rs` — `reqwest::blocking::Client` → `reqwest::Client`, all methods async
- `src/data/query.rs` — `resolve_page_data`, `resolve_dynamic_page_data`, `resolve_dynamic_page_data_for_item` become async
- `src/data/mod.rs` — re-exports unchanged
- `src/assets/download.rs` — all download functions become async
- `src/assets/rewrite.rs` — `localize_assets` becomes async
- `src/assets/mod.rs` — re-exports unchanged
- `src/build/source_asset.rs` — `resolve_source_assets` becomes async
- `src/build/render.rs` — introduce `BuildContext`, `build()` becomes async with `buffer_unordered`, render functions become async
- `src/build/feed.rs` — `generate_feeds` becomes async
- `src/build/mod.rs` — re-export unchanged
- `src/main.rs` — Build command wraps in tokio runtime
- `src/dev/server.rs` — drop `spawn_blocking` and thread bridge
- `src/dev/rebuild.rs` — `DevBuildState` methods become async, `reqwest::blocking::Client` → `reqwest::Client`
- `Cargo.toml` — remove `blocking` feature from reqwest (if no other consumers)
- `tests/integration_test.rs` — switch to `#[tokio::test]`

**New files:**
- None. All changes are modifications to existing files.

---

### Task 1: Convert `RateLimiterPool::wait()` to async

**Files:**
- Modify: `src/build/rate_limit.rs`

This is the leaf-most change. The rate limiter already uses `governor` (async-native) but bridges to sync with `futures::executor::block_on`. Convert `wait()` to `async fn` and remove the blocking bridge.

- [ ] **Step 1: Read the current `rate_limit.rs`**

Read `src/build/rate_limit.rs` in full to understand the current implementation.

- [ ] **Step 2: Convert `wait()` to `async fn`**

Change:
```rust
pub fn wait(&self, url: &str) {
    // ... host extraction ...
    futures::executor::block_on(limiter.until_ready());
}
```

To:
```rust
pub async fn wait(&self, url: &str) {
    // ... host extraction (same) ...
    limiter.until_ready().await;
}
```

The host extraction and limiter lookup logic stays identical. Only the blocking bridge changes.

- [ ] **Step 3: Verify the file compiles in isolation**

Run: `cargo check 2>&1 | head -30`

This will show errors in callers (DataFetcher, download.rs, etc.) — that's expected. The rate_limit.rs itself should have no errors. Note down the caller errors for the next tasks.

- [ ] **Step 4: Commit**

```bash
git add src/build/rate_limit.rs
git commit -m "refactor: convert RateLimiterPool::wait() to async"
```

---

### Task 2: Convert `DataFetcher` to async

**Files:**
- Modify: `src/data/fetcher.rs`

Convert `reqwest::blocking::Client` → `reqwest::Client` and make all methods async.

- [ ] **Step 1: Read `src/data/fetcher.rs` in full**

Read the complete file (lines 1-380, excluding tests) to understand every method.

- [ ] **Step 2: Update the struct and constructor**

Change the struct field:
```rust
client: reqwest::blocking::Client,
```
To:
```rust
client: reqwest::Client,
```

Change the constructor:
```rust
client: reqwest::blocking::Client::new(),
```
To:
```rust
client: reqwest::Client::new(),
```

- [ ] **Step 3: Convert `send_request()` to async**

Change signature from:
```rust
fn send_request(
    &self,
    method: &HttpMethod,
    url: &str,
    headers: reqwest::header::HeaderMap,
    body: Option<&serde_json::Value>,
) -> Result<reqwest::blocking::Response> {
```

To:
```rust
async fn send_request(
    &self,
    method: &HttpMethod,
    url: &str,
    headers: reqwest::header::HeaderMap,
    body: Option<&serde_json::Value>,
) -> Result<reqwest::Response> {
```

Inside the body:
- Change `self.rate_limiter.wait(url)` → `self.rate_limiter.wait(url).await`
- Change `.send()` → `.send().await` on both GET and POST branches
- Change return type references from `reqwest::blocking::Response` to `reqwest::Response`

- [ ] **Step 4: Convert `handle_success_response()` to async**

This method reads the response body. Change:
```rust
fn handle_success_response(
    &mut self,
    cache_key: &str,
    response: reqwest::blocking::Response,
) -> Result<Value> {
```

To:
```rust
async fn handle_success_response(
    &mut self,
    cache_key: &str,
    response: reqwest::Response,
) -> Result<Value> {
```

Inside: change `response.text()` → `response.text().await` and `response.headers()` access (headers are available before consuming body, so `.headers()` stays sync but `.text()` needs await).

- [ ] **Step 5: Convert `fetch_source()` to async**

Change signature:
```rust
fn fetch_source(
    &mut self,
    source_name: &str,
    path: &str,
    method: &HttpMethod,
    body: Option<&serde_json::Value>,
) -> Result<Value> {
```

To:
```rust
async fn fetch_source(
    &mut self,
    source_name: &str,
    path: &str,
    method: &HttpMethod,
    body: Option<&serde_json::Value>,
) -> Result<Value> {
```

Inside:
- `self.send_request(...)` → `self.send_request(...).await`
- `self.handle_success_response(...)` → `self.handle_success_response(...).await`
- `response.status()` and `response.headers()` stay sync (they don't consume the body)

- [ ] **Step 6: Convert `fetch()` to async**

Change signature:
```rust
pub fn fetch(
    &mut self,
    query: &DataQuery,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<Value> {
```

To:
```rust
pub async fn fetch(
    &mut self,
    query: &DataQuery,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<Value> {
```

Inside:
- `self.fetch_source(...)` → `self.fetch_source(...).await`
- `self.fetch_file(...)` stays sync (local filesystem, no benefit from async here)
- Everything else (plugin transforms, `apply_transforms`) stays sync

- [ ] **Step 7: Update DataFetcher tests to `#[tokio::test]`**

Read the test section of `src/data/fetcher.rs` (lines 381-926). For each test that calls `fetcher.fetch()` or `fetcher.send_request()`:
- Change `#[test]` → `#[tokio::test]`
- Add `.await` to the async method calls
- The test HTTP servers (`tiny_http`) stay sync — they run on their own threads

Example transformation:
```rust
#[test]
fn test_fetch_from_source() {
    // ... setup ...
    let result = fetcher.fetch(&query, None).unwrap();
    // ... assertions ...
}
```
Becomes:
```rust
#[tokio::test]
async fn test_fetch_from_source() {
    // ... setup (same) ...
    let result = fetcher.fetch(&query, None).await.unwrap();
    // ... assertions (same) ...
}
```

- [ ] **Step 8: Verify DataFetcher compiles and tests pass**

Run: `cargo test --lib data::fetcher 2>&1 | tail -20`

The fetcher tests should pass. Callers (query.rs, render.rs, etc.) will have errors — that's expected.

- [ ] **Step 9: Commit**

```bash
git add src/data/fetcher.rs
git commit -m "refactor: convert DataFetcher to async reqwest"
```

---

### Task 3: Convert data query layer to async

**Files:**
- Modify: `src/data/query.rs`

The query functions call `fetcher.fetch()` which is now async.

- [ ] **Step 1: Convert `resolve_page_data()` to async**

Change:
```rust
pub fn resolve_page_data(
    frontmatter: &Frontmatter,
    fetcher: &mut DataFetcher,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<HashMap<String, Value>> {
```

To:
```rust
pub async fn resolve_page_data(
    frontmatter: &Frontmatter,
    fetcher: &mut DataFetcher,
    plugin_registry: Option<&PluginRegistry>,
) -> Result<HashMap<String, Value>> {
```

Inside: `fetcher.fetch(query, plugin_registry)` → `fetcher.fetch(query, plugin_registry).await`

- [ ] **Step 2: Convert `resolve_dynamic_page_data()` to async**

Same pattern — add `async` to signature, `.await` on `fetcher.fetch()`.

- [ ] **Step 3: Convert `resolve_item_data()` and `resolve_dynamic_page_data_for_item()` to async**

Same pattern for both functions. Add `async`, `.await` on `fetcher.fetch()`.

- [ ] **Step 4: Update query tests to `#[tokio::test]`**

Read the test section of `src/data/query.rs`. Update tests that call the now-async functions.

- [ ] **Step 5: Verify query module compiles**

Run: `cargo check 2>&1 | grep "query.rs"` — should show no errors in query.rs itself.

- [ ] **Step 6: Commit**

```bash
git add src/data/query.rs
git commit -m "refactor: convert data query resolution to async"
```

---

### Task 4: Convert asset download layer to async

**Files:**
- Modify: `src/assets/download.rs`

- [ ] **Step 1: Read `src/assets/download.rs` in full**

- [ ] **Step 2: Convert `download_asset_with_headers()` to async**

Change signature:
```rust
pub fn download_asset_with_headers(
    client: &reqwest::blocking::Client,
    url: &str,
    cached_meta: Option<&AssetCacheMeta>,
    extra_headers: &reqwest::header::HeaderMap,
    pool: &RateLimiterPool,
) -> Result<DownloadResult> {
```

To:
```rust
pub async fn download_asset_with_headers(
    client: &reqwest::Client,
    url: &str,
    cached_meta: Option<&AssetCacheMeta>,
    extra_headers: &reqwest::header::HeaderMap,
    pool: &RateLimiterPool,
) -> Result<DownloadResult> {
```

Inside:
- `pool.wait(url)` → `pool.wait(url).await`
- `client.get(url).headers(headers).send()` → `client.get(url).headers(headers).send().await`
- `response.bytes()` → `response.bytes().await`
- `response.status()` and `response.headers()` stay sync

- [ ] **Step 3: Convert `download_asset()` to async**

Wrapper function — add `async`, `.await` on inner call.

- [ ] **Step 4: Convert `ensure_asset_with_headers()` to async**

Add `async`, `.await` on `download_asset_with_headers()`.

- [ ] **Step 5: Convert `ensure_asset()` to async**

Wrapper — add `async`, `.await`.

- [ ] **Step 6: Update download tests to `#[tokio::test]`**

Read tests in download.rs, add `.await` to async calls, change `reqwest::blocking::Client::new()` → `reqwest::Client::new()`.

- [ ] **Step 7: Commit**

```bash
git add src/assets/download.rs
git commit -m "refactor: convert asset downloads to async reqwest"
```

---

### Task 5: Convert asset rewriting to async

**Files:**
- Modify: `src/assets/rewrite.rs`

- [ ] **Step 1: Read `src/assets/rewrite.rs` in full**

- [ ] **Step 2: Convert `localize_assets()` to async**

Change signature:
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

To:
```rust
pub async fn localize_assets(
    html: &str,
    config: &AssetsConfig,
    cache: &mut AssetCache,
    client: &reqwest::Client,
    dist_dir: &Path,
    skip_urls: &HashSet<String>,
    pool: &RateLimiterPool,
) -> Result<String> {
```

Inside: `download::ensure_asset(client, cache, url, pool)` → `download::ensure_asset(client, cache, url, pool).await`

The `extract_remote_urls()` and `should_skip_cdn()` helper functions stay sync — they're pure string operations.

- [ ] **Step 3: Update rewrite tests to `#[tokio::test]`**

- [ ] **Step 4: Commit**

```bash
git add src/assets/rewrite.rs
git commit -m "refactor: convert asset rewriting to async"
```

---

### Task 6: Convert source asset resolution to async

**Files:**
- Modify: `src/build/source_asset.rs`

- [ ] **Step 1: Read `src/build/source_asset.rs`**

Read the `resolve_source_assets()` function.

- [ ] **Step 2: Convert `resolve_source_assets()` to async**

Change signature:
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

To:
```rust
pub async fn resolve_source_assets(
    html: &str,
    requests: &[SourceAssetRequest],
    sources: &HashMap<String, SourceConfig>,
    cache: &mut AssetCache,
    client: &reqwest::Client,
    dist_dir: &Path,
    pool: &RateLimiterPool,
) -> Result<String> {
```

Inside: `.await` on `download::ensure_asset_with_headers()` calls.

- [ ] **Step 3: Update source_asset tests to `#[tokio::test]`**

Change `reqwest::blocking::Client::new()` → `reqwest::Client::new()`, add `.await` on async calls.

- [ ] **Step 4: Commit**

```bash
git add src/build/source_asset.rs
git commit -m "refactor: convert source asset resolution to async"
```

---

### Task 7: Convert feed generation to async

**Files:**
- Modify: `src/build/feed.rs`

- [ ] **Step 1: Read `src/build/feed.rs` in full**

- [ ] **Step 2: Convert `generate_feed()` to async**

Add `async` to signature. Inside: `fetcher.fetch(&query, plugin_registry)` → `fetcher.fetch(&query, plugin_registry).await`

- [ ] **Step 3: Convert `generate_feeds()` to async**

Add `async` to signature. Inside: `generate_feed(...).await` for each feed. The loop stays sequential (feeds are few and fast).

- [ ] **Step 4: Update feed tests to `#[tokio::test]`**

- [ ] **Step 5: Commit**

```bash
git add src/build/feed.rs
git commit -m "refactor: convert feed generation to async"
```

---

### Task 8: Introduce `BuildContext` struct and convert `build()` to async

This is the largest task. It introduces the shared-state wrapper, converts the render functions, and makes `build()` async with concurrent page rendering.

**Files:**
- Modify: `src/build/render.rs`
- Modify: `src/build/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Read `src/build/render.rs` lines 1-50 (imports and struct)**

- [ ] **Step 2: Add new imports**

Add to the imports section of `src/build/render.rs`:
```rust
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::Mutex as AsyncMutex;
use futures::stream::{self, StreamExt};
```

- [ ] **Step 3: Define `BuildContext` struct**

Add after `RenderedPage`:
```rust
/// Shared state for concurrent page rendering.
///
/// Wraps mutable state in `Arc<AsyncMutex<T>>` so multiple pages can be
/// rendered concurrently via `buffer_unordered`.
pub(crate) struct BuildContext {
    pub config: Arc<SiteConfig>,
    pub env: Arc<AsyncMutex<minijinja::Environment<'static>>>,
    pub fetcher: Arc<AsyncMutex<DataFetcher>>,
    pub global_data: Arc<HashMap<String, serde_json::Value>>,
    pub dist_dir: std::path::PathBuf,
    pub build_time: String,
    pub output_paths: Arc<AsyncMutex<HashSet<String>>>,
    pub data_query_count: Arc<AtomicU32>,
    pub asset_cache: Arc<AsyncMutex<AssetCache>>,
    pub asset_client: reqwest::Client,
    pub plugin_registry: Arc<PluginRegistry>,
    pub image_cache: Arc<ImageCache>,
    pub css_cache: Arc<AsyncMutex<critical_css::StylesheetCache>>,
    pub manifest: Arc<content_hash::AssetManifest>,
    pub source_asset_collector: SourceAssetCollector,
    pub rate_limiter: Arc<RateLimiterPool>,
}
```

- [ ] **Step 4: Convert `render_static_page()` to async with `BuildContext`**

Change signature from 16 parameters to:
```rust
async fn render_static_page(
    page: &PageDef,
    ctx: &BuildContext,
) -> Result<RenderedPage> {
```

Replace all parameter references with `ctx.` field access. For mutex-wrapped fields, acquire the lock, do the operation, and drop the guard before the next `.await` point. Key pattern:

```rust
// Data fetching — lock, check cache, release, await HTTP, lock, store
let page_data = {
    let mut fetcher = ctx.fetcher.lock().await;
    data::resolve_page_data(&page.frontmatter, &mut fetcher, Some(&ctx.plugin_registry)).await?
};

ctx.data_query_count.fetch_add(page.frontmatter.data.len() as u32, Ordering::Relaxed);

// Template rendering — lock env, render, release
let rendered = {
    let env = ctx.env.lock().await;
    let tmpl = env.get_template(&tmpl_name)?;
    tmpl.render(&page_ctx)?
};

// Asset localization — lock cache, process, release
let full_html = {
    let mut cache = ctx.asset_cache.lock().await;
    assets::localize_assets(
        &full_html, &ctx.config.assets, &mut cache, &ctx.asset_client,
        &ctx.dist_dir, &source_asset_urls, &ctx.rate_limiter,
    ).await?
};

// Source assets — lock cache again
let full_html = {
    let mut cache = ctx.asset_cache.lock().await;
    source_asset::resolve_source_assets(
        &full_html, &source_asset_collector.drain(), &ctx.config.sources,
        &mut cache, &ctx.asset_client, &ctx.dist_dir, &ctx.rate_limiter,
    ).await?
};

// Critical CSS — lock css_cache
let full_html = if ctx.config.build.critical_css.enabled {
    let mut css_cache = ctx.css_cache.lock().await;
    critical_css::inline_critical_css(
        &full_html, &ctx.config.build.critical_css, &ctx.dist_dir,
        &mut css_cache, if ctx.manifest.is_empty() { None } else { Some(ctx.manifest.as_ref()) },
    )
} else {
    full_html
};

// File write — async
let full_path = ctx.dist_dir.join(&output_path);
if let Some(parent) = full_path.parent() {
    tokio::fs::create_dir_all(parent).await?;
}
tokio::fs::write(&full_path, &full_html).await?;

// Register output path — lock
{
    let mut paths = ctx.output_paths.lock().await;
    register_output_path(&url_path, &tmpl_name, &mut paths)?;
}
```

CPU-only operations (SEO injection, JSON-LD, view transitions, minification, analytics, hints) need no lock — they operate on local `String` values.

Fragment writing also becomes `tokio::fs::write`. The `localize_fragments` and `optimize_fragment_images` helper functions need the same mutex treatment for `asset_cache`.

- [ ] **Step 5: Convert `render_dynamic_page()` to async with `BuildContext`**

Same pattern as static. Change signature to:
```rust
async fn render_dynamic_page(
    page: &PageDef,
    ctx: &BuildContext,
) -> Result<Vec<RenderedPage>> {
```

The inner loop over collection items stays sequential (follow-up work to parallelize). Each iteration acquires/releases locks the same way as the static case.

Key difference: collection fetch happens once at the top, then per-item data fetch in the loop:
```rust
let items = {
    let mut fetcher = ctx.fetcher.lock().await;
    data::resolve_dynamic_page_data(&page.frontmatter, &mut fetcher, Some(&ctx.plugin_registry)).await?
};

for (idx, item) in items.iter().enumerate() {
    let item_data = {
        let mut fetcher = ctx.fetcher.lock().await;
        data::resolve_dynamic_page_data_for_item(
            &page.frontmatter, item, &mut fetcher, Some(&ctx.plugin_registry),
        ).await?
    };
    // ... render, same lock pattern as static ...
}
```

- [ ] **Step 6: Convert `localize_fragments()` to async**

The helper at line 972-998 calls `localize_assets` which is now async. Add `async` and `.await`. Since it's called from within render functions that already hold no lock at that point, it receives `&mut AssetCache` directly from the lock guard scope.

Actually, since the render functions will pass the `BuildContext`, change the signature to accept `ctx: &BuildContext` (or just `cache`, `client`, `pool` — keep it minimal):
```rust
async fn localize_fragments(
    frags: &HashMap<String, String>,
    config: &AssetsConfig,
    cache: &mut AssetCache,
    client: &reqwest::Client,
    dist_dir: &Path,
    skip_urls: &HashSet<String>,
    pool: &RateLimiterPool,
) -> Result<HashMap<String, String>> {
```

Add `.await` on the `localize_assets` call inside.

- [ ] **Step 7: Convert `build()` to async with concurrent page rendering**

Change signature:
```rust
pub async fn build(project_root: &Path, dev: bool, fresh: bool) -> Result<()> {
```

Phase 0-1 (config, discovery, output, static assets) stay the same — they're sync local operations called from async context (which is fine).

The template environment needs to own its templates with a `'static` lifetime. Change:
```rust
let env = template::setup_environment(...)?;
```
To wrap in `Arc<AsyncMutex<>>`:
```rust
let env = template::setup_environment(...)?;
let env = Arc::new(AsyncMutex::new(env));
```

Note: `minijinja::Environment<'_>` uses a lifetime for the template loader closure. To make it `'static`, the loader must own its data (no borrowed references). Check if the current loader captures references — if so, change the captures to `Arc`/owned values. The `setup_environment` function in `src/template/environment.rs` returns `Environment<'static>` if all template sources are added as owned strings (which they are — templates are read from disk and stored as `String`). Verify this before proceeding.

Build the `BuildContext`:
```rust
let ctx = Arc::new(BuildContext {
    config: Arc::new(config),
    env,
    fetcher: Arc::new(AsyncMutex::new(fetcher)),
    global_data: Arc::new(global_data),
    dist_dir: dist_dir.clone(),
    build_time,
    output_paths: Arc::new(AsyncMutex::new(HashSet::new())),
    data_query_count: Arc::new(AtomicU32::new(0)),
    asset_cache: Arc::new(AsyncMutex::new(asset_cache)),
    asset_client: reqwest::Client::new(),
    plugin_registry: Arc::new(plugin_registry),
    image_cache: Arc::new(image_cache),
    css_cache: Arc::new(AsyncMutex::new(css_cache)),
    manifest,
    source_asset_collector,
    rate_limiter,
});
```

Replace the sequential loop with:
```rust
let concurrency = std::thread::available_parallelism()
    .map(|n| n.get() * 2)
    .unwrap_or(8);

let results: Vec<Result<Vec<RenderedPage>>> = stream::iter(pages.iter())
    .map(|page| {
        let ctx = ctx.clone();
        async move {
            match &page.page_type {
                PageType::Static => {
                    let rp = render_static_page(page, &ctx).await?;
                    Ok(vec![rp])
                }
                PageType::Dynamic { param_name: _ } => {
                    render_dynamic_page(page, &ctx).await
                }
            }
        }
    })
    .buffer_unordered(concurrency)
    .collect()
    .await;

// Collect results, fail on first error.
let mut rendered_pages: Vec<RenderedPage> = Vec::new();
for result in results {
    rendered_pages.extend(result?);
}
```

Post-render phases:
- 404, sitemap, robots stay sync (local fs only, fast)
- Feed generation: `feed::generate_feeds(...)` → need to extract fetcher from the `Arc<Mutex<>>`. After page rendering is done, we can take exclusive ownership:

```rust
let mut fetcher = Arc::try_unwrap(ctx.fetcher.clone())
    .unwrap_or_else(|arc| {
        // Fallback: lock and clone if other refs exist
        let guard = arc.blocking_lock();
        // This shouldn't happen since rendering is done
        panic!("fetcher still shared after rendering");
    });
```

Actually, simpler: just lock the mutex. Note that `config` is now inside `ctx.config` (an `Arc`):
```rust
if !ctx.config.feed.is_empty() {
    let mut fetcher = ctx.fetcher.lock().await;
    let feed_count = feed::generate_feeds(
        &ctx.dist_dir, &ctx.config, &mut fetcher,
        Some(&ctx.plugin_registry), &ctx.build_time,
    ).await?;
    tracing::info!("Generating {} feed(s)... done", feed_count);
}
```

Bundling and content hash rewrite stay sync — they read/write `dist/` after all rendering is done.

- [ ] **Step 8: Update `main.rs` — Build command creates tokio runtime**

Change:
```rust
Command::Build { project, fresh } => {
    let project = std::fs::canonicalize(&project)?;
    let start = Instant::now();
    tracing::info!("Building site at {}...", project.display());
    if fresh {
        tracing::info!("Fresh mode: bypassing data cache.");
    }
    build::build(&project, false, fresh)?;
    let elapsed = start.elapsed();
    eprintln!("Built site in {:.1?}", elapsed);
    Ok(())
}
```

To:
```rust
Command::Build { project, fresh } => {
    let project = std::fs::canonicalize(&project)?;
    let start = Instant::now();
    tracing::info!("Building site at {}...", project.display());
    if fresh {
        tracing::info!("Fresh mode: bypassing data cache.");
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(build::build(&project, false, fresh))?;
    let elapsed = start.elapsed();
    eprintln!("Built site in {:.1?}", elapsed);
    Ok(())
}
```

Also update the `Audit` command which calls `build::build`:
```rust
if !no_build {
    tracing::info!("Building site...");
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(build::build(&project, false, false))?;
}
```

Actually — the Dev command already creates a runtime. Consider making `main()` itself async via `#[tokio::main]` instead of creating runtimes manually:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install().ok();
    let cli = Cli::parse();
    setup_logging(cli.verbose, cli.quiet);

    match cli.command {
        Command::Build { project, fresh } => {
            let project = std::fs::canonicalize(&project)?;
            let start = Instant::now();
            // ...
            build::build(&project, false, fresh).await?;
            // ...
        }
        Command::Dev { project, port, host, fresh } => {
            let project = std::fs::canonicalize(&project)?;
            // ...
            dev::dev_command(&project, port, &host, fresh).await?;
        }
        Command::Audit { project, format, output, no_build } => {
            // ...
            if !no_build {
                build::build(&project, false, false).await?;
            }
            // ...
        }
        // ...
    }
}
```

This removes the manual `tokio::runtime::Runtime::new()` in the Dev command too.

- [ ] **Step 9: Verify compilation**

Run: `cargo check 2>&1 | tail -30`

At this point, `build()`, render functions, and all their async dependencies should compile. The dev server will have errors — that's addressed in Task 10.

- [ ] **Step 10: Run existing tests**

Run: `cargo test 2>&1 | tail -30`

The integration tests call `eigen::build::build()` which is now async. They'll fail — fix in Task 9.

- [ ] **Step 11: Commit**

```bash
git add src/build/render.rs src/build/mod.rs src/main.rs
git commit -m "feat: convert build pipeline to async with concurrent page rendering"
```

---

### Task 9: Update integration tests

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Read `tests/integration_test.rs`**

- [ ] **Step 2: Switch all tests calling `build::build()` to `#[tokio::test]`**

For every test that calls `eigen::build::build(...)`:
- Change `#[test]` → `#[tokio::test]`
- Change `fn test_name()` → `async fn test_name()`
- Change `eigen::build::build(root, dev, fresh).unwrap()` → `eigen::build::build(root, dev, fresh).await.unwrap()`

Tests that don't call `build::build()` (if any) can stay as `#[test]`.

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1 | tail -40`

All tests should pass.

- [ ] **Step 4: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: update integration tests for async build"
```

---

### Task 10: Simplify dev server — drop blocking workarounds

**Files:**
- Modify: `src/dev/rebuild.rs`
- Modify: `src/dev/server.rs`

- [ ] **Step 1: Read `src/dev/rebuild.rs` in full**

- [ ] **Step 2: Convert `DevBuildState` to use async reqwest**

Change struct:
```rust
asset_client: reqwest::blocking::Client,
```
To:
```rust
asset_client: reqwest::Client,
```

Change constructor:
```rust
let asset_client = reqwest::blocking::Client::new();
```
To:
```rust
let asset_client = reqwest::Client::new();
```

- [ ] **Step 3: Convert `DevBuildState::new()` to async**

```rust
pub async fn new(project_root: &Path, fresh: bool) -> Result<Self> {
```

The `full_build()` call inside becomes `self.full_build().await?`.

- [ ] **Step 4: Convert `DevBuildState::rebuild()` to async**

```rust
pub async fn rebuild(&mut self, scope: RebuildScope) -> std::result::Result<(), DevBuildError> {
```

Inside: `self.full_build()` → `self.full_build().await`, and any other method calls that became async.

- [ ] **Step 5: Convert `DevBuildState::full_build()` to async**

```rust
async fn full_build(&mut self) -> Result<()> {
```

Inside:
- `data::resolve_page_data(...)` → `.await`
- `data::resolve_dynamic_page_data(...)` → `.await`
- `render_static_page_dev(...)` → `.await`
- `render_dynamic_page_dev(...)` → `.await`
- `std::fs::write(...)` → `tokio::fs::write(...).await`
- `std::fs::create_dir_all(...)` → `tokio::fs::create_dir_all(...).await`

- [ ] **Step 6: Convert `render_static_page_dev()` to async**

Add `async` to signature. Inside:
- `fetcher.fetch(...)` → `.await` (via the resolve functions)
- `assets::localize_assets(...)` → `.await`
- `source_asset::resolve_source_assets(...)` → `.await`
- File writes → `tokio::fs::write(...).await`

The dev render functions don't need `BuildContext` — they use `&mut` directly since dev rebuilds are single-task.

- [ ] **Step 7: Convert `render_dynamic_page_dev()` to async**

Same pattern as static dev.

- [ ] **Step 8: Convert `localize_fragments_dev()` to async**

Add `async`, `.await` on `localize_assets`.

- [ ] **Step 9: Read `src/dev/server.rs` in full**

- [ ] **Step 10: Simplify `dev_command()` — remove blocking workarounds**

Remove the `spawn_blocking` for initial build:
```rust
// OLD:
let build_state = tokio::task::spawn_blocking(move || -> Result<DevBuildState> {
    let state = DevBuildState::new(&build_root, fresh)?;
    Ok(state)
}).await??;

// NEW:
let build_state = DevBuildState::new(&project_root, fresh).await?;
```

Remove the thread bridge (`std::sync::mpsc`, bridge async task, dedicated OS thread). Replace with a single async task:
```rust
// OLD: sync_tx/sync_rx bridge + std::thread::spawn for rebuild loop

// NEW:
let build_state = std::sync::Arc::new(tokio::sync::Mutex::new(build_state));
let rebuild_state = build_state.clone();

tokio::spawn(async move {
    let mut rebuild_rx = rebuild_tx.subscribe();
    loop {
        match rebuild_rx.recv().await {
            Ok(scope) => {
                let mut state = rebuild_state.lock().await;
                match state.rebuild(scope).await {
                    Ok(()) => {
                        let _ = reload_signal.send(());
                    }
                    Err(e) => {
                        if !e.has_error_page {
                            eprintln!("Rebuild error: {:#}", e);
                        }
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                let mut state = rebuild_state.lock().await;
                let _ = state.rebuild(RebuildScope::Full).await;
                let _ = reload_signal.send(());
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
});
```

The file watcher thread stays as-is — `notify` is a blocking library.

- [ ] **Step 11: Verify dev server compiles**

Run: `cargo check 2>&1 | tail -20`

- [ ] **Step 12: Test dev server manually**

Run: `cargo run -- dev example_site`

Verify:
- Initial build completes
- Site loads in browser
- File changes trigger rebuild
- Live reload works

- [ ] **Step 13: Commit**

```bash
git add src/dev/rebuild.rs src/dev/server.rs
git commit -m "refactor: simplify dev server with native async build"
```

---

### Task 11: Update `Cargo.toml` and clean up

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Check if `blocking` feature is still needed**

Run: `grep -r "reqwest::blocking" src/`

If there are zero hits, remove the `blocking` feature:

Change:
```toml
reqwest = { version = "0.12", features = ["blocking", "json"] }
```
To:
```toml
reqwest = { version = "0.12", features = ["json"] }
```

If there are still references (e.g., in test utilities), keep it.

- [ ] **Step 2: Verify `futures` has the `stream` feature**

The `futures` crate includes `stream` by default. Verify it's already there — no change needed unless using a minimal feature set.

- [ ] **Step 3: Full test suite**

Run: `cargo test 2>&1`

All unit tests, integration tests, and doc tests should pass.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "chore: remove reqwest blocking feature"
```

---

### Task 12: Write docs and run final verification

**Files:**
- Create: `docs/async_build.md`

- [ ] **Step 1: Write feature documentation**

Create `docs/async_build.md`:
```markdown
# Async Build Pipeline

Eigen's build pipeline runs asynchronously on the tokio runtime, enabling
concurrent page rendering and overlapping HTTP requests.

## How It Works

Pages are rendered concurrently using `futures::stream::buffer_unordered`
with a concurrency limit of `available_parallelism * 2`. Shared state
(data cache, asset cache, template engine) is wrapped in
`Arc<tokio::sync::Mutex<T>>` for safe concurrent access.

## Concurrency Model

- **HTTP requests** overlap across pages — while one page waits for a
  response, others can render or fetch.
- **Template rendering** is serialized via a mutex on
  `minijinja::Environment` (it's `Send` but not `Sync`). Each render
  is fast (microseconds), so this is not a bottleneck.
- **File writes** use `tokio::fs` for non-blocking I/O.
- **CPU-bound work** (minification, CSS parsing, image optimization)
  runs synchronously within async tasks — tokio handles scheduling.

## Dev Server

The dev server runs builds natively on the tokio runtime. No
`spawn_blocking` or thread bridging is needed. Dev rebuilds are
sequential (single task) since they're fast enough for interactive use.

## Configuration

No new configuration is needed. The concurrency limit is automatic.
```

- [ ] **Step 2: Run full test suite one final time**

Run: `cargo test 2>&1`

- [ ] **Step 3: Run clippy**

Run: `cargo clippy 2>&1 | tail -20`

Fix any warnings.

- [ ] **Step 4: Commit**

```bash
git add docs/async_build.md
git commit -m "docs: add async build pipeline documentation"
```
