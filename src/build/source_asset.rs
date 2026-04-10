//! Authenticated asset downloads from configured data sources.
//!
//! During template rendering, `source_asset()` collects download requests.
//! After rendering, `resolve_source_assets()` downloads each asset with the
//! source's auth headers and rewrites URLs in the rendered HTML.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use eyre::{Result, WrapErr};

use crate::assets::cache::AssetCache;
use crate::assets::download::{self, DownloadResult};
use crate::build::rate_limit::RateLimiterPool;
use crate::config::SourceConfig;

/// Sentinel prefix used by the dev proxy to identify full-URL forwarding requests.
/// Includes trailing slash for direct use in URL construction.
pub const SOURCE_ASSET_PROXY_PREFIX: &str = "__source_asset__/";

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

/// Download collected source assets and rewrite their URLs in the HTML.
///
/// Called after `localize_assets` in the render pipeline. Each request is
/// downloaded with the named source's configured headers (auth, API keys).
pub async fn resolve_source_assets(
    html: &str,
    requests: &[SourceAssetRequest],
    sources: &HashMap<String, SourceConfig>,
    cache: &mut AssetCache,
    client: &reqwest::Client,
    dist_dir: &Path,
    pool: &RateLimiterPool,
) -> Result<String> {
    if requests.is_empty() {
        return Ok(html.to_string());
    }

    let dist_assets_dir = dist_dir.join("assets");

    let mut seen: HashMap<&str, &str> = HashMap::new();
    for req in requests {
        seen.entry(req.url.as_str())
            .or_insert(req.source_name.as_str());
    }

    let mut url_map: HashMap<&str, String> = HashMap::new();

    for (&url, &source_name) in &seen {
        let source = match sources.get(source_name) {
            Some(s) => s,
            None => {
                tracing::warn!(
                    "source_asset: source '{}' not found, skipping URL: {}",
                    source_name, url
                );
                continue;
            }
        };

        // Build header map from source config.
        let mut headers = reqwest::header::HeaderMap::new();
        for (key, val) in &source.headers {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(val),
            ) {
                headers.insert(name, value);
            }
        }

        match download::ensure_asset_with_headers(client, cache, url, &headers, pool).await {
            Ok(local_filename) => {
                cache.copy_to_dist(url, &dist_assets_dir)
                    .wrap_err_with(|| {
                        format!("Failed to copy source asset to dist: {}", url)
                    })?;
                url_map.insert(url, format!("/assets/{}", local_filename));
            }
            Err(e) => {
                tracing::warn!("Failed to download source asset {}: {:#}", url, e);
            }
        }
    }

    if url_map.is_empty() {
        return Ok(html.to_string());
    }

    // Rewrite URLs in the HTML, longest-first to avoid partial-match corruption.
    let mut sorted: Vec<_> = url_map.into_iter().collect();
    sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut result = html.to_string();
    for (original, local) in &sorted {
        result = result.replace(original, local);
    }
    Ok(result)
}

/// Phase 1: identify which source-asset requests need downloading.
///
/// Deduplicates by URL.  Returns `(to_download, already_cached)`:
/// - `to_download`: unique requests where the file is not on disk yet
/// - `already_cached`: map of URL → local `/assets/…` path for files already present
///
/// The caller should release any `AssetCache` mutex after this returns, then
/// call `download_source_assets` without holding the lock.
pub fn check_source_asset_cache(
    cache: &AssetCache,
    requests: &[SourceAssetRequest],
) -> (Vec<SourceAssetRequest>, HashMap<String, String>) {
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for req in requests {
        seen.entry(req.url.as_str()).or_insert(req.source_name.as_str());
    }

    let mut to_download: Vec<SourceAssetRequest> = Vec::new();
    let mut already_cached: HashMap<String, String> = HashMap::new();

    for (&url, &source_name) in &seen {
        if cache.has_file(url) {
            if let Some(meta) = cache.get(url) {
                already_cached.insert(
                    url.to_string(),
                    format!("/assets/{}", meta.local_filename),
                );
            }
        } else {
            to_download.push(SourceAssetRequest {
                source_name: source_name.to_string(),
                url: url.to_string(),
            });
        }
    }

    (to_download, already_cached)
}

/// Phase 2: download source assets that are not in the cache. No lock needed.
///
/// Returns `(url, DownloadResult)` for each successfully attempted download.
/// Auth headers are built per-source from `sources`.  Failed downloads are
/// logged as warnings and omitted from the result.
pub async fn download_source_assets(
    client: &reqwest::Client,
    sources: &HashMap<String, SourceConfig>,
    to_download: Vec<SourceAssetRequest>,
    pool: &RateLimiterPool,
) -> Vec<(String, DownloadResult)> {
    let mut results = Vec::with_capacity(to_download.len());
    for req in to_download {
        let source = match sources.get(&req.source_name) {
            Some(s) => s,
            None => {
                tracing::warn!(
                    "source_asset: source '{}' not found, skipping URL: {}",
                    req.source_name, req.url
                );
                continue;
            }
        };

        let mut headers = reqwest::header::HeaderMap::new();
        for (key, val) in &source.headers {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(val),
            ) {
                headers.insert(name, value);
            }
        }

        // Pass cached_meta=None: we only reach here on a cache miss.
        match download::download_asset_with_headers(
            client,
            &req.url,
            None,
            &headers,
            pool,
        )
        .await
        {
            Ok(result) => results.push((req.url, result)),
            Err(e) => tracing::warn!("Failed to download source asset {}: {:#}", req.url, e),
        }
    }
    results
}

/// Phase 3: store downloaded source assets in the cache and rewrite HTML.
///
/// Reacquire the `AssetCache` lock before calling this.
pub async fn store_source_assets_and_rewrite(
    html: &str,
    cache: &mut AssetCache,
    download_results: Vec<(String, DownloadResult)>,
    already_cached: HashMap<String, String>,
    dist_assets_dir: &Path,
) -> Result<String> {
    let mut url_map = already_cached;

    // Ensure already-cached assets are present in dist.
    for url in url_map.keys() {
        if let Err(e) = cache.copy_to_dist(url, dist_assets_dir) {
            tracing::warn!("Failed to copy cached source asset to dist {}: {:#}", url, e);
        }
    }

    for (url, result) in download_results {
        match result {
            DownloadResult::Downloaded {
                data,
                local_filename,
                etag,
                last_modified,
                content_type,
            } => {
                match cache.store(&url, &data, &local_filename, etag, last_modified, content_type) {
                    Ok(final_name) => {
                        cache.copy_to_dist(&url, dist_assets_dir)
                            .wrap_err_with(|| {
                                format!("Failed to copy source asset to dist: {}", url)
                            })?;
                        url_map.insert(url, format!("/assets/{}", final_name));
                    }
                    Err(e) => tracing::warn!("Failed to store source asset {}: {:#}", url, e),
                }
            }
            DownloadResult::NotModified => {
                if let Some(meta) = cache.get(&url).cloned() {
                    cache.copy_to_dist(&url, dist_assets_dir)
                        .wrap_err_with(|| {
                            format!("Failed to copy not-modified source asset to dist: {}", url)
                        })?;
                    url_map.insert(url, format!("/assets/{}", meta.local_filename));
                }
            }
        }
    }

    if url_map.is_empty() {
        return Ok(html.to_string());
    }

    // Rewrite URLs in HTML, longest-first to avoid partial-match corruption.
    let mut sorted: Vec<_> = url_map.into_iter().collect();
    sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let mut result = html.to_string();
    for (original, local) in &sorted {
        result = result.replace(original.as_str(), local.as_str());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::rate_limit::RateLimiterPool;

    fn no_op_pool() -> RateLimiterPool {
        RateLimiterPool::new(None, &HashMap::new())
    }

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

    /// Start a mock server that requires `Authorization: Bearer secret`,
    /// download the asset via `resolve_source_assets`, and verify the URL
    /// is rewritten and the file lands in `dist/assets/`.
    #[tokio::test]
    async fn resolve_source_assets_downloads_and_rewrites() {
        use std::thread;

        let server =
            tiny_http::Server::http("127.0.0.1:0").expect("failed to bind mock server");
        let addr = server.server_addr().to_ip().expect("no IP address");
        let asset_url = format!("http://{}/photo.png", addr);

        // Serve one request on a background thread.
        let handle = thread::spawn(move || {
            if let Ok(Some(req)) = server.recv_timeout(std::time::Duration::from_secs(5)) {
                let auth = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.as_str().to_string());

                let (status, body): (u16, &[u8]) = if auth.as_deref() == Some("Bearer secret") {
                    (200, b"fake-png-bytes")
                } else {
                    (401, b"unauthorized")
                };

                let content_type = tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"image/png"[..],
                ).expect("valid header");

                let response = tiny_http::Response::new(
                    tiny_http::StatusCode(status),
                    vec![content_type],
                    std::io::Cursor::new(body),
                    Some(body.len()),
                    None,
                );
                let _ = req.respond(response);
            }
        });

        let tmp = tempfile::TempDir::new().expect("tempdir");
        let project_root = tmp.path();
        let dist_dir = project_root.join("dist");
        std::fs::create_dir_all(&dist_dir).expect("create dist");

        let mut cache = AssetCache::open(project_root).expect("asset cache");
        let client = reqwest::Client::new();

        let mut sources = HashMap::new();
        sources.insert(
            "cms".to_string(),
            SourceConfig {
                url: format!("http://{}", addr),
                headers: {
                    let mut h = HashMap::new();
                    h.insert("Authorization".into(), "Bearer secret".into());
                    h
                },
                rate_limit: None,
            },
        );

        let html = format!(
            r#"<html><body><img src="{}"></body></html>"#,
            asset_url,
        );

        let requests = vec![SourceAssetRequest {
            source_name: "cms".into(),
            url: asset_url.clone(),
        }];

        let pool = no_op_pool();
        let result = resolve_source_assets(
            &html,
            &requests,
            &sources,
            &mut cache,
            &client,
            &dist_dir,
            &pool,
        )
        .await
        .expect("resolve should succeed");

        handle.join().expect("server thread");

        // URL should be rewritten to a local /assets/ path.
        assert!(
            !result.contains(&asset_url),
            "original URL should be replaced"
        );
        assert!(
            result.contains("/assets/"),
            "result should contain local asset path"
        );

        // The file should exist in dist/assets/.
        let dist_assets = dist_dir.join("assets");
        let entries: Vec<_> = std::fs::read_dir(&dist_assets)
            .expect("read dist/assets")
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1, "should have exactly one downloaded asset");

        let file_content = std::fs::read(entries[0].path()).expect("read asset file");
        assert_eq!(file_content, b"fake-png-bytes");
    }

    #[tokio::test]
    async fn resolve_source_assets_empty_requests_returns_html_unchanged() {
        let html = "<html><body>hello</body></html>";
        let sources = HashMap::new();
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let mut cache = AssetCache::open(tmp.path()).expect("asset cache");
        let client = reqwest::Client::new();
        let dist_dir = tmp.path().join("dist");

        let pool = no_op_pool();
        let result = resolve_source_assets(
            html,
            &[],
            &sources,
            &mut cache,
            &client,
            &dist_dir,
            &pool,
        )
        .await
        .expect("should succeed");

        assert_eq!(result, html);
    }

    #[tokio::test]
    async fn resolve_source_assets_deduplicates_urls() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::thread;

        let request_count = Arc::new(AtomicUsize::new(0));
        let count_clone = request_count.clone();

        let server =
            tiny_http::Server::http("127.0.0.1:0").expect("failed to bind mock server");
        let addr = server.server_addr().to_ip().expect("no IP address");
        let asset_url = format!("http://{}/image.png", addr);

        // Serve requests on a background thread — count how many arrive.
        let handle = thread::spawn(move || {
            // Only expect 1 request due to deduplication.
            while let Ok(Some(req)) = server.recv_timeout(std::time::Duration::from_secs(2)) {
                count_clone.fetch_add(1, Ordering::SeqCst);
                let body: &[u8] = b"img-data";
                let ct = tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"image/png"[..],
                ).expect("valid header");
                let response = tiny_http::Response::new(
                    tiny_http::StatusCode(200),
                    vec![ct],
                    std::io::Cursor::new(body),
                    Some(body.len()),
                    None,
                );
                let _ = req.respond(response);
            }
        });

        let tmp = tempfile::TempDir::new().expect("tempdir");
        let dist_dir = tmp.path().join("dist");
        std::fs::create_dir_all(&dist_dir).expect("create dist");

        let mut cache = AssetCache::open(tmp.path()).expect("asset cache");
        let client = reqwest::Client::new();

        let mut sources = HashMap::new();
        sources.insert(
            "api".to_string(),
            SourceConfig {
                url: format!("http://{}", addr),
                headers: HashMap::new(),
                rate_limit: None,
            },
        );

        let html = format!(
            r#"<img src="{0}"><img src="{0}">"#,
            asset_url,
        );

        // Two requests for the same URL.
        let requests = vec![
            SourceAssetRequest { source_name: "api".into(), url: asset_url.clone() },
            SourceAssetRequest { source_name: "api".into(), url: asset_url.clone() },
        ];

        let pool = no_op_pool();
        let result = resolve_source_assets(
            &html,
            &requests,
            &sources,
            &mut cache,
            &client,
            &dist_dir,
            &pool,
        )
        .await
        .expect("resolve should succeed");

        handle.join().expect("server thread");

        // Only 1 HTTP request should have been made.
        assert_eq!(request_count.load(Ordering::SeqCst), 1, "should deduplicate");

        // Both occurrences should be rewritten.
        assert!(!result.contains(&asset_url), "all URLs should be replaced");
    }

    // --- check_source_asset_cache tests ---

    #[test]
    fn check_source_asset_cache_returns_miss_when_not_on_disk() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let cache = crate::assets::cache::AssetCache::open(tmp.path()).expect("cache");
        let requests = vec![SourceAssetRequest {
            source_name: "cms".into(),
            url: "https://cms.example.com/img.jpg".into(),
        }];

        let (to_download, already_cached) = check_source_asset_cache(&cache, &requests);

        assert_eq!(to_download.len(), 1);
        assert_eq!(to_download[0].url, "https://cms.example.com/img.jpg");
        assert!(already_cached.is_empty());
    }

    #[test]
    fn check_source_asset_cache_deduplicates_duplicate_requests() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let cache = crate::assets::cache::AssetCache::open(tmp.path()).expect("cache");
        let url = "https://cms.example.com/banner.png";
        let requests = vec![
            SourceAssetRequest { source_name: "cms".into(), url: url.into() },
            SourceAssetRequest { source_name: "cms".into(), url: url.into() },
        ];

        let (to_download, _) = check_source_asset_cache(&cache, &requests);

        // Duplicates should be merged into one download request.
        assert_eq!(to_download.len(), 1);
    }

    #[test]
    fn check_source_asset_cache_returns_empty_for_empty_requests() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let cache = crate::assets::cache::AssetCache::open(tmp.path()).expect("cache");

        let (to_download, already_cached) = check_source_asset_cache(&cache, &[]);

        assert!(to_download.is_empty());
        assert!(already_cached.is_empty());
    }
}
