//! Build engine: output setup, context assembly, page rendering, fragment
//! extraction, and sitemap generation.

pub mod bundling;
pub mod content_hash;
pub mod context;
pub mod critical_css;
pub mod fragments;
pub mod hints;
pub mod minify;
pub mod output;
pub mod render;
pub mod sitemap;

pub use render::build;
