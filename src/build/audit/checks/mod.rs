//! Audit check modules.
//!
//! Each sub-module exposes a `check(html, url_path) -> Vec<Finding>`
//! function that inspects rendered HTML for issues in its category.

pub mod accessibility;
pub mod best_practices;
pub mod performance;
pub mod seo;
