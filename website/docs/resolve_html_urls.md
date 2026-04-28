# Resolve HTML URLs

Automatically resolve root-relative asset paths in HTML content from data
sources and download them with the source's authentication headers.

## Problem

Content APIs (CMSes like Substrukt) return HTML with root-relative upload
paths:

    <img src="/api/v1/apps/my-app/uploads/abc123/photo.jpg">

These paths require bearer-token auth — the same token used to fetch content.
Eigen's `localize_assets` only processes absolute `http://` URLs, and the
`source_asset()` template function only works with explicit template calls —
it can't process URLs embedded inside HTML strings rendered via
`{{ entry.body | safe }}`.

## Usage

Add `resolve_html_urls = true` to any source in `site.toml`:

    [sources.cms]
    url = "${CMS_URL}/api/v1/apps/id8nxt"
    headers = { Authorization = "Bearer ${SUBSTRUKT_API_TOKEN}" }
    resolve_html_urls = true

No template changes needed. After enabling, root-relative paths in `src` and
`href` attributes are automatically:

1. Resolved against the source's origin (`https://cms.example.com`)
2. Downloaded with the source's auth headers
3. Saved to `dist/assets/`
4. Rewritten to local `/assets/...` paths
5. Optimized (WebP, AVIF, responsive sizes) like any other image

Templates continue to use `{{ entry.body | safe }}` — everything just works.

## How it works

When data is fetched from a source with `resolve_html_urls = true`:

1. All string values in the JSON response are scanned for `src="..."` and
   `href="..."` HTML attributes containing root-relative paths (starting
   with `/`)
2. Each path is resolved against the source's origin (scheme + host extracted
   from the source `url`). For example, source URL
   `https://cms.example.com/api/v1/apps/x` yields origin
   `https://cms.example.com`, and path `/api/v1/apps/x/uploads/photo.jpg`
   resolves to `https://cms.example.com/api/v1/apps/x/uploads/photo.jpg`
3. The resolved absolute URLs are fed into the source asset pipeline — the
   same system used by `source_asset()` — which downloads them with the
   source's auth headers
4. After rendering, the absolute URLs in the HTML are rewritten to local
   `/assets/...` paths

## What gets resolved

- `src` and `href` attributes with root-relative paths (starting with `/`)
- Both double-quoted and single-quoted attributes

## What is NOT touched

- Absolute URLs (`https://...`, `http://...`) — already handled by
  `localize_assets`
- Protocol-relative URLs (`//cdn.example.com/...`)
- Relative paths without leading slash (`images/photo.jpg`)
- Non-HTML string values — only attribute patterns are matched
- File-based data (`_data/*.json`) — only remote sources

## Comparison with `source_asset()`

| | `source_asset()` | `resolve_html_urls` |
|---|---|---|
| Scope | Single URL per call | All URLs in all data strings |
| Usage | Explicit template call | Automatic on fetch |
| Best for | Individual image fields | Rich text / HTML content |
| Auth | Yes | Yes |
| Dev proxy | Yes | No (resolved at fetch time) |
