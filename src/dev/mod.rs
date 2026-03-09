//! Phase 7: Dev Server — Axum static file server, CMS proxy, live reload
//! via SSE, file watcher with debounce, and smart rebuild with cached data.
//!
//! The `dev` command:
//! 1. Performs an initial full build (with live-reload script injection).
//! 2. Starts a file watcher on `templates/`, `_data/`, `static/`, `site.toml`.
//! 3. Starts an Axum HTTP server that serves `dist/`, provides a `/_reload`
//!    SSE endpoint for live reload, and proxies `/_proxy/{source}/*` to
//!    configured external API sources.

pub mod inject;
mod proxy;
mod rebuild;
mod server;
mod watcher;

pub use server::dev_command;
