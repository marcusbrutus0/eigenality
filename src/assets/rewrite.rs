//! HTML scanning and URL rewriting for asset localization.
//!
//! Scans rendered HTML for remote URLs in:
//! - `<img src="...">`, `<video src="...">`, `<source src="...">`, `<audio src="...">`
//! - CSS `background-image: url(...)` in `style` attributes and `<style>` blocks
//!
//! Downloads each remote asset (respecting CDN skip/allow lists), copies it to
//! `dist/assets/`, and rewrites the URL in the HTML.

use eyre::{Result, WrapErr};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::build::rate_limit::RateLimiterPool;
use crate::config::AssetsConfig;

use super::cache::AssetCache;
use super::download;

/// Default CDN hostnames that are skipped (not downloaded).
///
/// These serve libraries, fonts, and infrastructure assets that should
/// remain on CDNs for performance and caching reasons.
const DEFAULT_CDN_SKIP_HOSTS: &[&str] = &[
    "cdn.jsdelivr.net",
    "cdnjs.cloudflare.com",
    "unpkg.com",
    "fonts.googleapis.com",
    "fonts.gstatic.com",
    "ajax.googleapis.com",
    "stackpath.bootstrapcdn.com",
    "maxcdn.bootstrapcdn.com",
    "code.jquery.com",
    "cdn.tailwindcss.com",
    "ga.jspm.io",
    "esm.sh",
    "use.fontawesome.com",
    "kit.fontawesome.com",
    "cdn.fontawesome.com",
];

/// The main entry point: localize all remote assets in rendered HTML.
///
/// Returns the rewritten HTML string with remote URLs replaced by local
/// `/assets/...` paths.
pub async fn localize_assets(
    html: &str,
    config: &AssetsConfig,
    cache: &mut AssetCache,
    client: &reqwest::Client,
    dist_dir: &Path,
    skip_urls: &HashSet<String>,
    pool: &RateLimiterPool,
) -> Result<String> {
    if !config.localize {
        return Ok(html.to_string());
    }

    let dist_assets_dir = dist_dir.join("assets");

    // Collect all remote URLs from the HTML.
    let urls = extract_remote_urls(html);

    if urls.is_empty() {
        return Ok(html.to_string());
    }

    // Build the URL → local path mapping.
    let mut url_map: HashMap<String, String> = HashMap::new();

    for url in &urls {
        // Skip if already processed.
        if url_map.contains_key(url.as_str()) {
            continue;
        }

        // Skip non-HTTP URLs.
        if !url.starts_with("http://") && !url.starts_with("https://") {
            continue;
        }

        // Skip URLs already pointing to /assets/.
        if url.starts_with("/assets/") {
            continue;
        }

        // Skip URLs claimed by source_asset (downloaded with auth later).
        if skip_urls.contains(url.as_str()) {
            continue;
        }

        // Check CDN skip/allow lists.
        if should_skip_cdn(url, config) {
            tracing::debug!("  Skipping CDN URL: {}", url);
            continue;
        }

        // Download (or validate cache).
        match download::ensure_asset(client, cache, url, pool).await {
            Ok(local_filename) => {
                // Copy from cache to dist/assets/.
                cache.copy_to_dist(url, &dist_assets_dir)
                    .wrap_err_with(|| {
                        format!("Failed to copy cached asset to dist: {}", url)
                    })?;
                let local_path = format!("/assets/{}", local_filename);
                url_map.insert(url.clone(), local_path);
            }
            Err(e) => {
                tracing::warn!("Failed to download asset {}: {:#}", url, e);
                // Don't rewrite — leave the original URL.
            }
        }
    }

    if url_map.is_empty() {
        return Ok(html.to_string());
    }

    // Rewrite the HTML.
    let result = rewrite_urls(html, &url_map);
    Ok(result)
}

/// Extract all remote URLs from HTML that should be considered for localization.
///
/// Looks for:
/// 1. `src="..."` attributes on img, video, source, audio elements
/// 2. `background-image: url(...)` in style attributes and style blocks
fn extract_remote_urls(html: &str) -> Vec<String> {
    let mut urls = Vec::new();

    // 1. Match src attributes on target elements.
    //    Pattern: <(img|video|source|audio) ... src="URL" ...>
    //    We use a regex that finds src="..." within tags that start with one of our targets.
    let src_re = Regex::new(
        r#"(?is)<(?:img|video|source|audio)\b[^>]*?\bsrc\s*=\s*"([^"]+)""#
    ).unwrap();

    for cap in src_re.captures_iter(html) {
        let url = cap[1].to_string();
        if is_absolute_http_url(&url) {
            urls.push(url);
        }
    }

    // Also match single-quoted src attributes.
    let src_sq_re = Regex::new(
        r#"(?is)<(?:img|video|source|audio)\b[^>]*?\bsrc\s*=\s*'([^']+)'"#
    ).unwrap();

    for cap in src_sq_re.captures_iter(html) {
        let url = cap[1].to_string();
        if is_absolute_http_url(&url) {
            urls.push(url);
        }
    }

    // 2. Match background-image: url(...) anywhere in the HTML.
    let bg_re = Regex::new(
        r#"(?i)background-image\s*:\s*url\(\s*['"]?([^'")]+)['"]?\s*\)"#
    ).unwrap();

    for cap in bg_re.captures_iter(html) {
        let url = cap[1].trim().to_string();
        if is_absolute_http_url(&url) {
            urls.push(url);
        }
    }

    urls
}

/// Check if a URL is an absolute HTTP(S) URL.
fn is_absolute_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Determine whether a URL should be skipped based on CDN host lists.
fn should_skip_cdn(url: &str, config: &AssetsConfig) -> bool {
    let host = match extract_host(url) {
        Some(h) => h.to_lowercase(),
        None => return false,
    };

    // If the host is explicitly in the allow list, never skip it.
    for allowed in &config.cdn_allow_hosts {
        if host == allowed.to_lowercase() || host.ends_with(&format!(".{}", allowed.to_lowercase())) {
            return false;
        }
    }

    // Check the default skip list.
    for skip in DEFAULT_CDN_SKIP_HOSTS {
        if host == *skip || host.ends_with(&format!(".{}", skip)) {
            return true;
        }
    }

    // Check user-configured skip list.
    for skip in &config.cdn_skip_hosts {
        if host == skip.to_lowercase() || host.ends_with(&format!(".{}", skip.to_lowercase())) {
            return true;
        }
    }

    false
}

/// Extract the hostname from a URL.
fn extract_host(url: &str) -> Option<&str> {
    let after_scheme = url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    // Take everything up to the first `/`, `?`, `#`, or `:` (port).
    let end = after_scheme
        .find(|c: char| c == '/' || c == '?' || c == '#' || c == ':')
        .unwrap_or(after_scheme.len());

    Some(&after_scheme[..end])
}

/// Rewrite URLs in the HTML using the provided mapping.
fn rewrite_urls(html: &str, url_map: &HashMap<String, String>) -> String {
    let mut result = html.to_string();

    // Sort URLs by length (longest first) to avoid partial replacements.
    let mut urls: Vec<(&String, &String)> = url_map.iter().collect();
    urls.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (original, replacement) in urls {
        result = result.replace(original.as_str(), replacement.as_str());
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AssetsConfig;

    fn default_config() -> AssetsConfig {
        AssetsConfig {
            localize: true,
            cdn_skip_hosts: Vec::new(),
            cdn_allow_hosts: Vec::new(),
            images: Default::default()
        }
    }

    // --- extract_remote_urls tests ---

    #[test]
    fn test_extract_img_src() {
        let html = r#"<img src="https://example.com/photo.jpg" alt="test">"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/photo.jpg"]);
    }

    #[test]
    fn test_extract_video_src() {
        let html = r#"<video src="https://example.com/video.mp4"></video>"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/video.mp4"]);
    }

    #[test]
    fn test_extract_source_src() {
        let html = r#"<source src="https://example.com/audio.ogg" type="audio/ogg">"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/audio.ogg"]);
    }

    #[test]
    fn test_extract_audio_src() {
        let html = r#"<audio src="https://example.com/song.mp3"></audio>"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/song.mp3"]);
    }

    #[test]
    fn test_extract_background_image() {
        let html = r#"<div style="background-image: url('https://example.com/bg.jpg')"></div>"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/bg.jpg"]);
    }

    #[test]
    fn test_extract_background_image_no_quotes() {
        let html = r#"<div style="background-image: url(https://example.com/bg.jpg)"></div>"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/bg.jpg"]);
    }

    #[test]
    fn test_extract_background_image_in_style_block() {
        let html = r#"<style>.hero { background-image: url("https://example.com/hero.png"); }</style>"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/hero.png"]);
    }

    #[test]
    fn test_skip_relative_urls() {
        let html = r#"<img src="/images/local.jpg"><img src="relative.png">"#;
        let urls = extract_remote_urls(html);
        assert!(urls.is_empty());
    }

    #[test]
    fn test_skip_iframe_src() {
        let html = r#"<iframe src="https://example.com/embed"></iframe>"#;
        let urls = extract_remote_urls(html);
        assert!(urls.is_empty());
    }

    #[test]
    fn test_multiple_urls() {
        let html = r#"
            <img src="https://example.com/a.jpg">
            <img src="https://example.com/b.png">
            <video src="https://example.com/c.mp4">
        "#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls.len(), 3);
    }

    #[test]
    fn test_single_quoted_src() {
        let html = r#"<img src='https://example.com/photo.jpg'>"#;
        let urls = extract_remote_urls(html);
        assert_eq!(urls, vec!["https://example.com/photo.jpg"]);
    }

    // --- should_skip_cdn tests ---

    #[test]
    fn test_skip_default_cdn() {
        let config = default_config();
        assert!(should_skip_cdn("https://cdn.jsdelivr.net/npm/htmx.org", &config));
        assert!(should_skip_cdn("https://fonts.googleapis.com/css2?family=Roboto", &config));
        assert!(should_skip_cdn("https://cdnjs.cloudflare.com/ajax/libs/foo.js", &config));
    }

    #[test]
    fn test_dont_skip_regular_url() {
        let config = default_config();
        assert!(!should_skip_cdn("https://example.com/photo.jpg", &config));
        assert!(!should_skip_cdn("https://mysite.com/images/banner.png", &config));
    }

    #[test]
    fn test_skip_user_configured_cdn() {
        let config = AssetsConfig {
            localize: true,
            cdn_skip_hosts: vec!["mycdn.example.com".to_string()],
            cdn_allow_hosts: Vec::new(),
            images: Default::default()
        };
        assert!(should_skip_cdn("https://mycdn.example.com/assets/lib.js", &config));
    }

    #[test]
    fn test_allow_overrides_default_skip() {
        let config = AssetsConfig {
            localize: true,
            cdn_skip_hosts: Vec::new(),
            cdn_allow_hosts: vec!["cdn.jsdelivr.net".to_string()],
            images: Default::default()
        };
        // Normally this would be skipped, but allow_hosts overrides.
        assert!(!should_skip_cdn("https://cdn.jsdelivr.net/my-image.jpg", &config));
    }

    // --- extract_host tests ---

    #[test]
    fn test_extract_host_basic() {
        assert_eq!(extract_host("https://example.com/path"), Some("example.com"));
        assert_eq!(extract_host("http://foo.bar.com:8080/path"), Some("foo.bar.com"));
        assert_eq!(extract_host("https://example.com"), Some("example.com"));
    }

    // --- rewrite_urls tests ---

    #[test]
    fn test_rewrite_urls_basic() {
        let html = r#"<img src="https://example.com/photo.jpg">"#;
        let mut map = HashMap::new();
        map.insert(
            "https://example.com/photo.jpg".to_string(),
            "/assets/photo-abc123.jpg".to_string(),
        );
        let result = rewrite_urls(html, &map);
        assert_eq!(result, r#"<img src="/assets/photo-abc123.jpg">"#);
    }

    #[test]
    fn test_rewrite_urls_multiple() {
        let html = r#"<img src="https://a.com/1.jpg"><img src="https://b.com/2.png">"#;
        let mut map = HashMap::new();
        map.insert("https://a.com/1.jpg".to_string(), "/assets/1-aaa.jpg".to_string());
        map.insert("https://b.com/2.png".to_string(), "/assets/2-bbb.png".to_string());
        let result = rewrite_urls(html, &map);
        assert!(result.contains("/assets/1-aaa.jpg"));
        assert!(result.contains("/assets/2-bbb.png"));
    }

    #[test]
    fn test_rewrite_background_image() {
        let html = r#"<div style="background-image: url('https://example.com/bg.jpg')"></div>"#;
        let mut map = HashMap::new();
        map.insert(
            "https://example.com/bg.jpg".to_string(),
            "/assets/bg-abc.jpg".to_string(),
        );
        let result = rewrite_urls(html, &map);
        assert!(result.contains("/assets/bg-abc.jpg"));
    }

    #[test]
    fn test_rewrite_same_url_multiple_occurrences() {
        let html = r#"<img src="https://example.com/x.jpg"><img src="https://example.com/x.jpg">"#;
        let mut map = HashMap::new();
        map.insert("https://example.com/x.jpg".to_string(), "/assets/x-abc.jpg".to_string());
        let result = rewrite_urls(html, &map);
        assert_eq!(result.matches("/assets/x-abc.jpg").count(), 2);
    }
}
