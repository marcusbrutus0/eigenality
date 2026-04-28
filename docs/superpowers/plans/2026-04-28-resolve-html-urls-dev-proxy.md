# resolve_html_urls Dev Proxy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `resolve_html_urls` runs in dev mode, rewrite root-relative URLs to `/_proxy/{source}/...` proxy URLs instead of absolute CMS URLs, so the dev server can forward requests with auth headers and images/links work during local development.

**Architecture:** Move `build_proxy_url`, `extract_host`, and `extract_path` from `template/functions.rs` to `build/source_asset.rs` as shared helpers. Add `dev_mode: bool` to `DataFetcher`. Update `resolve_html_urls_in_value` to accept dev context and produce proxy URLs when in dev mode, using the same `build_proxy_url` logic that `source_asset()` already uses.

**Tech Stack:** Rust, existing proxy infrastructure in `src/dev/proxy.rs`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/build/source_asset.rs` | Modify | Receive `build_proxy_url`, `extract_host`, `extract_path` + unit tests |
| `src/template/functions.rs` | Modify | Import moved functions, remove local definitions |
| `src/data/fetcher.rs` | Modify | Add `dev_mode: bool` field + constructor param |
| `src/data/html_urls.rs` | Modify | Accept dev context, produce proxy URLs when dev_mode |
| `src/data/query.rs` | Modify | Thread dev_mode + base URL through `html_url_ctx` |
| `src/build/render.rs` | Modify | Pass `false` for dev_mode |
| `src/dev/rebuild.rs` | Modify | Pass `true` for dev_mode |
| `website/docs/resolve_html_urls.md` | Modify | Update dev proxy row in comparison table |

---

## Task 1: Move proxy URL helpers to `source_asset.rs`

**Files:**
- Modify: `src/build/source_asset.rs`
- Modify: `src/template/functions.rs:166-212`

- [ ] **Step 1: Write the failing tests**

Add to the test section in `src/build/source_asset.rs`:

```rust
#[test]
fn build_proxy_url_same_host_relative() {
    let result = build_proxy_url("my_cms", "https://cms.example.com/uploads/photo.jpg", "https://cms.example.com");
    assert_eq!(result, "/_proxy/my_cms/uploads/photo.jpg");
}

#[test]
fn build_proxy_url_same_host_with_base_path() {
    let result = build_proxy_url(
        "cms_assets",
        "http://localhost:4001/apps/id8nxt/uploads/file/abc123/hero.png",
        "http://localhost:4001/apps/id8nxt/uploads/file",
    );
    assert_eq!(result, "/_proxy/cms_assets/abc123/hero.png");
}

#[test]
fn build_proxy_url_cross_host() {
    let result = build_proxy_url(
        "my_cms",
        "https://media.example.com/photo.jpg",
        "https://cms.example.com",
    );
    assert_eq!(
        result,
        "/_proxy/my_cms/__source_asset__/https://media.example.com/photo.jpg",
    );
}

#[test]
fn extract_host_basic() {
    assert_eq!(extract_host("https://cms.example.com/api"), "cms.example.com");
}

#[test]
fn extract_host_with_port() {
    assert_eq!(extract_host("http://localhost:4001/api"), "localhost");
}

#[test]
fn extract_path_basic() {
    assert_eq!(extract_path("https://cms.example.com/api/v1"), "/api/v1");
}

#[test]
fn extract_path_no_path() {
    assert_eq!(extract_path("https://cms.example.com"), "/");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test build_proxy_url -- --nocapture`
Expected: FAIL — functions don't exist in `source_asset.rs`

- [ ] **Step 3: Move the three functions**

Cut `build_proxy_url`, `extract_host`, and `extract_path` from `src/template/functions.rs` (lines 166-212) and add them to `src/build/source_asset.rs` above `#[cfg(test)]`. Change visibility from `fn` to `pub fn`:

```rust
/// Build a dev proxy URL for a source asset.
///
/// - Same-host URLs: strip the source base path and use `/_proxy/{source}/{relative}`.
///   The proxy handler prepends the source base URL, so only the relative tail is needed.
/// - Cross-host URLs: use `/_proxy/{source}/__source_asset__/{full_url}`.
pub fn build_proxy_url(source_name: &str, resolved_url: &str, source_base_url: &str) -> String {
    let base_host = extract_host(source_base_url);
    let url_host = extract_host(resolved_url);

    if base_host == url_host {
        let base_path = extract_path(source_base_url);
        let resolved_path = extract_path(resolved_url);
        let relative = resolved_path
            .strip_prefix(base_path)
            .unwrap_or(resolved_path);
        let relative = relative.trim_start_matches('/');
        format!("/_proxy/{}/{}", source_name, relative)
    } else {
        format!(
            "/_proxy/{}/{}{}",
            source_name, SOURCE_ASSET_PROXY_PREFIX, resolved_url
        )
    }
}

/// Extract the path portion of a URL (everything after `scheme://host[:port]`).
pub fn extract_path(url: &str) -> &str {
    url.find("://")
        .and_then(|i| url[i + 3..].find('/').map(|j| &url[i + 3 + j..]))
        .unwrap_or("/")
}

/// Extract hostname from a URL (without port).
pub fn extract_host(url: &str) -> &str {
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

- [ ] **Step 4: Update `template/functions.rs` to import from `source_asset`**

In `src/template/functions.rs`, the import on line 16 already includes `SOURCE_ASSET_PROXY_PREFIX`. Extend it:

```rust
use crate::build::source_asset::{SOURCE_ASSET_PROXY_PREFIX, SourceAssetCollector, resolve_url, build_proxy_url};
```

Remove the three local function definitions (`build_proxy_url`, `extract_path`, `extract_host` — the block from lines 166-212). The call site at line 114 (`build_proxy_url(source_name, &resolved, &source.url)`) stays unchanged.

- [ ] **Step 5: Run all tests to verify nothing broke**

Run: `cargo test`
Expected: PASS — all tests pass including the existing `source_asset_dev_mode_*` tests in `template/functions.rs` and the new unit tests in `source_asset.rs`

- [ ] **Step 6: Commit**

```bash
git add src/build/source_asset.rs src/template/functions.rs
git commit -m "refactor: move proxy URL helpers to source_asset module"
```

---

## Task 2: Add `dev_mode: bool` to DataFetcher

**Files:**
- Modify: `src/data/fetcher.rs:74-115`
- Modify: `src/build/render.rs:260`
- Modify: `src/dev/rebuild.rs:88, 128`
- Modify: all test callers

- [ ] **Step 1: Write the failing test**

Add to the test section in `src/data/fetcher.rs`:

```rust
#[test]
fn new_accepts_dev_mode() {
    let pool = Arc::new(RateLimiterPool::new(None, &HashMap::new()));
    let dir = tempfile::tempdir().unwrap();
    let fetcher = DataFetcher::new(
        &HashMap::new(),
        dir.path(),
        None,
        pool,
        None,
        true,
    );
    assert!(fetcher.dev_mode);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test new_accepts_dev_mode -- --nocapture`
Expected: FAIL — `DataFetcher::new` doesn't accept 6 arguments

- [ ] **Step 3: Add `dev_mode` field to DataFetcher**

In `src/data/fetcher.rs`, add the field and update the constructor:

Add to the struct (after `source_asset_collector`):

```rust
    pub(crate) dev_mode: bool,
```

Update the constructor signature and body:

```rust
    pub fn new(
        sources: &HashMap<String, SourceConfig>,
        project_root: &Path,
        data_cache: Option<super::cache::DataCache>,
        rate_limiter: Arc<RateLimiterPool>,
        source_asset_collector: Option<SourceAssetCollector>,
        dev_mode: bool,
    ) -> Self {
        Self {
            sources: sources.clone(),
            url_cache: HashMap::new(),
            file_cache: HashMap::new(),
            data_dir: project_root.join("_data"),
            client: reqwest::Client::new(),
            data_cache,
            rate_limiter,
            source_asset_collector,
            dev_mode,
        }
    }
```

- [ ] **Step 4: Fix all existing callers**

Production code:
- `src/build/render.rs:260` — add `false` as 6th arg
- `src/dev/rebuild.rs:88` — add `true` as 6th arg
- `src/dev/rebuild.rs:128` — add `true` as 6th arg

Test code (all pass `false`):
- `src/data/fetcher.rs` — all test `DataFetcher::new` calls (approximately 12 call sites)
- `src/data/query.rs` — all test `DataFetcher::new` calls (approximately 16 call sites)
- `src/build/feed.rs` — all test `DataFetcher::new` calls (6 call sites)
- `tests/integration_test.rs` — 1 call site

Use grep to find them all: `grep -rn "DataFetcher::new(" src/ tests/`

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: PASS — all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/data/fetcher.rs src/build/render.rs src/dev/rebuild.rs src/data/query.rs src/build/feed.rs tests/integration_test.rs
git commit -m "feat: add dev_mode field to DataFetcher"
```

---

## Task 3: Update resolver to support dev mode proxy URLs

**Files:**
- Modify: `src/data/html_urls.rs`

- [ ] **Step 1: Write the failing tests**

Add to the test section in `src/data/html_urls.rs`:

```rust
#[test]
fn resolve_dev_mode_rewrites_to_proxy_url_same_host() {
    let input = json!({
        "body": r#"<img src="/uploads/photo.jpg">"#
    });
    let (result, urls) = resolve_with_dev_mode(
        &input,
        "https://cms.example.com",
        "my_cms",
        "https://cms.example.com/api",
    );
    assert_eq!(
        result["body"].as_str().unwrap(),
        r#"<img src="/_proxy/my_cms/uploads/photo.jpg">"#,
    );
    assert!(urls.is_empty(), "dev mode should not collect URLs");
}

#[test]
fn resolve_dev_mode_cross_host_uses_source_asset_prefix() {
    let input = json!({
        "body": r#"<img src="/media/photo.jpg">"#
    });
    // origin is media.example.com but source base is cms.example.com → cross-host
    let (result, urls) = resolve_with_dev_mode(
        &input,
        "https://media.example.com",
        "my_cms",
        "https://cms.example.com/api",
    );
    assert_eq!(
        result["body"].as_str().unwrap(),
        r#"<img src="/_proxy/my_cms/__source_asset__/https://media.example.com/media/photo.jpg">"#,
    );
    assert!(urls.is_empty());
}

fn resolve_with_dev_mode(
    value: &Value,
    origin: &str,
    source_name: &str,
    source_base_url: &str,
) -> (Value, Vec<String>) {
    let mut collected = Vec::new();
    let result = resolve_value_with_mode(
        value.clone(),
        origin,
        &mut collected,
        Some(&DevRewriteCtx { source_name, source_base_url }),
    );
    (result, collected)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test resolve_dev_mode -- --nocapture`
Expected: FAIL — `resolve_value_with_mode` and `DevRewriteCtx` don't exist

- [ ] **Step 3: Implement dev mode rewriting**

In `src/data/html_urls.rs`, add the import at the top:

```rust
use crate::build::source_asset::build_proxy_url;
```

Add a context struct for dev mode rewriting (above the public functions):

```rust
pub struct DevRewriteCtx<'a> {
    pub source_name: &'a str,
    pub source_base_url: &'a str,
}
```

Update `resolve_html_urls_in_value` to accept an optional dev context:

```rust
pub fn resolve_html_urls_in_value(
    value: Value,
    origin: &str,
    source_name: &str,
    collector: Option<&SourceAssetCollector>,
    dev_ctx: Option<&DevRewriteCtx<'_>>,
) -> Value {
    let mut urls = Vec::new();
    let result = resolve_value_with_mode(value, origin, &mut urls, dev_ctx);
    if dev_ctx.is_none() {
        if let Some(collector) = collector {
            for url in urls {
                collector.push(source_name.to_string(), url);
            }
        }
    }
    result
}
```

Add the dev-mode-aware walker (rename the existing `resolve_value` to keep it as the internal workhorse, add a new entry that handles the mode switch):

```rust
fn resolve_value_with_mode(
    value: Value,
    origin: &str,
    collected: &mut Vec<String>,
    dev_ctx: Option<&DevRewriteCtx<'_>>,
) -> Value {
    match value {
        Value::String(s) => {
            let resolved = resolve_string_with_mode(&s, origin, collected, dev_ctx);
            Value::String(resolved)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|v| resolve_value_with_mode(v, origin, collected, dev_ctx))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, resolve_value_with_mode(v, origin, collected, dev_ctx)))
                .collect(),
        ),
        other => other,
    }
}

fn resolve_string_with_mode(
    s: &str,
    origin: &str,
    collected: &mut Vec<String>,
    dev_ctx: Option<&DevRewriteCtx<'_>>,
) -> String {
    let after_dq = ROOT_RELATIVE_RE.replace_all(s, |caps: &regex::Captures| {
        let path = &caps[1];
        let absolute = format!("{}{}", origin, path);
        let rewritten = match dev_ctx {
            Some(ctx) => build_proxy_url(ctx.source_name, &absolute, ctx.source_base_url),
            None => {
                collected.push(absolute.clone());
                absolute
            }
        };
        let full = &caps[0];
        let eq_pos = full.find('=').unwrap();
        format!("{}=\"{}\"", &full[..eq_pos], rewritten)
    });

    let result = ROOT_RELATIVE_SQ_RE.replace_all(&after_dq, |caps: &regex::Captures| {
        let path = &caps[1];
        let absolute = format!("{}{}", origin, path);
        let rewritten = match dev_ctx {
            Some(ctx) => build_proxy_url(ctx.source_name, &absolute, ctx.source_base_url),
            None => {
                collected.push(absolute.clone());
                absolute
            }
        };
        let full = &caps[0];
        let eq_pos = full.find('=').unwrap();
        format!("{}='{}'", &full[..eq_pos], rewritten)
    });

    result.into_owned()
}
```

Remove the old `resolve_value` and `resolve_string` functions — they are fully replaced by `resolve_value_with_mode` and `resolve_string_with_mode`.

Update the existing test helper `resolve_html_urls_in_value_collect` to pass `None` for dev_ctx:

```rust
fn resolve_html_urls_in_value_collect(value: &Value, origin: &str) -> (Value, Vec<String>) {
    let mut collected = Vec::new();
    let result = resolve_value_with_mode(value.clone(), origin, &mut collected, None);
    (result, collected)
}
```

Update the existing `resolve_with_collector` test to pass `None` for the new param:

```rust
#[test]
fn resolve_with_collector() {
    use crate::build::source_asset::SourceAssetCollector;

    let input = json!({
        "body": "<img src=\"/uploads/photo.jpg\">"
    });
    let collector = SourceAssetCollector::new();
    let result = resolve_html_urls_in_value(
        input,
        "https://cms.example.com",
        "my_cms",
        Some(&collector),
        None,
    );
    assert_eq!(
        result["body"].as_str().unwrap(),
        "<img src=\"https://cms.example.com/uploads/photo.jpg\">",
    );
    let requests = collector.drain();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].source_name, "my_cms");
    assert_eq!(requests[0].url, "https://cms.example.com/uploads/photo.jpg");
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test html_urls -- --nocapture`
Expected: PASS — all existing + 3 new tests pass

- [ ] **Step 5: Commit**

```bash
git add src/data/html_urls.rs
git commit -m "feat: support dev mode proxy URLs in resolve_html_urls"
```

---

## Task 4: Thread dev context through call sites

**Files:**
- Modify: `src/data/fetcher.rs:175-207`
- Modify: `src/data/query.rs:512-607`

- [ ] **Step 1: Write the failing test**

Add to the test section in `src/data/fetcher.rs`:

```rust
#[tokio::test]
async fn fetch_dev_mode_rewrites_to_proxy_urls() {
    use crate::build::source_asset::SourceAssetCollector;

    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/api/entries")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"body": "<img src=\"/uploads/photo.jpg\">"}"#)
        .create_async().await;

    let mut sources = HashMap::new();
    sources.insert("cms".to_string(), SourceConfig {
        url: server.url() + "/api",
        headers: HashMap::new(),
        rate_limit: None,
        resolve_html_urls: true,
    });

    let collector = SourceAssetCollector::new();
    let pool = Arc::new(RateLimiterPool::new(None, &sources));
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("_data")).unwrap();

    let mut fetcher = DataFetcher::new(
        &sources,
        dir.path(),
        None,
        pool,
        Some(collector.clone()),
        true, // dev_mode
    );

    let query = DataQuery {
        source: Some("cms".to_string()),
        path: Some("/entries".to_string()),
        ..Default::default()
    };

    let result = fetcher.fetch(&query, None).await.unwrap();

    let body = result["body"].as_str().unwrap();
    assert!(
        body.contains("/_proxy/cms/"),
        "Expected proxy URL in dev mode, got: '{}'",
        body,
    );
    assert!(
        !body.contains(&server.url()),
        "Should not contain absolute server URL in dev mode, got: '{}'",
        body,
    );

    let requests = collector.drain();
    assert!(requests.is_empty(), "Dev mode should not collect source assets");

    mock.assert_async().await;
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test fetch_dev_mode_rewrites -- --nocapture`
Expected: FAIL — `resolve_html_urls_in_value` call doesn't pass dev context yet

- [ ] **Step 3: Update `fetch()` in `fetcher.rs`**

In `src/data/fetcher.rs`, update the resolve_html_urls block (around lines 175-207). Replace the current `resolve_html_urls_in_value` call:

```rust
        // Resolve root-relative HTML URLs if the source has resolve_html_urls enabled.
        let result = if let Some(ref source_name) = query.source {
            if let Some(source) = self.sources.get(source_name) {
                if source.resolve_html_urls {
                    match super::html_urls::extract_origin(&source.url) {
                        Some(origin) => {
                            let dev_ctx = if self.dev_mode {
                                Some(super::html_urls::DevRewriteCtx {
                                    source_name,
                                    source_base_url: &source.url,
                                })
                            } else {
                                None
                            };
                            super::html_urls::resolve_html_urls_in_value(
                                result,
                                &origin,
                                source_name,
                                self.source_asset_collector.as_ref(),
                                dev_ctx.as_ref(),
                            )
                        }
                        None => {
                            tracing::warn!(
                                source = source_name,
                                url = %source.url,
                                "resolve_html_urls enabled but could not extract origin from source URL",
                            );
                            result
                        }
                    }
                } else {
                    result
                }
            } else {
                result
            }
        } else {
            result
        };
```

- [ ] **Step 4: Update `fetch_unlocked()` in `query.rs`**

In `src/data/query.rs`, update Phase 1 to also capture `dev_mode` and `source_base_url`. Change the `html_url_ctx` type from `Option<(String, Option<SourceAssetCollector>)>` to `Option<(String, Option<SourceAssetCollector>, bool, String)>` — the last two fields are `dev_mode` and `source_base_url`.

Update the Phase 1 block:

```rust
        let html_url_ctx = f.sources.get(source_name)
            .filter(|s| s.resolve_html_urls)
            .and_then(|s| {
                let origin = match crate::data::html_urls::extract_origin(&s.url) {
                    Some(o) => o,
                    None => {
                        tracing::warn!(
                            source = source_name,
                            url = %s.url,
                            "resolve_html_urls enabled but could not extract origin from source URL",
                        );
                        return None;
                    }
                };
                let collector = f.source_asset_collector.clone();
                let dev_mode = f.dev_mode;
                let source_base_url = s.url.clone();
                Some((origin, collector, dev_mode, source_base_url))
            });
```

Update `apply_html_url_resolution`:

```rust
fn apply_html_url_resolution(
    value: Value,
    source_name: &str,
    ctx: &Option<(String, Option<SourceAssetCollector>, bool, String)>,
) -> Value {
    match ctx {
        Some((origin, collector, dev_mode, source_base_url)) => {
            let dev_ctx = if *dev_mode {
                Some(crate::data::html_urls::DevRewriteCtx {
                    source_name,
                    source_base_url,
                })
            } else {
                None
            };
            crate::data::html_urls::resolve_html_urls_in_value(
                value,
                origin,
                source_name,
                collector.as_ref(),
                dev_ctx.as_ref(),
            )
        }
        None => value,
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS — all tests including the new dev mode test

- [ ] **Step 6: Commit**

```bash
git add src/data/fetcher.rs src/data/query.rs
git commit -m "feat: thread dev mode context through resolve_html_urls call sites"
```

---

## Task 5: Update documentation

**Files:**
- Modify: `website/docs/resolve_html_urls.md`

- [ ] **Step 1: Update the comparison table**

In `website/docs/resolve_html_urls.md`, replace the last row of the comparison table:

```markdown
| Dev proxy | Yes | Yes (proxy URLs in dev mode) |
```

And add a brief section after "What is NOT touched":

```markdown
## Dev mode

In dev mode (`eigen dev`), resolved URLs are rewritten to proxy paths
(`/_proxy/{source}/...`) instead of absolute CMS URLs. The dev server
forwards these requests with the source's auth headers, so images and
links work during local development without downloading assets to disk.
```

- [ ] **Step 2: Commit**

```bash
git add website/docs/resolve_html_urls.md
git commit -m "docs: update resolve_html_urls for dev proxy support"
```

---

## Design Decisions

### Why `DevRewriteCtx` struct instead of extra bool + string params?

Groups the dev-mode-specific parameters into a single `Option<&DevRewriteCtx>`. In production mode, there's zero overhead (just `None`). It also makes the function signature clearer: `None` means production, `Some(ctx)` means dev mode.

### Why move helpers instead of duplicating?

`build_proxy_url` has non-trivial same-host vs. cross-host logic. Duplicating it would create a maintenance burden. Both `source_asset()` and `resolve_html_urls` need identical proxy URL construction.

### Why not rewrite the proxy URL in a post-processing step?

The URLs are embedded in JSON string values (HTML content). Rewriting them after they've been inserted into the JSON tree would require a second pass. Doing it inline during the resolve step is both simpler and more efficient.
