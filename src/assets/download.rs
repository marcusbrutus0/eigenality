//! HTTP-aware asset downloading with conditional request support.
//!
//! Sends `If-None-Match` / `If-Modified-Since` headers when we have cached
//! metadata, and respects 304 Not Modified responses.

use eyre::{Result, WrapErr, bail};

use super::cache::{AssetCache, AssetCacheMeta, local_filename_for_url};
use crate::build::rate_limit::RateLimiterPool;

/// Result of attempting to download an asset.
pub enum DownloadResult {
    /// New data was downloaded.
    Downloaded {
        data: Vec<u8>,
        local_filename: String,
        etag: Option<String>,
        last_modified: Option<String>,
        content_type: Option<String>,
    },
    /// Server returned 304 — cached copy is still valid.
    NotModified,
}

/// Download an asset URL with extra headers, using conditional requests if we have cached metadata.
///
/// Merges `extra_headers` into the request before sending.
/// Use this when the source requires authentication (e.g. a Bearer token).
pub async fn download_asset_with_headers(
    client: &reqwest::Client,
    url: &str,
    cached_meta: Option<&AssetCacheMeta>,
    extra_headers: &reqwest::header::HeaderMap,
    pool: &RateLimiterPool,
) -> Result<DownloadResult> {
    pool.wait(url).await;
    let mut request = client.get(url);

    // Merge caller-supplied headers first so conditional headers below take precedence.
    for (name, value) in extra_headers {
        request = request.header(name, value);
    }

    // Add conditional headers if we have cached metadata.
    if let Some(meta) = cached_meta {
        if let Some(ref etag) = meta.etag {
            request = request.header("If-None-Match", etag.as_str());
        }
        if let Some(ref last_mod) = meta.last_modified {
            request = request.header("If-Modified-Since", last_mod.as_str());
        }
    }

    let response = request
        .send()
        .await
        .wrap_err_with(|| format!("Failed to download asset: {}", url))?;

    let status = response.status();

    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(DownloadResult::NotModified);
    }

    if !status.is_success() {
        bail!("HTTP {} downloading asset: {}", status, url);
    }

    // Extract caching headers.
    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let last_modified = response
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());

    let data = response
        .bytes()
        .await
        .wrap_err_with(|| format!("Failed to read asset body: {}", url))?
        .to_vec();

    let local_filename = local_filename_for_url(url);

    Ok(DownloadResult::Downloaded {
        data,
        local_filename,
        etag,
        last_modified,
        content_type,
    })
}

/// Download an asset URL, using conditional requests if we have cached metadata.
#[allow(dead_code)]
pub async fn download_asset(
    client: &reqwest::Client,
    url: &str,
    cached_meta: Option<&AssetCacheMeta>,
    pool: &RateLimiterPool,
) -> Result<DownloadResult> {
    download_asset_with_headers(client, url, cached_meta, &reqwest::header::HeaderMap::new(), pool).await
}

/// Ensure an asset is available in the cache (downloading or validating as needed),
/// sending `extra_headers` on every request (e.g. for source authentication).
///
/// Returns the local filename on success.
pub async fn ensure_asset_with_headers(
    client: &reqwest::Client,
    cache: &mut AssetCache,
    url: &str,
    extra_headers: &reqwest::header::HeaderMap,
    pool: &RateLimiterPool,
) -> Result<String> {
    let cached_meta = cache.get(url).cloned();

    // If we have the file cached, try a conditional request.
    if let Some(ref meta) = cached_meta {
        if cache.has_file(url) {
            match download_asset_with_headers(client, url, Some(meta), extra_headers, pool).await? {
                DownloadResult::NotModified => {
                    tracing::debug!("  Source asset not modified (304): {}", url);
                    return Ok(meta.local_filename.clone());
                }
                DownloadResult::Downloaded {
                    data,
                    local_filename,
                    etag,
                    last_modified,
                    content_type,
                } => {
                    tracing::debug!("  Source asset re-downloaded: {}", url);
                    let final_name =
                        cache.store(url, &data, &local_filename, etag, last_modified, content_type)?;
                    return Ok(final_name);
                }
            }
        }
    }

    // No cache entry or file missing — full download.
    match download_asset_with_headers(client, url, None, extra_headers, pool).await? {
        DownloadResult::Downloaded {
            data,
            local_filename,
            etag,
            last_modified,
            content_type,
        } => {
            tracing::debug!("  Source asset downloaded: {} → {}", url, local_filename);
            let final_name =
                cache.store(url, &data, &local_filename, etag, last_modified, content_type)?;
            Ok(final_name)
        }
        DownloadResult::NotModified => {
            bail!("Unexpected 304 for {}", url);
        }
    }
}

/// Ensure an asset is available in the cache (downloading or validating as needed).
///
/// Returns the local filename on success.
pub async fn ensure_asset(
    client: &reqwest::Client,
    cache: &mut AssetCache,
    url: &str,
    pool: &RateLimiterPool,
) -> Result<String> {
    ensure_asset_with_headers(client, cache, url, &reqwest::header::HeaderMap::new(), pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn no_op_pool() -> RateLimiterPool {
        RateLimiterPool::new(None, &HashMap::new())
    }

    /// Starts a tiny_http server that requires `Authorization: Bearer secret`.
    /// Returns 401 when the header is absent or wrong, 200 with a small body when correct.
    /// Verifies that `download_asset_with_headers` sends the header and receives the body.
    #[tokio::test]
    async fn download_asset_with_headers_sends_auth_header() {
        use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
        use std::thread;

        let server =
            tiny_http::Server::http("127.0.0.1:0").expect("failed to bind mock server");
        let addr = server.server_addr().to_ip().expect("no IP address");
        let url = format!("http://{}/image.png", addr);

        // Serve one request on a background thread.
        thread::spawn(move || {
            if let Ok(Some(req)) = server.recv_timeout(std::time::Duration::from_secs(5)) {
                let auth = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Authorization"))
                    .map(|h| h.value.as_str());

                let (status_code, body): (u16, &[u8]) = if auth == Some("Bearer secret") {
                    (200, b"fake-image-data")
                } else {
                    (401, b"unauthorized")
                };

                let response = tiny_http::Response::new(
                    tiny_http::StatusCode(status_code),
                    vec![],
                    std::io::Cursor::new(body),
                    Some(body.len()),
                    None,
                );
                let _ = req.respond(response);
            }
        });

        let client = reqwest::Client::new();
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        let pool = no_op_pool();

        let result = download_asset_with_headers(&client, &url, None, &headers, &pool)
            .await
            .expect("download should succeed");

        match result {
            DownloadResult::Downloaded { data, .. } => {
                assert_eq!(data, b"fake-image-data");
            }
            DownloadResult::NotModified => panic!("expected Downloaded, got NotModified"),
        }
    }

    /// Verifies that `download_asset_with_headers` returns an error when the server
    /// rejects the request (missing/wrong auth header).
    #[tokio::test]
    async fn download_asset_with_headers_fails_without_auth() {
        use reqwest::header::HeaderMap;
        use std::thread;

        let server =
            tiny_http::Server::http("127.0.0.1:0").expect("failed to bind mock server");
        let addr = server.server_addr().to_ip().expect("no IP address");
        let url = format!("http://{}/image.png", addr);

        thread::spawn(move || {
            if let Ok(Some(req)) = server.recv_timeout(std::time::Duration::from_secs(5)) {
                let body: &[u8] = b"unauthorized";
                let response = tiny_http::Response::new(
                    tiny_http::StatusCode(401),
                    vec![],
                    std::io::Cursor::new(body),
                    Some(body.len()),
                    None,
                );
                let _ = req.respond(response);
            }
        });

        let client = reqwest::Client::new();
        let pool = no_op_pool();
        let result =
            download_asset_with_headers(&client, &url, None, &HeaderMap::new(), &pool).await;

        assert!(result.is_err(), "expected error for 401 response");
    }
}
