# Async Build Pipeline — Design Spec

**Date:** 2026-04-08
**Status:** Approved
**Scope:** Convert `build::build()` from synchronous to fully async, enabling concurrent page rendering, concurrent HTTP fetching, and native integration with the tokio-based dev server.

## Goals

1. **Concurrent data fetching** — overlap HTTP requests across pages instead of sequential blocking
2. **Concurrent page rendering** — render multiple pages in parallel via `buffer_unordered`
3. **Dev server unification** — eliminate `spawn_blocking` / OS thread workaround; build runs natively on the tokio runtime
4. **Faster builds** — all of the above combine for significant build time reduction

## Non-Goals

- Parallelizing the inner loop of dynamic page rendering (collection items within one template) — tracked as follow-up work
- Converting CPU-only post-processing (bundling, content hash rewrite) to async — no benefit
- Adding rayon or other thread-pool parallelism — tokio is sufficient

## Current Architecture

- `build::build()` is fully synchronous with a sequential `for page in &pages` loop
- `DataFetcher` uses `reqwest::blocking::Client` with in-memory caches (`url_cache`, `file_cache`)
- Asset downloading uses `reqwest::blocking::Client` throughout
- The dev server (`dev_command`) is async (tokio/axum) but uses `spawn_blocking` and a dedicated OS thread for builds because `reqwest::blocking` panics inside async runtimes
- Rate limiter uses `futures::executor::block_on` to bridge async `governor` into sync code
- Render functions (`render_static_page`, `render_dynamic_page`) take 16 parameters each

## Design

### 1. Async I/O Layer

Convert all network I/O from blocking to async.

**DataFetcher:**
- `reqwest::blocking::Client` → `reqwest::Client`
- `fetch()`, `send_request()`, `fetch_source()`, `fetch_file()` become `async fn`
- In-memory caches (`url_cache`, `file_cache`) stay inside the struct; the struct is wrapped at the caller level
- `DataCache` (disk cache) stays sync behind the mutex

**Asset downloading (`assets/download.rs`, `assets/rewrite.rs`):**
- All `download_*` functions become `async fn`
- Take `&reqwest::Client` instead of `&reqwest::blocking::Client`
- `localize_remote_assets()` becomes async

**Source assets (`build/source_asset.rs`):**
- `download_source_assets()` becomes async

**Rate limiter (`build/rate_limit.rs`):**
- `RateLimiterPool::wait()` becomes `async fn`
- Drops `futures::executor::block_on(limiter.until_ready())` in favor of `limiter.until_ready().await`

**Unchanged:** Template rendering, HTML rewriting (`lol_html`), minification, CSS parsing — all CPU-bound, stay sync, called from within async functions.

### 2. Shared State Model

**`Arc<tokio::sync::Mutex<T>>` for:**
- `DataFetcher` — serializes cache reads/writes; HTTP awaits happen outside the lock
- `AssetCache` — same lock-check-release-fetch-reacquire-store pattern
- `StylesheetCache` — low contention, cache warms fast
- `output_paths: HashSet<String>` — collision detection

**`Arc<AtomicU32>` for:**
- `data_query_count` — just a counter

**Shared immutably (`Arc<T>`):**
- `SiteConfig` — read-only after load
- `AssetManifest` — already `Arc<AssetManifest>`
- `global_data: HashMap<String, Value>` — read-only
- `PluginRegistry` — read-only after construction
- `ImageCache` — stateless struct (just a `PathBuf` to cache dir), safe to share

**`minijinja::Environment`:**
- `Send` but not `Sync` → `Arc<tokio::sync::Mutex<Environment>>`
- Template rendering is CPU-bound and brief; the lock serializes rendering but overlaps with I/O waits on other tasks

**`BuildContext` struct** replaces the 16-parameter function signatures:

```rust
struct BuildContext {
    config: Arc<SiteConfig>,
    env: Arc<tokio::sync::Mutex<minijinja::Environment<'static>>>,
    fetcher: Arc<tokio::sync::Mutex<DataFetcher>>,
    global_data: Arc<HashMap<String, Value>>,
    dist_dir: PathBuf,
    build_time: String,
    output_paths: Arc<tokio::sync::Mutex<HashSet<String>>>,
    data_query_count: Arc<AtomicU32>,
    asset_cache: Arc<tokio::sync::Mutex<AssetCache>>,
    asset_client: reqwest::Client,
    plugin_registry: Arc<PluginRegistry>,
    image_cache: Arc<ImageCache>,
    css_cache: Arc<tokio::sync::Mutex<StylesheetCache>>,
    manifest: Arc<AssetManifest>,
    source_asset_collector: SourceAssetCollector,
    rate_limiter: Arc<RateLimiterPool>,
}
```

### 3. Concurrent Page Rendering

Replace the sequential page loop with bounded concurrent execution:

```rust
let ctx = Arc::new(BuildContext { ... });

let results: Vec<Result<Vec<RenderedPage>>> = futures::stream::iter(pages)
    .map(|page| {
        let ctx = ctx.clone();
        async move {
            match &page.page_type {
                PageType::Static => {
                    let rp = render_static_page(&page, &ctx).await?;
                    Ok(vec![rp])
                }
                PageType::Dynamic { .. } => {
                    render_dynamic_page(&page, &ctx).await
                }
            }
        }
    })
    .buffer_unordered(concurrency_limit)
    .collect()
    .await;
```

**Concurrency limit:** Default `num_cpus * 2`, configurable via `[build]` in `site.toml`.

**Lock discipline inside each page render:**
1. Acquire `fetcher` lock → check cache → release lock
2. If cache miss: `await` HTTP (no lock held — other pages proceed)
3. Re-acquire `fetcher` lock → store result → release lock
4. Acquire `env` lock → render template → release lock
5. Post-processing (SEO, critical CSS, minification) — no lock needed
6. Write output with `tokio::fs::write`
7. Acquire `output_paths` lock → register → release lock

**Error handling:** Fail-fast — first error short-circuits, matching current behavior.

**Dynamic pages:** The inner loop over collection items stays sequential within a single template. Outer parallelism across templates provides the main win. Inner parallelism is tracked as follow-up work.

### 4. Build Pipeline Phases

```
Phase 0: Setup (sync — config, discovery, output dir, template env)
    ↓
Phase 1: Copy static assets + content hashing (sync — local fs only)
    ↓
Phase 2: Render pages concurrently (async — buffer_unordered)
    ↓
Phase 3: Post-render
    3a: 404, sitemap, robots.txt (sync, fast)
    3b: Feed generation (async — fetches data)
    3c: Plugin post_build hooks (sync)
    ↓
Phase 4: Bundling + content hash rewrite (sync — operates on dist/)
```

**`build()` signature:**
```rust
pub async fn build(project_root: &Path, dev: bool, fresh: bool) -> Result<()>
```

**Caller in `main.rs`:**
```rust
Command::Build { project, fresh } => {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(build::build(&project, false, fresh))?;
}
```

### 5. Dev Server Integration

**Current pain points eliminated:**
- No more `spawn_blocking` for initial build
- No more dedicated OS thread for rebuild loop
- No more async→sync bridge (`tokio::spawn` → `std::sync::mpsc`)

**New dev server flow:**
1. `DevBuildState::new().await` — initial build, async
2. `tokio::spawn(watcher)` — file watcher stays OS thread (notify is blocking)
3. `tokio::spawn(rebuild_loop)` — receives broadcast, calls `state.rebuild().await`
4. `axum::serve(...)` — HTTP server

**`DevBuildState` changes:**
- `reqwest::blocking::Client` → `reqwest::Client`
- `DataFetcher` used directly (not behind `Arc<Mutex<>>`) — dev rebuilds are single-task, one rebuild at a time
- Same for `AssetCache` — no concurrency needed within dev rebuilds

Production builds get full concurrency via `BuildContext`; dev rebuilds stay simple sequential-async.

### 6. Migration Strategy

Each step compiles and passes tests independently.

**Step 1: Introduce `BuildContext` struct**
- Extract the 16-parameter signatures into `BuildContext` (still sync)
- Pure refactor, no behavior change

**Step 2: Convert `DataFetcher` to async**
- `reqwest::blocking::Client` → `reqwest::Client`
- All methods become `async fn`
- `RateLimiterPool::wait()` becomes `async fn`
- Tests become `#[tokio::test]`

**Step 3: Convert asset layer to async**
- `assets/download.rs`, `assets/rewrite.rs`, `assets/cache.rs` become async
- `build/source_asset.rs` becomes async

**Step 4: Convert render pipeline to async**
- Wrap shared state in `Arc<Mutex<>>`
- `build()` becomes `async fn` with `buffer_unordered`
- File writes become `tokio::fs::write`
- `main.rs` Build command creates tokio runtime

**Step 5: Convert feed generation to async**
- `feed::generate_feeds` becomes async (calls `DataFetcher::fetch`)

**Step 6: Simplify dev server**
- Drop `spawn_blocking`, thread bridge, `std::sync::mpsc`
- `DevBuildState` methods become async
- Rebuild loop becomes a tokio task

## Dependencies

- `reqwest` — drop `blocking` feature, keep `json` (check if any non-build code needs blocking)
- `futures` — already present, used for `stream::iter` + `buffer_unordered`
- `tokio` — already `features = ["full"]`
- No new dependencies required

## Testing

- All existing integration tests in `tests/integration_test.rs` must pass — they call `build::build()` so they'll need a tokio runtime wrapper or `#[tokio::test]`
- `DataFetcher` unit tests switch to `#[tokio::test]`
- Asset download tests switch to `#[tokio::test]`
- New test: verify concurrent page rendering produces identical output to sequential (determinism check)
