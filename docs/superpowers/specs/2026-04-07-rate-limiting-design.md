# Configurable Rate Limiting for Build-Time HTTP Requests

## Problem

Eigen makes outbound HTTP requests during build to fetch data from sources, download assets, and resolve authenticated source assets. Without rate limiting, rapid sequential requests can overwhelm external APIs, triggering 429 responses or getting the client blocked.

## Solution

Add configurable request-per-second rate limiting using the `governor` crate's token bucket algorithm. Rate limits can be set globally and overridden per source.

## Config

```toml
# Global default — applies to all outbound requests unless overridden
[build]
rate_limit = 10  # requests per second, optional, no limit if omitted

# Per-source override
[sources.strapi]
url = "https://strapi.example.com"
rate_limit = 5  # overrides global for this source
```

- `rate_limit` is optional everywhere. If omitted entirely, no throttling (current behavior preserved).
- Per-source `rate_limit` overrides the global default for requests to that source.
- Asset localization requests (non-source URLs found in HTML) use the global rate limit.

## Architecture

### `RateLimiterPool`

A struct that manages per-host `governor::RateLimiter` instances:

- **Keyed by host** — each unique hostname gets its own rate limiter.
- **Lazy creation** — limiters created on first request to a host, stored in a `HashMap`.
- **Rate resolution** — when creating a limiter for a host, check if it matches a configured source's URL. If so, use that source's `rate_limit`. Otherwise, fall back to the global `rate_limit`.
- **Blocking wait** — `pool.wait(&url)` calls `governor`'s blocking `until_ready()`, which sleeps until a token is available.
- **No limiter if no config** — if neither global nor per-source rate limit is set for a host, `wait()` returns immediately with no delay.

The pool is created once at build start and passed as `&RateLimiterPool` to request sites.

### `governor` usage

- `RateLimiter::direct()` with `Quota::per_second(NonZeroU32)` for the configured rate.
- Token bucket naturally smooths bursts.
- Blocking `until_ready()` fits the sequential blocking reqwest usage.

## Integration Points

### `fetcher.rs` — Data source fetching

Before each `client.get()`/`client.post()` call in `fetch_source()`, call `pool.wait(&url)`. The source name is known, so the pool resolves the per-source rate limit.

### `download.rs` — Asset localization

Before each download in `download_asset_with_headers()` / `ensure_asset_with_headers()`, call `pool.wait(&url)`. These are non-source URLs (images, media found in HTML), so they use the global rate limit.

### `source_asset.rs` — Authenticated asset downloads

Before each download in `resolve_source_assets()`, call `pool.wait(&url)`. The source name is available, so per-source limits apply.

## Error Handling

None needed. `governor`'s `until_ready()` blocks until a token is available — it does not fail. A very low rate limit (e.g., 1 req/s) simply slows the build.

## Testing

- **Unit test**: `RateLimiterPool` correctly resolves per-source vs global rate limits for a given URL.
- **Unit test**: With a low rate limit (e.g., 2 req/s), verify that multiple `wait()` calls take at least the expected duration.
- No integration test needed — rate limiting does not change build output, only timing.

## Dependencies

- `governor` crate added to `Cargo.toml`.
