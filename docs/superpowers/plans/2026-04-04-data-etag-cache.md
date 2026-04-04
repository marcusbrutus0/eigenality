# Data Source ETag Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add disk-persisted HTTP caching with conditional requests (ETag / Last-Modified / 304) to the data fetcher, matching the pattern already used for assets.

**Architecture:** New `DataCache` struct in `src/data/cache.rs` manages `.eigen_cache/data/` with `.body` + `.meta` sidecar files per cached response. `DataFetcher` gains an `Option<DataCache>` field and sends conditional headers when cached metadata exists. A `--fresh` CLI flag bypasses the cache entirely.

**Tech Stack:** Rust, serde_json, reqwest::blocking, tempfile (tests)

---

### File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `src/data/cache.rs` | `DataCache` struct — disk-backed HTTP response cache |
| Modify | `src/data/mod.rs` | Add `mod cache; pub use cache::DataCache;` |
| Modify | `src/data/fetcher.rs` | Add `Option<DataCache>` field, conditional request logic |
| Modify | `src/cli.rs:22-45` | Add `--fresh` flag to `Build` and `Dev` commands |
| Modify | `src/main.rs:28-55` | Thread `fresh` flag to `build::build` and `dev_command` |
| Modify | `src/build/render.rs:50-53,129-130` | Accept `fresh` param, create `DataCache` conditionally |
| Modify | `src/dev/server.rs:39,46-48` | Accept `fresh` param, pass to `DevBuildState` |
| Modify | `src/dev/rebuild.rs:78-80,112-115` | Accept `fresh` param, create `DataCache` conditionally |
| Create | `docs/data_cache.md` | Feature documentation |

---

### Task 1: `DataCache` struct — core storage

**Files:**
- Create: `src/data/cache.rs`

- [ ] **Step 1: Write the failing tests**

Add tests at the bottom of the new file:

```rust
//! Disk-backed HTTP response cache for data sources.
//!
//! Stores raw response bytes and HTTP caching headers (ETag, Last-Modified)
//! in `.eigen_cache/data/`, enabling conditional requests on subsequent builds.

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Metadata stored alongside a cached HTTP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCacheMeta {
    /// Cache key: `"GET:<url>"` or `"POST:<url>:<body_hash>"`.
    pub cache_key: String,
    /// HTTP `ETag` header from the server, if provided.
    pub etag: Option<String>,
    /// HTTP `Last-Modified` header from the server, if provided.
    pub last_modified: Option<String>,
}

/// Disk-backed cache for remote data source responses.
pub struct DataCache {
    /// Path to `.eigen_cache/data/`.
    cache_dir: PathBuf,
    /// In-memory index: cache_key → metadata.
    index: HashMap<String, DataCacheMeta>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn store_and_read_round_trip() {
        let tmp = TempDir::new().unwrap();
        let mut cache = DataCache::open(tmp.path()).unwrap();

        let key = "GET:https://api.example.com/posts";
        let body = br#"[{"id":1,"title":"Hello"}]"#;

        cache.store(key, body, Some("\"abc123\""), Some("Tue, 01 Apr 2026 00:00:00 GMT")).unwrap();

        // Metadata is indexed.
        let meta = cache.get(key).unwrap();
        assert_eq!(meta.etag.as_deref(), Some("\"abc123\""));
        assert_eq!(meta.last_modified.as_deref(), Some("Tue, 01 Apr 2026 00:00:00 GMT"));

        // Body bytes are readable.
        let read_body = cache.read(key).unwrap();
        assert_eq!(read_body, body);
    }

    #[test]
    fn read_returns_none_for_unknown_key() {
        let tmp = TempDir::new().unwrap();
        let cache = DataCache::open(tmp.path()).unwrap();

        assert!(cache.get("GET:https://unknown.com").is_none());
        assert!(cache.read("GET:https://unknown.com").is_none());
    }

    #[test]
    fn clear_removes_all_entries() {
        let tmp = TempDir::new().unwrap();
        let mut cache = DataCache::open(tmp.path()).unwrap();

        cache.store("GET:https://a.com", b"[]", None, None).unwrap();
        cache.store("GET:https://b.com", b"{}", None, None).unwrap();

        cache.clear().unwrap();

        assert!(cache.get("GET:https://a.com").is_none());
        assert!(cache.read("GET:https://a.com").is_none());
        assert!(cache.get("GET:https://b.com").is_none());
    }

    #[test]
    fn reopen_loads_persisted_entries() {
        let tmp = TempDir::new().unwrap();

        {
            let mut cache = DataCache::open(tmp.path()).unwrap();
            cache.store("GET:https://api.com/data", b"[1,2,3]", Some("\"v1\""), None).unwrap();
        }

        // Re-open from the same directory.
        let cache = DataCache::open(tmp.path()).unwrap();
        let meta = cache.get("GET:https://api.com/data").unwrap();
        assert_eq!(meta.etag.as_deref(), Some("\"v1\""));

        let body = cache.read("GET:https://api.com/data").unwrap();
        assert_eq!(body, b"[1,2,3]");
    }

    #[test]
    fn corrupted_meta_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join(".eigen_cache").join("data");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // Write a malformed .meta file.
        std::fs::write(cache_dir.join("bad.meta"), "not valid json").unwrap();

        // open should succeed, just skip the bad entry.
        let cache = DataCache::open(tmp.path()).unwrap();
        assert!(cache.index.is_empty());
    }

    #[test]
    fn missing_body_returns_none() {
        let tmp = TempDir::new().unwrap();
        let mut cache = DataCache::open(tmp.path()).unwrap();

        cache.store("GET:https://api.com/x", b"data", None, None).unwrap();

        // Delete the .body file behind the cache's back.
        let hash = cache_key_hash("GET:https://api.com/x");
        let body_path = tmp.path().join(".eigen_cache").join("data").join(format!("{}.body", hash));
        std::fs::remove_file(body_path).unwrap();

        // read returns None, get still returns metadata.
        assert!(cache.get("GET:https://api.com/x").is_some());
        assert!(cache.read("GET:https://api.com/x").is_none());
    }

    #[test]
    fn store_overwrites_existing_entry() {
        let tmp = TempDir::new().unwrap();
        let mut cache = DataCache::open(tmp.path()).unwrap();

        let key = "GET:https://api.com/posts";
        cache.store(key, b"old", Some("\"v1\""), None).unwrap();
        cache.store(key, b"new", Some("\"v2\""), None).unwrap();

        assert_eq!(cache.get(key).unwrap().etag.as_deref(), Some("\"v2\""));
        assert_eq!(cache.read(key).unwrap(), b"new");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib data::cache -- --nocapture`
Expected: compilation errors — `DataCache` methods not implemented yet.

- [ ] **Step 3: Implement `DataCache`**

Add the implementation between the struct definitions and the `#[cfg(test)]` block:

```rust
/// Hash a cache key into a hex string for filenames.
pub(crate) fn cache_key_hash(key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

impl DataCache {
    /// Open (or create) the data cache for a project.
    ///
    /// Loads all `.meta` files into memory. Malformed files are logged
    /// as warnings and skipped.
    pub fn open(project_root: &Path) -> Result<Self> {
        let cache_dir = project_root.join(".eigen_cache").join("data");
        std::fs::create_dir_all(&cache_dir)
            .wrap_err_with(|| format!("Failed to create data cache dir: {}", cache_dir.display()))?;

        let mut index = HashMap::new();

        for entry in std::fs::read_dir(&cache_dir).wrap_err("Failed to read data cache dir")? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("meta") {
                match std::fs::read_to_string(&path) {
                    Ok(content) => match serde_json::from_str::<DataCacheMeta>(&content) {
                        Ok(meta) => { index.insert(meta.cache_key.clone(), meta); }
                        Err(e) => {
                            tracing::warn!("Skipping malformed data cache meta {}: {}", path.display(), e);
                        }
                    },
                    Err(e) => {
                        tracing::warn!("Failed to read data cache meta {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(Self { cache_dir, index })
    }

    /// Look up cached metadata for a cache key.
    pub fn get(&self, cache_key: &str) -> Option<&DataCacheMeta> {
        self.index.get(cache_key)
    }

    /// Read the cached response body for a cache key.
    ///
    /// Returns `None` if the body file is missing or unreadable.
    pub fn read(&self, cache_key: &str) -> Option<Vec<u8>> {
        if !self.index.contains_key(cache_key) {
            return None;
        }
        let hash = cache_key_hash(cache_key);
        let body_path = self.cache_dir.join(format!("{}.body", hash));
        std::fs::read(&body_path).ok()
    }

    /// Store a response body and its HTTP caching metadata.
    ///
    /// Writes both `.body` and `.meta` files, and updates the in-memory
    /// index. Disk write failures are logged as warnings.
    pub fn store(
        &mut self,
        cache_key: &str,
        body: &[u8],
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<()> {
        let hash = cache_key_hash(cache_key);

        let body_path = self.cache_dir.join(format!("{}.body", hash));
        std::fs::write(&body_path, body)
            .wrap_err_with(|| format!("Failed to write data cache body: {}", body_path.display()))?;

        let meta = DataCacheMeta {
            cache_key: cache_key.to_string(),
            etag: etag.map(|s| s.to_string()),
            last_modified: last_modified.map(|s| s.to_string()),
        };

        let meta_path = self.cache_dir.join(format!("{}.meta", hash));
        let meta_json = serde_json::to_string_pretty(&meta)
            .wrap_err("Failed to serialize data cache meta")?;
        std::fs::write(&meta_path, meta_json)
            .wrap_err_with(|| format!("Failed to write data cache meta: {}", meta_path.display()))?;

        self.index.insert(cache_key.to_string(), meta);
        Ok(())
    }

    /// Delete all cached files and clear the in-memory index.
    pub fn clear(&mut self) -> Result<()> {
        for entry in std::fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                std::fs::remove_file(&path)?;
            }
        }
        self.index.clear();
        Ok(())
    }
}
```

- [ ] **Step 4: Register the module**

In `src/data/mod.rs`, add:

```rust
mod cache;
pub use cache::DataCache;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib data::cache -- --nocapture`
Expected: all 7 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/data/cache.rs src/data/mod.rs
git commit -m "feat: add DataCache for disk-persisted data source caching"
```

---

### Task 2: Wire `DataCache` into `DataFetcher`

**Files:**
- Modify: `src/data/fetcher.rs:16-27` (struct fields)
- Modify: `src/data/fetcher.rs:29-39` (`new` method)
- Modify: `src/data/fetcher.rs:128-214` (`fetch_source` method)

- [ ] **Step 1: Write the failing test**

Add at the bottom of the existing test module in `src/data/fetcher.rs`:

```rust
#[test]
fn fetch_source_uses_data_cache_on_304() {
    use std::io::Read as _;
    use std::net::TcpListener;

    // Spin up a tiny HTTP server that returns an ETag on the first request
    // and 304 on conditional requests with matching ETag.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let server = std::thread::spawn(move || {
        // Accept two connections.
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);

            if request.contains("If-None-Match") && request.contains("\"test-etag\"") {
                // Conditional request — return 304.
                let response = "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n";
                std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
            } else {
                // First request — return data with ETag.
                let body = r#"[{"id":1}]"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"test-etag\"\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body,
                );
                std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
            }
        }
    });

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("_data")).unwrap();

    let mut sources = HashMap::new();
    sources.insert("api".to_string(), crate::config::SourceConfig {
        url: format!("http://{}", addr),
        headers: HashMap::new(),
    });

    let mut cache = crate::data::DataCache::open(root).unwrap();
    let mut fetcher = DataFetcher::new(&sources, root, Some(&mut cache));

    // First fetch — should hit the server, get 200, cache the response.
    let result = fetcher.fetch_source("api", "/posts", &HttpMethod::Get, None).unwrap();
    assert_eq!(result, serde_json::json!([{"id": 1}]));

    // Clear in-memory cache to force disk lookup path.
    fetcher.url_cache.clear();

    // Second fetch — should send If-None-Match, get 304, use cached body.
    let result = fetcher.fetch_source("api", "/posts", &HttpMethod::Get, None).unwrap();
    assert_eq!(result, serde_json::json!([{"id": 1}]));

    server.join().unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib data::fetcher::tests::fetch_source_uses_data_cache_on_304 -- --nocapture`
Expected: compilation error — `DataFetcher::new` doesn't accept a cache parameter.

- [ ] **Step 3: Update `DataFetcher` struct and constructor**

Change the struct definition at `src/data/fetcher.rs:16-27`:

```rust
pub struct DataFetcher<'a> {
    /// Source definitions from `site.toml`.
    sources: HashMap<String, SourceConfig>,
    /// In-memory cache for parsed JSON keyed by cache key (avoids re-parsing within a build).
    url_cache: HashMap<String, Value>,
    /// Cache for local file data keyed by file path (relative to `_data/`).
    file_cache: HashMap<String, Value>,
    /// Path to the project's `_data/` directory.
    data_dir: PathBuf,
    /// HTTP client (reused across requests).
    client: reqwest::blocking::Client,
    /// Optional disk cache for conditional HTTP requests.
    data_cache: Option<&'a mut super::cache::DataCache>,
}
```

Update `new` at `src/data/fetcher.rs:29-39`:

```rust
impl<'a> DataFetcher<'a> {
    pub fn new(
        sources: &HashMap<String, SourceConfig>,
        project_root: &Path,
        data_cache: Option<&'a mut super::cache::DataCache>,
    ) -> Self {
        Self {
            sources: sources.clone(),
            url_cache: HashMap::new(),
            file_cache: HashMap::new(),
            data_dir: project_root.join("_data"),
            client: reqwest::blocking::Client::new(),
            data_cache,
        }
    }
```

**Note:** Adding a lifetime to `DataFetcher` will cause compilation errors in all call sites. Those are fixed in Task 3. For now, also update the test helper in `fetcher.rs`:

```rust
fn test_fetcher(root: &Path) -> DataFetcher<'_> {
    DataFetcher::new(&HashMap::new(), root, None)
}
```

- [ ] **Step 4: Update `fetch_source` with conditional request logic**

Replace the HTTP request section of `fetch_source` (lines 180-214) with:

```rust
    fn fetch_source(
        &mut self,
        source_name: &str,
        path: &str,
        method: &HttpMethod,
        body: Option<&serde_json::Value>,
    ) -> Result<Value> {
        let source = self
            .sources
            .get(source_name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Source '{}' not found in site.toml. Available: {}",
                    source_name,
                    self.sources.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?
            .clone();

        // Build full URL.
        let full_url = format!(
            "{}{}",
            source.url.trim_end_matches('/'),
            if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{}", path)
            }
        );

        // Build cache key.
        let cache_key = match method {
            HttpMethod::Get => format!("GET:{}", full_url),
            HttpMethod::Post => {
                let body_hash = match body {
                    Some(b) => {
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};
                        let body_str = serde_json::to_string(b).unwrap_or_default();
                        let mut hasher = DefaultHasher::new();
                        body_str.hash(&mut hasher);
                        hasher.finish().to_string()
                    }
                    None => String::new(),
                };
                format!("POST:{}:{}", full_url, body_hash)
            }
        };

        // Level 1: in-memory cache.
        if let Some(cached) = self.url_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let mut headers = reqwest::header::HeaderMap::new();
        for (key, val) in &source.headers {
            let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                .wrap_err_with(|| format!("Invalid header name: {}", key))?;
            let value = reqwest::header::HeaderValue::from_str(val)
                .wrap_err_with(|| format!("Invalid header value for {}", key))?;
            headers.insert(name, value);
        }

        // Level 2: add conditional headers if disk cache has metadata.
        if let Some(ref cache) = self.data_cache {
            if let Some(meta) = cache.get(&cache_key) {
                if let Some(ref etag) = meta.etag {
                    if let Ok(val) = reqwest::header::HeaderValue::from_str(etag) {
                        headers.insert(reqwest::header::IF_NONE_MATCH, val);
                    }
                }
                if let Some(ref last_mod) = meta.last_modified {
                    if let Ok(val) = reqwest::header::HeaderValue::from_str(last_mod) {
                        headers.insert(reqwest::header::IF_MODIFIED_SINCE, val);
                    }
                }
            }
        }

        let response = match method {
            HttpMethod::Get => {
                self.client.get(&full_url).headers(headers).send()
            }
            HttpMethod::Post => {
                let mut req = self.client.post(&full_url).headers(headers);
                if let Some(b) = body {
                    req = req.json(b);
                }
                req.send()
            }
        }
        .wrap_err_with(|| format!("HTTP request failed for {}", full_url))?;

        let status = response.status();

        // 304 Not Modified — use cached body.
        if status == reqwest::StatusCode::NOT_MODIFIED {
            if let Some(ref cache) = self.data_cache {
                if let Some(body_bytes) = cache.read(&cache_key) {
                    match serde_json::from_slice::<Value>(&body_bytes) {
                        Ok(value) => {
                            tracing::debug!("Data cache hit (304): {}", cache_key);
                            self.url_cache.insert(cache_key, value.clone());
                            return Ok(value);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Cached body for {} failed to parse: {}. Re-fetching.",
                                cache_key, e
                            );
                            // Fall through to a full re-fetch below.
                        }
                    }
                }
            }
            // 304 but no usable cached body — re-fetch without conditional headers.
            let response = match method {
                HttpMethod::Get => self.client.get(&full_url).send(),
                HttpMethod::Post => {
                    let mut req = self.client.post(&full_url);
                    if let Some(b) = body {
                        req = req.json(b);
                    }
                    req.send()
                }
            }
            .wrap_err_with(|| format!("HTTP re-fetch failed for {}", full_url))?;

            let status = response.status();
            if !status.is_success() {
                eyre::bail!("HTTP {} from {}", status, full_url);
            }
            let etag = response.headers().get("etag").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
            let last_modified = response.headers().get("last-modified").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
            let raw_bytes = response.bytes().wrap_err_with(|| format!("Failed to read response from {}", full_url))?.to_vec();

            if let Some(ref mut cache) = self.data_cache {
                if let Err(e) = cache.store(&cache_key, &raw_bytes, etag.as_deref(), last_modified.as_deref()) {
                    tracing::warn!("Failed to cache data for {}: {}", cache_key, e);
                }
            }

            let value: Value = serde_json::from_slice(&raw_bytes)
                .wrap_err_with(|| format!("Failed to parse JSON from {}", full_url))?;
            self.url_cache.insert(cache_key, value.clone());
            return Ok(value);
        }

        if !status.is_success() {
            eyre::bail!("HTTP {} from {}", status, full_url);
        }

        // Extract caching headers and body.
        let etag = response.headers().get("etag").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
        let last_modified = response.headers().get("last-modified").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
        let raw_bytes = response.bytes().wrap_err_with(|| format!("Failed to read response from {}", full_url))?.to_vec();

        // Store in disk cache.
        if let Some(ref mut cache) = self.data_cache {
            if let Err(e) = cache.store(&cache_key, &raw_bytes, etag.as_deref(), last_modified.as_deref()) {
                tracing::warn!("Failed to cache data for {}: {}", cache_key, e);
            }
        }

        let value: Value = serde_json::from_slice(&raw_bytes)
            .wrap_err_with(|| format!("Failed to parse JSON response from {}", full_url))?;
        self.url_cache.insert(cache_key, value.clone());
        Ok(value)
    }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib data::fetcher::tests::fetch_source_uses_data_cache_on_304 -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/data/fetcher.rs
git commit -m "feat: wire DataCache into DataFetcher for conditional requests"
```

---

### Task 3: Fix all call sites for new `DataFetcher` signature

**Files:**
- Modify: `src/build/render.rs:130`
- Modify: `src/dev/rebuild.rs:78-80,112-115`
- Modify: `src/data/query.rs` (test helpers)
- Modify: `src/build/feed.rs` (test helpers)
- Modify: `tests/integration_test.rs:1951`

The `DataFetcher::new` signature now takes `Option<&mut DataCache>`. All existing call sites that don't need caching pass `None`.

- [ ] **Step 1: Update `src/build/render.rs`**

At line 130, change:

```rust
let mut fetcher = DataFetcher::new(&config.sources, project_root);
```

to:

```rust
let mut fetcher = DataFetcher::new(&config.sources, project_root, None);
```

(This will be updated to use `DataCache` in Task 5 when we add the `--fresh` plumbing.)

- [ ] **Step 2: Update `src/dev/rebuild.rs`**

At line 80, change:

```rust
let fetcher = DataFetcher::new(&config.sources, project_root);
```

to:

```rust
let fetcher = DataFetcher::new(&config.sources, project_root, None);
```

At line 115, change:

```rust
self.fetcher = DataFetcher::new(&self.config.sources, &self.project_root);
```

to:

```rust
self.fetcher = DataFetcher::new(&self.config.sources, &self.project_root, None);
```

(These will also be updated in Task 5.)

- [ ] **Step 3: Update test helpers**

In `src/data/query.rs`, update every `DataFetcher::new(&HashMap::new(), root)` call to `DataFetcher::new(&HashMap::new(), root, None)`. There are approximately 10 occurrences.

In `src/build/feed.rs`, update every `DataFetcher::new(&std::collections::HashMap::new(), root)` call to `DataFetcher::new(&std::collections::HashMap::new(), root, None)`. There are approximately 7 occurrences.

In `tests/integration_test.rs` at line 1951, change:

```rust
let mut fetcher = eigen::data::DataFetcher::new(&sources, root);
```

to:

```rust
let mut fetcher = eigen::data::DataFetcher::new(&sources, root, None);
```

- [ ] **Step 4: Fix any lifetime annotation issues**

Adding `'a` to `DataFetcher<'a>` may require updating type annotations in `DevBuildState` (line 67) and `resolve_page_data` / `resolve_dynamic_page_data` signatures in `src/data/query.rs`. If the lifetime is too disruptive, consider making `data_cache` an owned `Option<DataCache>` instead of a borrowed reference — this avoids lifetime propagation while achieving the same result. The `DataFetcher` would then own the `DataCache`. Choose whichever compiles cleanly.

If using owned `Option<DataCache>`:

```rust
pub struct DataFetcher {
    // ... existing fields ...
    data_cache: Option<super::cache::DataCache>,
}

impl DataFetcher {
    pub fn new(
        sources: &HashMap<String, SourceConfig>,
        project_root: &Path,
        data_cache: Option<super::cache::DataCache>,
    ) -> Self {
        Self {
            sources: sources.clone(),
            url_cache: HashMap::new(),
            file_cache: HashMap::new(),
            data_dir: project_root.join("_data"),
            client: reqwest::blocking::Client::new(),
            data_cache,
        }
    }
}
```

This avoids adding lifetime parameters to `DataFetcher`, `DevBuildState`, and all downstream consumers. The `DataCache` is cheap to move (a `PathBuf` + `HashMap`).

- [ ] **Step 5: Verify everything compiles and tests pass**

Run: `cargo test`
Expected: all existing tests pass, no compilation errors.

- [ ] **Step 6: Commit**

```bash
git add src/build/render.rs src/dev/rebuild.rs src/data/query.rs src/build/feed.rs tests/integration_test.rs src/data/fetcher.rs
git commit -m "refactor: update all DataFetcher call sites for optional DataCache"
```

---

### Task 4: Add `--fresh` CLI flag

**Files:**
- Modify: `src/cli.rs:22-26` (Build command)
- Modify: `src/cli.rs:33-45` (Dev command)

- [ ] **Step 1: Add the flag to both commands**

In `src/cli.rs`, update the `Build` variant:

```rust
Build {
    /// Path to the project root (default: current directory)
    #[arg(short, long, default_value = ".")]
    project: PathBuf,

    /// Bypass the data source cache and re-fetch all remote data
    #[arg(long)]
    fresh: bool,
},
```

Update the `Dev` variant:

```rust
Dev {
    /// Path to the project root (default: current directory)
    #[arg(short, long, default_value = ".")]
    project: PathBuf,

    /// Port to bind the dev server to
    #[arg(long, default_value_t = 3000)]
    port: u16,

    /// Host address to bind the dev server to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Bypass the data source cache and re-fetch all remote data
    #[arg(long)]
    fresh: bool,
},
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compilation errors in `src/main.rs` because the match arms don't destructure `fresh`. That's expected — fixed in Task 5.

- [ ] **Step 3: Commit**

```bash
git add src/cli.rs
git commit -m "feat: add --fresh flag to build and dev commands"
```

---

### Task 5: Thread `--fresh` through `main`, `build`, and `dev`

**Files:**
- Modify: `src/main.rs:28-55`
- Modify: `src/build/render.rs:50-53,129-130`
- Modify: `src/dev/server.rs:39,46-48`
- Modify: `src/dev/rebuild.rs:76-90,112-115`

- [ ] **Step 1: Update `main.rs` match arms**

Update the `Build` match arm at line 28:

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

Update the `Dev` match arm at line 44:

```rust
Command::Dev { project, port, host, fresh } => {
    let project = std::fs::canonicalize(&project)?;
    tracing::info!("Starting dev server for {} on {host}:{port}...", project.display());
    if fresh {
        tracing::info!("Fresh mode: bypassing data cache.");
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        dev::dev_command(&project, port, &host, fresh).await
    })?;

    Ok(())
}
```

Also update the `Audit` match arm's build call (line 62):

```rust
build::build(&project, false, false)?;
```

- [ ] **Step 2: Update `build::build` signature and body**

In `src/build/render.rs`, change the function signature at line 53:

```rust
pub fn build(project_root: &Path, dev: bool, fresh: bool) -> Result<()> {
```

At line 130, replace:

```rust
let mut fetcher = DataFetcher::new(&config.sources, project_root, None);
```

with:

```rust
let data_cache = if fresh {
    None
} else {
    match DataCache::open(project_root) {
        Ok(cache) => Some(cache),
        Err(e) => {
            tracing::warn!("Failed to open data cache, proceeding without: {}", e);
            None
        }
    }
};
let mut fetcher = DataFetcher::new(&config.sources, project_root, data_cache);
```

Add the import at the top of the file:

```rust
use crate::data::DataCache;
```

- [ ] **Step 3: Update `dev_command` signature**

In `src/dev/server.rs`, change line 39:

```rust
pub async fn dev_command(project_root: &Path, port: u16, host: &str, fresh: bool) -> Result<()> {
```

Update the `DevBuildState::new` call at line 48:

```rust
let state = DevBuildState::new(&build_root, fresh)?;
```

- [ ] **Step 4: Update `DevBuildState` to accept `fresh`**

In `src/dev/rebuild.rs`, update the constructor at line 78:

```rust
pub fn new(project_root: &Path, fresh: bool) -> Result<Self> {
    let config = crate::config::load_config(project_root)?;
    let data_cache = if fresh {
        None
    } else {
        match crate::data::DataCache::open(project_root) {
            Ok(cache) => Some(cache),
            Err(e) => {
                tracing::warn!("Failed to open data cache: {}", e);
                None
            }
        }
    };
    let fetcher = DataFetcher::new(&config.sources, project_root, data_cache);
```

Add a `fresh` field to `DevBuildState`:

```rust
pub struct DevBuildState {
    // ... existing fields ...
    /// Whether to bypass the data cache.
    fresh: bool,
}
```

And use it in the config-reload path (around line 115):

```rust
let data_cache = if self.fresh {
    None
} else {
    match crate::data::DataCache::open(&self.project_root) {
        Ok(cache) => Some(cache),
        Err(e) => {
            tracing::warn!("Failed to open data cache: {}", e);
            None
        }
    }
};
self.fetcher = DataFetcher::new(&self.config.sources, &self.project_root, data_cache);
```

Also update the full_build call inside `DevBuildState` that calls `build::build` (if any) to pass `fresh`.

- [ ] **Step 5: Verify everything compiles and tests pass**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/build/render.rs src/dev/server.rs src/dev/rebuild.rs
git commit -m "feat: thread --fresh flag through build and dev pipelines"
```

---

### Task 6: Documentation

**Files:**
- Create: `docs/data_cache.md`

- [ ] **Step 1: Write the feature doc**

```markdown
# Data Source Caching

Eigen caches remote data source responses on disk using HTTP conditional
requests (ETag and Last-Modified headers). This avoids redundant fetches
when the remote data hasn't changed, speeding up builds.

## How It Works

When Eigen fetches data from a remote source configured in `site.toml`, it:

1. Checks an in-memory cache first (within a single build, the same URL
   is never fetched twice).
2. If cached metadata exists on disk (`.eigen_cache/data/`), sends
   conditional headers (`If-None-Match`, `If-Modified-Since`) with the
   request.
3. If the server responds with `304 Not Modified`, uses the cached
   response body without re-downloading.
4. If the server responds with `200 OK`, stores the new response and
   caching headers for next time.

The cache stores two files per response:
- `<hash>.body` — raw response bytes
- `<hash>.meta` — JSON with the cache key, ETag, and Last-Modified values

## Cache Location

All cached data lives in `.eigen_cache/data/` under the project root. This
directory is safe to delete at any time — Eigen will simply re-fetch on the
next build.

## Bypassing the Cache

Use `--fresh` to skip the cache entirely:

```bash
eigen build --fresh
eigen dev --fresh
```

This ignores all cached data for that run but does not delete the cache
files. A subsequent run without `--fresh` can still use them.

## Error Handling

The cache never prevents a successful build:
- Corrupted cache files are silently skipped with a warning log.
- If a `304` response has no usable cached body, Eigen re-fetches normally.
- Disk write failures are logged as warnings and do not fail the build.
```

- [ ] **Step 2: Commit**

```bash
git add docs/data_cache.md
git commit -m "docs: add data source caching documentation"
```

---

### Task 7: End-to-end verification

**Files:**
- No new files — manual verification.

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 2: Test with the example site**

Run: `cargo run -- build -p example_site`
Expected: builds successfully with no errors.

Run: `cargo run -- build -p example_site --fresh`
Expected: builds successfully, logs "Fresh mode: bypassing data cache."

- [ ] **Step 3: Verify cache files**

If the example site has remote data sources, check that `.eigen_cache/data/` contains `.body` and `.meta` files after a normal build. If it has no remote sources, the directory will be empty (or not created) — that's expected.

- [ ] **Step 4: Commit (if any fixups were needed)**

```bash
git add -A
git commit -m "fix: address issues found during end-to-end verification"
```
