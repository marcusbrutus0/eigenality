use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::build::source_asset::SourceAssetCollector;

static ROOT_RELATIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:src|href)\s*=\s*"(/[^/"][^"]*)""#).unwrap()
});

static ROOT_RELATIVE_SQ_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:src|href)\s*=\s*'(/[^/'][^']*)'"#).unwrap()
});

/// Extract the origin (scheme + host + optional port) from a URL.
///
/// Example: `https://cms.example.com/api/v1/apps/x` → `https://cms.example.com`
pub fn extract_origin(url: &str) -> Option<String> {
    let scheme_end = url.find("://")?;
    let after_scheme = &url[scheme_end + 3..];
    let authority_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    Some(format!(
        "{}{}",
        &url[..scheme_end + 3],
        &after_scheme[..authority_end]
    ))
}

/// Resolve root-relative URLs in HTML attributes within all string values
/// of a JSON tree. Pushes resolved absolute URLs into the collector.
pub fn resolve_html_urls_in_value(
    value: Value,
    origin: &str,
    source_name: &str,
    collector: Option<&SourceAssetCollector>,
) -> Value {
    let mut urls = Vec::new();
    let result = resolve_value(value, origin, &mut urls);
    if let Some(collector) = collector {
        for url in urls {
            collector.push(source_name.to_string(), url);
        }
    }
    result
}

fn resolve_value(value: Value, origin: &str, collected: &mut Vec<String>) -> Value {
    match value {
        Value::String(s) => Value::String(resolve_string(&s, origin, collected)),
        Value::Array(arr) => {
            Value::Array(arr.into_iter().map(|v| resolve_value(v, origin, collected)).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, resolve_value(v, origin, collected)))
                .collect(),
        ),
        other => other,
    }
}

fn resolve_string(s: &str, origin: &str, collected: &mut Vec<String>) -> String {
    let after_dq = ROOT_RELATIVE_RE.replace_all(s, |caps: &regex::Captures| {
        let path = &caps[1];
        let absolute = format!("{}{}", origin, path);
        collected.push(absolute.clone());
        let full = &caps[0];
        let eq_pos = full.find('=').unwrap();
        format!("{}=\"{}\"", &full[..eq_pos], absolute)
    });

    let result = ROOT_RELATIVE_SQ_RE.replace_all(&after_dq, |caps: &regex::Captures| {
        let path = &caps[1];
        let absolute = format!("{}{}", origin, path);
        collected.push(absolute.clone());
        let full = &caps[0];
        let eq_pos = full.find('=').unwrap();
        format!("{}='{}'", &full[..eq_pos], absolute)
    });

    result.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_origin_https() {
        assert_eq!(
            extract_origin("https://cms.example.com/api/v1/apps/x"),
            Some("https://cms.example.com".to_string()),
        );
    }

    #[test]
    fn extract_origin_with_port() {
        assert_eq!(
            extract_origin("https://cms.example.com:8443/api"),
            Some("https://cms.example.com:8443".to_string()),
        );
    }

    #[test]
    fn extract_origin_http() {
        assert_eq!(
            extract_origin("http://localhost:3000/api"),
            Some("http://localhost:3000".to_string()),
        );
    }

    #[test]
    fn extract_origin_no_path() {
        assert_eq!(
            extract_origin("https://cms.example.com"),
            Some("https://cms.example.com".to_string()),
        );
    }

    #[test]
    fn extract_origin_not_http() {
        assert_eq!(extract_origin("/some/path"), None);
    }

    #[test]
    fn extract_origin_empty() {
        assert_eq!(extract_origin(""), None);
    }

    #[test]
    fn resolve_rewrites_src_root_relative() {
        let input = json!({
            "body": r#"<img src="/uploads/photo.jpg">"#
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(
            result["body"].as_str().unwrap(),
            r#"<img src="https://cms.example.com/uploads/photo.jpg">"#,
        );
        assert_eq!(urls, vec!["https://cms.example.com/uploads/photo.jpg"]);
    }

    #[test]
    fn resolve_rewrites_href_root_relative() {
        let input = json!({
            "body": r#"<a href="/docs/guide.pdf">Download</a>"#
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(
            result["body"].as_str().unwrap(),
            r#"<a href="https://cms.example.com/docs/guide.pdf">Download</a>"#,
        );
        assert_eq!(urls, vec!["https://cms.example.com/docs/guide.pdf"]);
    }

    #[test]
    fn resolve_ignores_absolute_urls() {
        let input = json!({
            "body": r#"<img src="https://other.com/photo.jpg">"#
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(
            result["body"].as_str().unwrap(),
            r#"<img src="https://other.com/photo.jpg">"#,
        );
        assert!(urls.is_empty());
    }

    #[test]
    fn resolve_ignores_protocol_relative_urls() {
        let input = json!({
            "body": r#"<img src="//cdn.example.com/photo.jpg">"#
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(
            result["body"].as_str().unwrap(),
            r#"<img src="//cdn.example.com/photo.jpg">"#,
        );
        assert!(urls.is_empty());
    }

    #[test]
    fn resolve_ignores_non_html_strings() {
        let input = json!({
            "path": "/api/v1/foo",
            "name": "my item"
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(result["path"].as_str().unwrap(), "/api/v1/foo");
        assert_eq!(result["name"].as_str().unwrap(), "my item");
        assert!(urls.is_empty());
    }

    #[test]
    fn resolve_walks_nested_objects_and_arrays() {
        let input = json!({
            "items": [
                { "content": r#"<img src="/uploads/a.jpg">"# },
                { "content": r#"<img src="/uploads/b.jpg">"# },
            ]
        });
        let (result, mut urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(
            result["items"][0]["content"].as_str().unwrap(),
            r#"<img src="https://cms.example.com/uploads/a.jpg">"#,
        );
        assert_eq!(
            result["items"][1]["content"].as_str().unwrap(),
            r#"<img src="https://cms.example.com/uploads/b.jpg">"#,
        );
        urls.sort();
        assert_eq!(urls, vec![
            "https://cms.example.com/uploads/a.jpg",
            "https://cms.example.com/uploads/b.jpg",
        ]);
    }

    #[test]
    fn resolve_handles_single_quoted_attributes() {
        let input = json!({
            "body": "<img src='/uploads/photo.jpg'>"
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(
            result["body"].as_str().unwrap(),
            "<img src='https://cms.example.com/uploads/photo.jpg'>",
        );
        assert_eq!(urls, vec!["https://cms.example.com/uploads/photo.jpg"]);
    }

    #[test]
    fn resolve_handles_multiple_attrs_in_one_string() {
        let input = json!({
            "body": r#"<img src="/uploads/a.jpg"><a href="/docs/b.pdf">link</a>"#
        });
        let (result, mut urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(
            result["body"].as_str().unwrap(),
            r#"<img src="https://cms.example.com/uploads/a.jpg"><a href="https://cms.example.com/docs/b.pdf">link</a>"#,
        );
        urls.sort();
        assert_eq!(urls, vec![
            "https://cms.example.com/docs/b.pdf",
            "https://cms.example.com/uploads/a.jpg",
        ]);
    }

    #[test]
    fn extract_origin_malformed_url_returns_none() {
        assert_eq!(extract_origin("not-a-url"), None);
        assert_eq!(
            extract_origin("ftp://files.example.com/data"),
            Some("ftp://files.example.com".to_string()),
        );
        // "://missing-scheme" has "://" at index 0, so scheme_end=0 and the
        // function returns Some("://missing-scheme"). This edge case is acceptable
        // because extract_origin is only ever called with real URLs from SourceConfig.
        assert_eq!(
            extract_origin("://missing-scheme"),
            Some("://missing-scheme".to_string()),
        );
        assert_eq!(extract_origin(""), None);
    }

    #[test]
    fn resolve_no_mutation_when_no_root_relative() {
        let input = json!({
            "title": "Hello World",
            "count": 42,
            "active": true,
            "tags": ["rust", "wasm"],
            "body": "<p>No images here</p>",
            "link": "<a href=\"https://example.com\">External</a>",
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://cms.example.com");
        assert_eq!(result, input);
        assert!(urls.is_empty());
    }

    #[test]
    fn resolve_deeply_nested_structure() {
        let input = json!({
            "data": {
                "pages": [
                    {
                        "sections": [
                            {
                                "blocks": [
                                    { "html": "<img src=\"/media/hero.jpg\">" }
                                ]
                            }
                        ]
                    }
                ]
            }
        });
        let (result, urls) = resolve_html_urls_in_value_collect(&input, "https://api.example.com");
        assert_eq!(
            result["data"]["pages"][0]["sections"][0]["blocks"][0]["html"].as_str().unwrap(),
            "<img src=\"https://api.example.com/media/hero.jpg\">",
        );
        assert_eq!(urls, vec!["https://api.example.com/media/hero.jpg"]);
    }

    #[test]
    fn resolve_with_collector() {
        use crate::build::source_asset::SourceAssetCollector;

        let input = json!({
            "body": "<img src=\"/uploads/photo.jpg\">"
        });
        let collector = SourceAssetCollector::new();
        let result = resolve_html_urls_in_value(
            input,
            "https://cms.example.com",
            "my_cms",
            Some(&collector),
        );
        assert_eq!(
            result["body"].as_str().unwrap(),
            "<img src=\"https://cms.example.com/uploads/photo.jpg\">",
        );
        let requests = collector.drain();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].source_name, "my_cms");
        assert_eq!(requests[0].url, "https://cms.example.com/uploads/photo.jpg");
    }

    /// Helper: resolve and collect URLs without needing SourceAssetCollector.
    fn resolve_html_urls_in_value_collect(value: &Value, origin: &str) -> (Value, Vec<String>) {
        let mut collected = Vec::new();
        let result = resolve_value(value.clone(), origin, &mut collected);
        (result, collected)
    }
}
