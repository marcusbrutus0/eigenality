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
pub mod download;
pub mod html_rewrite;
pub mod images;
mod rewrite;

pub use html_rewrite::optimize_and_rewrite_images;
pub use html_rewrite::rewrite_css_background_images;
pub use rewrite::localize_assets;
