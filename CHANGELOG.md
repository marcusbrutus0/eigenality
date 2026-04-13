# Changelog

## v0.14.0

### New Features

- **Project Website**: Eigen now has its own documentation website, built with eigen itself. Includes a landing page, quickstart tutorial, docs browser with sidebar navigation, and a custom 404 page — all styled in warm gruvbox tones with the Recursive font.

- **Environment Variable Escape Convention**: Use `$${...}` to write a literal `${...}` in your templates. This prevents eigen from substituting environment variables in strings you want kept verbatim (e.g., shell snippets in documentation).

- **Focused Template Error Context**: When a template render fails, eigen now walks the expression path to pinpoint where the undefined variable broke, highlights the relevant template line, and falls back to a full context dump when the focused view isn't enough. Much easier to debug complex template errors.

### Improvements

- Flash-free HTMX navigation on the website via `hx-boost`
- CI now runs tests on every push to master

### Bug Fixes

- Fixed CI toolchain pinning to match `rust-toolchain.toml`

## v0.13.1

### New Features

- **Umami Analytics Support**: Track page views with [Umami](https://umami.is) alongside or instead of Google Analytics. Configure under `[analytics.umami]` in `site.toml` with your website ID and optional settings for host URL, domain filtering, auto-tracking, and event tags.

- **Multi-Provider Analytics**: Run multiple analytics providers simultaneously. Both Google Analytics and Umami snippets are injected automatically into every full page.

- **setup-eigen GitHub Action**: New composite action to install eigen in CI pipelines via `cargo-dist`. Supports version pinning, custom build arguments, and optional `eigen build` step.

### Improvements

- **Analytics Config Validation**: Empty `tracking_id`, `website_id`, and `host_url` values are now caught at config load time with clear error messages instead of producing broken snippets.

- **Smarter CSP Warning**: The Content Security Policy warning for inline analytics scripts now only fires when a provider is actually configured, not when an empty `[analytics]` table exists.

### Bug Fixes

- Fixed script injection vulnerability in the setup-eigen GitHub Action by passing build args via environment variables instead of inline interpolation.

- Fixed composite action `if` condition syntax in the setup-eigen action.

### Breaking Changes

- **Analytics config format changed.** The flat `[analytics] tracking_id = "..."` format is removed. Use `[analytics.google] tracking_id = "..."` instead. Umami is configured under `[analytics.umami]`.
