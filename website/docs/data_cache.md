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

Both GET and POST requests are cached. POST requests are keyed by
URL + request body hash, so different POST bodies are cached separately.

Responses without ETag or Last-Modified headers are not cached to disk
(no conditional request would be possible).

## Cache Location

All cached data lives in `.eigen_cache/data/` under the project root.
Each cached response produces two files:

- `<hash>.body` — raw response bytes
- `<hash>.meta` — JSON with the cache key, ETag, and Last-Modified values

This directory is safe to delete at any time — Eigen will re-fetch on the
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
- If a `304` response has no usable cached body, Eigen re-fetches.
- Disk write failures are logged as warnings and do not fail the build.
