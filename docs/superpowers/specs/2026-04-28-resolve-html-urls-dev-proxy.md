# Dev Mode Proxy Support for resolve_html_urls

## Problem

`resolve_html_urls` rewrites root-relative URLs in CMS HTML content to absolute URLs. In production, these are downloaded with auth headers and served locally. In dev mode, the browser receives absolute CMS URLs it cannot fetch (auth required), resulting in broken images and links.

The dev proxy at `/_proxy/{source}/...` already handles this for `source_asset()` template calls — it forwards requests to the CMS with injected auth headers. `resolve_html_urls` should use the same mechanism.

## Design

### Move proxy URL helpers to shared location

`build_proxy_url`, `extract_host`, and `extract_path` currently live in `src/template/functions.rs`. Move them to `src/build/source_asset.rs` (where `resolve_url` and `SOURCE_ASSET_PROXY_PREFIX` already live) and update `template/functions.rs` to import from there.

This eliminates duplication: both `source_asset()` and `resolve_html_urls` need the same proxy URL construction logic.

### Add `dev_mode: bool` to DataFetcher

Add a `dev_mode` field to `DataFetcher` and its constructor. Callers:
- `src/build/render.rs` passes `false` (production build)
- `src/dev/rebuild.rs` passes `true` (dev server)
- Test callers pass `false`

### Update resolve_html_urls_in_value

The resolver gains two new parameters: `dev_mode: bool` and `source_base_url: &str`.

When `dev_mode = true`:
- Resolve root-relative path to absolute URL (as now)
- Convert absolute URL to proxy URL via `build_proxy_url(source_name, &absolute_url, source_base_url)`
- Rewrite the HTML attribute to the proxy URL
- Skip collector push (no downloads in dev mode)

When `dev_mode = false` (production, unchanged):
- Resolve root-relative path to absolute URL
- Rewrite the HTML attribute to the absolute URL
- Push to collector for auth-aware download

### Thread through call sites

`fetcher.rs::fetch()` and `query.rs::fetch_unlocked()` pass `self.dev_mode` and the source's base URL to the resolver. The `html_url_ctx` tuple in `fetch_unlocked` gains a `dev_mode` field.

## File map

| File | Action | Change |
|------|--------|--------|
| `src/build/source_asset.rs` | Modify | Receive `build_proxy_url`, `extract_host`, `extract_path` from template/functions.rs |
| `src/template/functions.rs` | Modify | Import moved functions; remove local definitions |
| `src/data/fetcher.rs` | Modify | Add `dev_mode: bool` field + constructor param; pass to resolver |
| `src/data/html_urls.rs` | Modify | Accept `dev_mode` + `source_base_url`; use `build_proxy_url` when dev |
| `src/data/query.rs` | Modify | Pass `dev_mode` + base URL through `html_url_ctx` |
| `src/build/render.rs` | Modify | Pass `dev_mode: false` to DataFetcher |
| `src/dev/rebuild.rs` | Modify | Pass `dev_mode: true` to DataFetcher |
| `website/docs/resolve_html_urls.md` | Modify | Update "Dev proxy" row in comparison table |

## What doesn't change

- Dev proxy handler (`proxy.rs`) — already supports `__source_asset__` prefix
- `source_asset()` template function behavior — just imports from new location
- Production build behavior — identical to current

## Testing

- Unit tests in `html_urls.rs` for proxy URL rewriting (same-host and cross-host cases)
- Test in `fetcher.rs` that dev_mode produces proxy URLs instead of absolute URLs
- Existing `source_asset` dev mode tests continue to pass after the move
- Full test suite regression check
