# Changelog

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
