//! CSS/JS bundling and tree-shaking.
//!
//! Merges multiple CSS/JS files into single bundles, tree-shakes unused
//! CSS selectors, and rewrites HTML references. Runs as Phase 2.5 in the
//! build pipeline (after rendering, before content hashing).

pub mod collect;
pub mod css;
pub mod js;
pub mod rewrite;

use std::path::Path;

use eyre::Result;

use crate::config::BundlingConfig;
