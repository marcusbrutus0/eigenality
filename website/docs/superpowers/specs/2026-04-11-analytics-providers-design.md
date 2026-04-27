# Analytics Providers Design

## Overview

Replace the flat `[analytics] tracking_id` config with a provider-based `[analytics.{provider}]` structure. Add Umami as a second analytics provider alongside the existing Google Analytics support. All pages get tracking snippets injected automatically before `</body>` — fragments are unaffected.

This is a **breaking change** to the config format. The old `[analytics] tracking_id` form is removed.

## Config Format

Both providers are optional. Use one, both, or neither.

```toml
[analytics.google]
tracking_id = "G-XXXXXXXXXX"

[analytics.umami]
website_id = "abc-123-def"
host_url = "https://analytics.example.com"  # default: "https://cloud.umami.is"
domains = "example.com,www.example.com"      # optional, restrict tracking to listed domains
auto_track = true                             # optional, default: true
tag = "production"                            # optional, custom event tag
```

When `[analytics]` is absent or both sub-tables are absent, no snippets are injected.

## Config Structs

Replace the current `AnalyticsConfig { tracking_id: String }` with:

```rust
pub struct AnalyticsConfig {
    pub google: Option<GoogleAnalyticsConfig>,
    pub umami: Option<UmamiAnalyticsConfig>,
}

pub struct GoogleAnalyticsConfig {
    pub tracking_id: String,
}

pub struct UmamiAnalyticsConfig {
    pub website_id: String,
    #[serde(default = "default_umami_host")]
    pub host_url: String,
    pub domains: Option<String>,
    #[serde(default = "default_true")]
    pub auto_track: bool,
    pub tag: Option<String>,
}
```

`default_umami_host` returns `"https://cloud.umami.is"`.

## Snippet Generation

### Google Analytics (existing, renamed)

```html
<script async src="https://www.googletagmanager.com/gtag/js?id={tracking_id}"></script>
<script>
  window.dataLayer = window.dataLayer || [];
  function gtag(){dataLayer.push(arguments);}
  gtag('js', new Date());
  gtag('config', '{tracking_id}');
</script>
```

### Umami (new)

```html
<script defer src="{host_url}/script.js"
  data-website-id="{website_id}"></script>
```

Optional `data-*` attributes are added only when configured:
- `data-domains="{domains}"` — when `domains` is set
- `data-auto-track="false"` — only when `auto_track` is `false` (true is Umami's default, so the attribute is omitted)
- `data-tag="{tag}"` — when `tag` is set

## Injection

`inject_analytics` changes its signature from `(html: &str, tracking_id: &str) -> String` to `(html: &str, config: &AnalyticsConfig) -> String`.

It collects all enabled provider snippets into one combined string and injects once before `</body>`. Order: Google first, then Umami. If no `</body>` tag exists, the combined snippet is appended at the end.

The call site in `src/build/render.rs` passes `&AnalyticsConfig` instead of `&analytics.tracking_id`.

## Validation

Add `validate_analytics_config` to the validation chain in `validate_config`. Checks:
- If `analytics.google` is present: `tracking_id` must be non-empty.
- If `analytics.umami` is present: `website_id` must be non-empty.

No other cross-field validation is needed — the fields are independent.

## Files Changed

| File | Change |
|------|--------|
| `src/config/mod.rs` | Replace `AnalyticsConfig` with nested structs, add `default_umami_host`, add `validate_analytics_config` |
| `src/build/analytics.rs` | Rename `build_snippet` to `build_google_snippet`, add `build_umami_snippet`, update `inject_analytics` signature |
| `src/build/render.rs` | Update `inject_analytics` call to pass `&AnalyticsConfig` |
| `src/build/context.rs` | Update test fixtures if they reference old `AnalyticsConfig` |
| `src/build/robots.rs` | Update test fixtures if they reference old `AnalyticsConfig` |
| `src/build/sitemap.rs` | Update test fixtures if they reference old `AnalyticsConfig` |
| `src/build/redirects.rs` | Update test fixtures if they reference old `AnalyticsConfig` |
| `docs/analytics.md` | New feature documentation |

## Testing

Unit tests in `src/build/analytics.rs`:
- `build_google_snippet` — existing tests adapted to new function name
- `build_umami_snippet` — minimal config (id + default host), full config (all options), optional attributes omitted when unset, `auto_track=false` emits attribute
- `inject_analytics` — Google only, Umami only, both providers, neither provider, case-insensitive `</body>`, no `</body>` fallback

Config parsing tests in `src/config/mod.rs`:
- Parse new `[analytics.google]` and `[analytics.umami]` tables
- Umami defaults (`host_url`, `auto_track`)
- Validation rejects empty `tracking_id` and `website_id`
- Missing `[analytics]` section results in `None`
