//! Authenticated asset downloads from configured data sources.
//!
//! During template rendering, `source_asset()` collects download requests.
//! After rendering, `resolve_source_assets()` downloads each asset with the
//! source's auth headers and rewrites URLs in the rendered HTML.

use std::sync::{Arc, Mutex};

/// A request to download an asset using a named source's auth headers.
#[derive(Debug, Clone)]
pub struct SourceAssetRequest {
    /// Source name (key in `[sources.*]`).
    pub source_name: String,
    /// Fully resolved absolute URL.
    pub url: String,
}

/// Thread-safe collector for source asset requests during rendering.
///
/// Shared via `Arc` between the template function closure and the caller.
#[derive(Debug, Clone, Default)]
pub struct SourceAssetCollector {
    requests: Arc<Mutex<Vec<SourceAssetRequest>>>,
}

impl SourceAssetCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a request. Called from the `source_asset` template function.
    pub fn push(&self, source_name: String, url: String) {
        let mut reqs = self.requests.lock().expect("collector lock poisoned");
        reqs.push(SourceAssetRequest { source_name, url });
    }

    /// Drain all collected requests.
    pub fn drain(&self) -> Vec<SourceAssetRequest> {
        let mut reqs = self.requests.lock().expect("collector lock poisoned");
        std::mem::take(&mut *reqs)
    }
}

/// Resolve a URL or path against a source's base URL.
///
/// - Absolute URLs (`https://...`) pass through unchanged.
/// - Relative paths (`/uploads/foo` or `uploads/foo`) are joined with the base.
pub fn resolve_url(url_or_path: &str, base_url: &str) -> String {
    if url_or_path.starts_with("http://") || url_or_path.starts_with("https://") {
        return url_or_path.to_string();
    }

    let base = base_url.trim_end_matches('/');
    if url_or_path.starts_with('/') {
        format!("{}{}", base, url_or_path)
    } else {
        format!("{}/{}", base, url_or_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_push_and_drain() {
        let collector = SourceAssetCollector::new();
        collector.push("cms".into(), "https://cms.example.com/img.jpg".into());
        collector.push("cms".into(), "https://cms.example.com/img2.jpg".into());

        let requests = collector.drain();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].source_name, "cms");
        assert_eq!(requests[0].url, "https://cms.example.com/img.jpg");

        // drain returns empty after first call
        assert!(collector.drain().is_empty());
    }

    #[test]
    fn collector_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SourceAssetCollector>();
    }

    #[test]
    fn resolve_url_absolute_passthrough() {
        let result = resolve_url("https://cdn.example.com/img.jpg", "https://cms.example.com");
        assert_eq!(result, "https://cdn.example.com/img.jpg");
    }

    #[test]
    fn resolve_url_relative_with_leading_slash() {
        let result = resolve_url("/uploads/photo.jpg", "https://cms.example.com");
        assert_eq!(result, "https://cms.example.com/uploads/photo.jpg");
    }

    #[test]
    fn resolve_url_relative_without_leading_slash() {
        let result = resolve_url("uploads/photo.jpg", "https://cms.example.com");
        assert_eq!(result, "https://cms.example.com/uploads/photo.jpg");
    }

    #[test]
    fn resolve_url_base_with_trailing_slash() {
        let result = resolve_url("/img.jpg", "https://cms.example.com/");
        assert_eq!(result, "https://cms.example.com/img.jpg");
    }
}
