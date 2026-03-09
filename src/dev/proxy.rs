//! Step 7.2: CMS proxy.
//!
//! For each `[sources.*]` in the site config, we mount a reverse proxy route
//! at `/_proxy/{source_name}/*rest`. Requests are forwarded to the source's
//! base URL with the configured headers injected. This lets frontend JS call
//! `/_proxy/blog_api/posts` during development without CORS issues.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::config::SourceConfig;

/// Shared state for the proxy handler.
#[derive(Clone)]
pub struct ProxyState {
    /// The source configuration (base URL + headers).
    pub source: SourceConfig,
    /// A reusable reqwest client.
    pub client: reqwest::Client,
}

/// Handler for `/_proxy/{source_name}/*rest`.
///
/// Forwards the request to the source's base URL, injecting configured
/// headers (e.g. auth tokens). Returns the raw response to the caller.
pub async fn proxy_handler(
    State(state): State<ProxyState>,
    Path(rest): Path<String>,
    headers: HeaderMap,
) -> Response {
    let base = state.source.url.trim_end_matches('/');
    let rest_path = if rest.starts_with('/') {
        rest.clone()
    } else {
        format!("/{}", rest)
    };
    let url = format!("{}{}", base, rest_path);

    let mut req = state.client.get(&url);

    // Inject source-configured headers.
    for (key, val) in &state.source.headers {
        req = req.header(key.as_str(), val.as_str());
    }

    // Forward selected request headers from the client.
    for (name, value) in headers.iter() {
        // Skip hop-by-hop headers and host.
        let name_str = name.as_str().to_lowercase();
        if matches!(
            name_str.as_str(),
            "host" | "connection" | "transfer-encoding" | "keep-alive" | "proxy-connection"
        ) {
            continue;
        }
        req = req.header(name.clone(), value.clone());
    }

    match req.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);

            let mut response_headers = HeaderMap::new();
            for (key, value) in resp.headers() {
                // Skip hop-by-hop headers.
                let name_str = key.as_str().to_lowercase();
                if matches!(
                    name_str.as_str(),
                    "transfer-encoding" | "connection" | "keep-alive"
                ) {
                    continue;
                }
                if let (Ok(name), Ok(val)) = (
                    HeaderName::from_bytes(key.as_str().as_bytes()),
                    HeaderValue::from_bytes(value.as_bytes()),
                ) {
                    response_headers.insert(name, val);
                }
            }

            // Add CORS headers for dev.
            response_headers.insert(
                HeaderName::from_static("access-control-allow-origin"),
                HeaderValue::from_static("*"),
            );

            let body_bytes = resp.bytes().await.unwrap_or_default();

            let mut response = Response::new(Body::from(body_bytes));
            *response.status_mut() = status;
            *response.headers_mut() = response_headers;
            response
        }
        Err(e) => {
            eprintln!("Proxy error for {}: {}", url, e);
            (
                StatusCode::BAD_GATEWAY,
                format!("Proxy error: {}", e),
            )
                .into_response()
        }
    }
}
