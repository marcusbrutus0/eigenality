# Redirect Rules (`_redirects`)

Eigen generates a `dist/_redirects` file from `[[redirects]]` rules declared
in `site.toml`. The format is compatible with Cloudflare Pages and Netlify.

## Configuration

```toml
[[redirects]]
from = "/old-path"
to   = "/new-path"
status = 301          # optional; default 301

[[redirects]]
from = "/blog/*"
to   = "/posts/:splat"
status = 302

[[redirects]]
from = "https://old.example.com/*"
to   = "https://new.example.com/:splat"
status = 301
```

### Fields

| Field    | Required | Default | Notes |
|----------|----------|---------|-------|
| `from`   | yes      | —       | Source path or absolute URL |
| `to`     | yes      | —       | Destination path or absolute URL |
| `status` | no       | `301`   | Must be 301, 302, 307, or 308 |

## Conflict policy

If `static/_redirects` exists **and** `[[redirects]]` rules are configured,
the build aborts with an error. Remove one or the other — redirect ordering
is load-bearing and silent merging would produce incorrect behaviour.

If `static/_redirects` exists and `[[redirects]]` is empty, the static file
is copied to `dist/_redirects` verbatim (same as `_headers` behaviour).

## Validation

Errors (build aborts):
- `from` or `to` is empty
- `from` starts with `#` (interpreted as comment by platforms)
- `from` or `to` does not start with `/`, `http://`, or `https://`
- `status` is not 301, 302, 307, or 308 (200/rewrite is explicitly rejected)

Warnings (build continues):
- `from == to` (browser redirect loop)
- Duplicate `from` values (first match wins; later rules are dead)
- Rule count exceeds 2,000 (Cloudflare Pages per-project limit)

## Content hash exclusion

`_redirects` is excluded from the content hash pipeline by default (via
`default_hash_exclude()` in `src/config/mod.rs`). No user action required.

## Dev vs. production

The `_redirects` file is written in all builds (dev and production). There
is no separate dev-mode code path. This matches the behaviour of robots.txt
and sitemaps.
