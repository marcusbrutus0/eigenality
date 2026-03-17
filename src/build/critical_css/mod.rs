//! Critical CSS inlining: extract per-page used CSS and inline it to
//! eliminate render-blocking stylesheets.

pub mod extract;
pub mod rewrite;
