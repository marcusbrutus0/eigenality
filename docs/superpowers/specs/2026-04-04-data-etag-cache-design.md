# Data Source ETag Cache

## Problem

Eigen's asset pipeline uses ETag and Last-Modified headers for conditional
HTTP requests, avoiding redundant downloads across builds. The data fetcher
(`DataFetcher`) has no equivalent — every build re-fetches all remote data
sources from scratch, even when the data hasn't changed. For sites with many
or large data sources this wastes time and bandwidth.

## Goal

Add disk-persisted HTTP caching with conditional requests (ETag /
Last-Modified / 304 Not Modified) to the data fetcher, using the same
approach that already works for assets.

## Design

### `DataCache` (`src/data/cache.rs`)

A disk-backed cache stored in `.eigen_cache/data/`. Each cached response
produces two files:

- **`<hash>.body`** — raw response bytes exactly as received from the server.
- **`<hash>.meta`** — JSON sidecar containing cache metadata.

The `<hash>` is derived from the cache key using the same hashing approach
the fetcher already uses for POST body hashing (`DefaultHasher`), hex-encoded.

#### `DataCacheMeta`

```rust
#[derive(Debug, Serialize, Deserialize)]
struct DataCacheMeta {
    /// Cache key: "GET:<url>" or "POST:<url>:<body_hash>"
    cache_key: String,
    /// HTTP ETag header from the server, if provided.
    etag: Option<String>,
    /// HTTP Last-Modified header from the server, if provided.
    last_modified: Option<String>,
}
```

#### Public API

| Method | Description |
|--------|-------------|
| `open(project_root: &Path) -> Result<Self>` | Create/open `.eigen_cache/data/`, load all `.meta` files into an in-memory index (`HashMap<String, DataCacheMeta>`). Malformed `.meta` files are logged as warnings and skipped. |
| `get(cache_key: &str) -> Option<&DataCacheMeta>` | Look up metadata for building conditional request headers. |
| `read(cache_key: &str) -> Option<Vec<u8>>` | Read the cached `.body` file. Returns `None` if the file is missing or unreadable. |
| `store(cache_key: &str, body: &[u8], etag: Option<&str>, last_modified: Option<&str>) -> Result<()>` | Write `.body` and `.meta` files, update the in-memory index. Disk write failures are logged as warnings and do not fail the build. |
| `clear() -> Result<()>` | Delete all files in `.eigen_cache/data/` and clear the in-memory index. |

### Changes to `DataFetcher`

`DataFetcher` gains an `Option<DataCache>` field. When present, `fetch_source`
uses a three-level lookup:

1. **In-memory cache** (`url_cache: HashMap<String, Value>`) — hit means
   same endpoint was already fetched this build. Return immediately.
2. **Disk cache with conditional request** — if `DataCache` has metadata for
   the cache key, add `If-None-Match` and/or `If-Modified-Since` headers to
   the HTTP request.
   - **304 Not Modified** → read bytes from `DataCache::read()`, parse JSON,
     insert into in-memory cache, return.
   - **2xx** → extract ETag/Last-Modified from response headers, store raw
     bytes via `DataCache::store()`, parse JSON, insert into in-memory cache,
     return.
3. **Full fetch** — no cached metadata exists. Fetch, store if cache is
   available, return.

When `DataCache` is `None` (i.e. `--fresh` mode), the fetcher skips steps 2
entirely and always does a full fetch, matching current behavior.

#### Cache key construction

Identical to the existing key scheme in `fetch_source`:

- GET: `"GET:<full_url>"`
- POST: `"POST:<full_url>:<body_hash>"` where body_hash is `DefaultHasher`
  over the serialized JSON body.

### `--fresh` CLI flag

Added to both `Build` and `Dev` commands:

```
eigen build --fresh
eigen dev --fresh
```

When passed, `DataFetcher` is constructed without a `DataCache`. Existing
cache files are **not deleted** — they are simply ignored for that run. A
subsequent run without `--fresh` can still use them.

### Error handling

The cache must never prevent a successful build:

| Scenario | Behavior |
|----------|----------|
| `.meta` file is malformed | Skipped during `DataCache::open`, warning logged |
| `.body` file missing when `.meta` exists | Ignore cached meta, do full fetch |
| 304 received but cached body fails JSON parse | Warning logged, do full fetch, discard bad cache entry |
| Disk write failure on `store()` | Warning logged, build continues without caching |

### Testing

1. **`DataCache` unit tests** — use `tempfile::TempDir`. Test
   `store`/`read`/`get`/`clear`. Test corrupted `.meta` files are skipped.
   Test missing `.body` files return `None`.

2. **`DataFetcher` conditional request tests** — spin up a lightweight HTTP
   server (using `axum` or similar, already a project dependency) that:
   - Returns an ETag on first request
   - Returns 304 on conditional requests with matching ETag
   - Verify the fetcher uses cached data on 304
   - Verify POST requests with bodies are cached correctly

3. **Integration test** — extend existing integration tests to verify that
   data source caching works end-to-end across two sequential builds.

## Out of scope

- Per-source cache bypass (e.g. `cache = false` in source config)
- Cache TTL / max-age expiry
- Generalized shared cache with `AssetCache`

These can be added later if needed.
