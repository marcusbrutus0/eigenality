# Rate Limiting

Eigen can throttle outbound HTTP requests during build to avoid overwhelming external APIs. Rate limits are expressed as requests per second and use a token bucket algorithm for smooth throttling.

## Configuration

### Global rate limit

Set a default rate limit for all outbound HTTP requests in `[build]`:

```toml
[build]
rate_limit = 10  # max 10 requests per second per host
```

### Per-source rate limit

Override the global default for a specific source:

```toml
[sources.strapi]
url = "https://strapi.example.com"
rate_limit = 5  # max 5 requests per second to this source
```

### No rate limit (default)

If `rate_limit` is not set anywhere, requests are not throttled. This is the default behavior.

## Behavior

- Rate limits are applied **per host**. Requests to different hosts are throttled independently.
- Per-source `rate_limit` overrides the global default for that source's host.
- Asset localization downloads (images, media found in HTML) use the global rate limit.
- The token bucket algorithm allows brief bursts while maintaining the average rate.
- Rate limiting only affects build-time requests, not the dev server proxy.
