//! Asset localization: download remote images/videos/audio referenced in
//! rendered HTML, save them to `dist/assets/`, and rewrite `src` attributes
//! to point to the local copies.
//!
//! Supports:
//! - `<img>`, `<video>`, `<source>`, `<audio>` `src` attributes
//! - CSS `background-image: url(...)` in inline `style` attributes and
//!   `<style>` blocks
//!
//! Skips:
//! - Relative URLs (no scheme)
//! - URLs already under `/assets/`
//! - Known CDN hostnames (configurable via `site.toml`)

pub mod cache;
mod download;
mod rewrite;

pub use rewrite::localize_assets;
