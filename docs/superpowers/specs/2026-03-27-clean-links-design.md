# Clean Links Design

## Problem

When deploying to CDNs like Cloudflare, the server automatically resolves `/about` to `about.html`. But during local development, the dev server maps URLs to disk paths directly — so `/about` returns 404 because only `about.html` exists. Additionally, `link_to()` emits paths with `.html` extensions, which don't match the clean URLs users see in production.

## Solution

A `build.clean_links` config option that strips `.html` extensions from generated links, plus an always-on dev server fallback that resolves extensionless paths to `.html` files.

## Config

```toml
[build]
clean_links = true   # default: false
```

Independent of `clean_urls` (which controls output file structure). Composes with it:
- `clean_urls` off: file is `dist/about.html`, link becomes `/about`
- `clean_urls` on: file is `dist/about/index.html`, link becomes `/about`

## Cleaning Function

A shared `to_clean_link(path) -> String` function applied wherever links are emitted:

| Input | Output |
|---|---|
| `/about.html` | `/about` |
| `/posts/my-post.html` | `/posts/my-post` |
| `/about/index.html` | `/about` |
| `/index.html` | `/` |
| `/` | `/` |
| `/style.css` | `/style.css` (non-`.html` unchanged) |

## Touch Points

### 1. `link_to()` (src/template/functions.rs)

When `clean_links` is enabled, clean the path before emitting `href` and `hx-push-url`. The `hx-get` fragment path is unaffected — `compute_fragment_path` already handles clean inputs.

Before:
```
href="/about.html" hx-get="/_fragments/about.html" hx-target="#content" hx-push-url="/about.html"
```

After (`clean_links = true`):
```
href="/about" hx-get="/_fragments/about.html" hx-target="#content" hx-push-url="/about"
```

### 2. `page.current_url` (src/build/context.rs via render.rs)

When `clean_links` is enabled, apply `to_clean_link()` to the URL path when constructing `PageMeta`. The `url_path` stored in `RenderedPage` stays as-is for internal use.

### 3. Dev Server (src/dev/server.rs)

Always-on middleware (not gated by config): if `ServeDir` can't find a file for an extensionless path, try appending `.html` before returning 404.

Request flow:
1. `/about` → try `dist/about` (directory or file)
2. Not found → try `dist/about.html`
3. Found → serve it. Not found → normal 404.

This mimics Cloudflare's behavior and is safe to always enable — it only triggers for paths without extensions.

### 4. Sitemap (src/build/sitemap.rs)

When `clean_links` is enabled, apply `to_clean_link()` to sitemap URLs. This takes precedence over `sitemap.clean_urls`:

- `clean_links` on → `/about` (no extension, no trailing slash)
- `clean_links` off, `sitemap.clean_urls` on → `/about/` (trailing slash)
- Both off → `/about.html` (raw file paths)

### 5. No Changes Needed

- `compute_fragment_path` — already handles both `/about` and `/about.html`
- `robots.txt` — only references `sitemap.xml` location, not page URLs
- Output file structure — `clean_links` doesn't affect how files are written to disk

## Testing

- Unit tests for `to_clean_link()` covering all cases in the table above
- Unit tests for `link_to()` with `clean_links` enabled
- Unit test for `page.current_url` with `clean_links` enabled
- Unit test for sitemap URL generation with `clean_links` enabled
- Integration test: dev server resolves `/about` to `about.html`
- Integration test: `clean_links` + `clean_urls` compose correctly
