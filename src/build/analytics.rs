//! Analytics snippet injection.
//!
//! When `[analytics.google]` and/or `[analytics.umami]` are configured in
//! site.toml, the corresponding tracking snippets are injected into every
//! full rendered page before `</body>`.
//! Fragment files are not affected — analytics only belongs on full pages.

use crate::config::{AnalyticsConfig, UmamiAnalyticsConfig};

/// Build the Google Analytics gtag.js snippet.
fn build_google_snippet(tracking_id: &str) -> String {
    format!(
        r#"<script async src="https://www.googletagmanager.com/gtag/js?id={id}"></script>
<script>
  window.dataLayer = window.dataLayer || [];
  function gtag(){{dataLayer.push(arguments);}}
  gtag('js', new Date());
  gtag('config', '{id}');
</script>"#,
        id = tracking_id,
    )
}

/// Build the Umami analytics snippet.
///
/// Optional `data-*` attributes are only emitted when configured:
/// - `data-domains` when `domains` is set
/// - `data-auto-track="false"` when `auto_track` is false (true is Umami's default)
/// - `data-tag` when `tag` is set
fn build_umami_snippet(config: &UmamiAnalyticsConfig) -> String {
    let mut script = format!(
        r#"<script defer src="{}/script.js"
  data-website-id="{}""#,
        config.host_url, config.website_id,
    );
    if let Some(ref domains) = config.domains {
        script.push_str(&format!(
            r#"
  data-domains="{domains}""#
        ));
    }
    if !config.auto_track {
        script.push_str(
            r#"
  data-auto-track="false""#,
        );
    }
    if let Some(ref tag) = config.tag {
        script.push_str(&format!(
            r#"
  data-tag="{tag}""#
        ));
    }
    script.push_str("></script>");
    script
}

/// Inject analytics snippets into rendered HTML before `</body>`.
///
/// Collects all enabled provider snippets (Google first, then Umami) and
/// inserts them before the last `</body>` tag. If no `</body>` tag is found,
/// the snippets are appended at the end.
pub fn inject_analytics(html: &str, config: &AnalyticsConfig) -> String {
    let mut snippets = Vec::new();

    if let Some(ref google) = config.google {
        snippets.push(build_google_snippet(&google.tracking_id));
    }
    if let Some(ref umami) = config.umami {
        snippets.push(build_umami_snippet(umami));
    }

    if snippets.is_empty() {
        return html.to_string();
    }

    let combined = snippets.join("\n");
    let lower = html.to_lowercase();

    if let Some(pos) = lower.rfind("</body>") {
        let mut result = String::with_capacity(html.len() + combined.len() + 2);
        result.push_str(&html[..pos]);
        result.push('\n');
        result.push_str(&combined);
        result.push('\n');
        result.push_str(&html[pos..]);
        result
    } else {
        format!("{html}\n{combined}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnalyticsConfig, GoogleAnalyticsConfig, UmamiAnalyticsConfig};

    fn google_config(tracking_id: &str) -> AnalyticsConfig {
        AnalyticsConfig {
            google: Some(GoogleAnalyticsConfig {
                tracking_id: tracking_id.to_string(),
            }),
            umami: None,
        }
    }

    fn umami_config_minimal(website_id: &str) -> AnalyticsConfig {
        AnalyticsConfig {
            google: None,
            umami: Some(UmamiAnalyticsConfig {
                website_id: website_id.to_string(),
                host_url: "https://cloud.umami.is".to_string(),
                domains: None,
                auto_track: true,
                tag: None,
            }),
        }
    }

    // --- build_google_snippet ---

    #[test]
    fn test_google_snippet_contains_tracking_id_twice() {
        let snippet = build_google_snippet("G-MYSITE99");
        let count = snippet.matches("G-MYSITE99").count();
        assert_eq!(
            count, 2,
            "tracking ID should appear in script src and gtag config"
        );
    }

    #[test]
    fn test_google_snippet_structure() {
        let snippet = build_google_snippet("G-ABC123");
        assert!(snippet.contains(r#"src="https://www.googletagmanager.com/gtag/js?id=G-ABC123""#));
        assert!(snippet.contains("window.dataLayer = window.dataLayer || []"));
        assert!(snippet.contains("function gtag()"));
        assert!(snippet.contains("gtag('js', new Date())"));
        assert!(snippet.contains("gtag('config', 'G-ABC123')"));
    }

    // --- build_umami_snippet ---

    #[test]
    fn test_umami_snippet_minimal() {
        let config = UmamiAnalyticsConfig {
            website_id: "abc-123".to_string(),
            host_url: "https://cloud.umami.is".to_string(),
            domains: None,
            auto_track: true,
            tag: None,
        };
        let snippet = build_umami_snippet(&config);
        assert!(snippet.contains(r#"src="https://cloud.umami.is/script.js""#));
        assert!(snippet.contains(r#"data-website-id="abc-123""#));
        assert!(!snippet.contains("data-domains"));
        assert!(!snippet.contains("data-auto-track"));
        assert!(!snippet.contains("data-tag"));
    }

    #[test]
    fn test_umami_snippet_full() {
        let config = UmamiAnalyticsConfig {
            website_id: "abc-123".to_string(),
            host_url: "https://analytics.example.com".to_string(),
            domains: Some("example.com,www.example.com".to_string()),
            auto_track: false,
            tag: Some("production".to_string()),
        };
        let snippet = build_umami_snippet(&config);
        assert!(snippet.contains(r#"src="https://analytics.example.com/script.js""#));
        assert!(snippet.contains(r#"data-website-id="abc-123""#));
        assert!(snippet.contains(r#"data-domains="example.com,www.example.com""#));
        assert!(snippet.contains(r#"data-auto-track="false""#));
        assert!(snippet.contains(r#"data-tag="production""#));
    }

    #[test]
    fn test_umami_snippet_auto_track_true_omits_attribute() {
        let config = UmamiAnalyticsConfig {
            website_id: "abc-123".to_string(),
            host_url: "https://cloud.umami.is".to_string(),
            domains: None,
            auto_track: true,
            tag: None,
        };
        let snippet = build_umami_snippet(&config);
        assert!(
            !snippet.contains("data-auto-track"),
            "auto_track=true should not emit the attribute"
        );
    }

    // --- inject_analytics ---

    #[test]
    fn test_inject_google_only() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = google_config("G-TEST123");
        let result = inject_analytics(html, &config);
        assert!(result.contains("googletagmanager"));
        assert!(!result.contains("umami"));
    }

    #[test]
    fn test_inject_umami_only() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = umami_config_minimal("abc-123");
        let result = inject_analytics(html, &config);
        assert!(result.contains("data-website-id"));
        assert!(!result.contains("googletagmanager"));
    }

    #[test]
    fn test_inject_both_providers() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = AnalyticsConfig {
            google: Some(GoogleAnalyticsConfig {
                tracking_id: "G-BOTH".to_string(),
            }),
            umami: Some(UmamiAnalyticsConfig {
                website_id: "both-123".to_string(),
                host_url: "https://cloud.umami.is".to_string(),
                domains: None,
                auto_track: true,
                tag: None,
            }),
        };
        let result = inject_analytics(html, &config);
        let google_pos = result.find("googletagmanager").unwrap();
        let umami_pos = result.find("data-website-id").unwrap();
        assert!(
            google_pos < umami_pos,
            "Google snippet should come before Umami"
        );
        let body_pos = result.find("</body>").unwrap();
        assert!(
            umami_pos < body_pos,
            "both snippets should be before </body>"
        );
    }

    #[test]
    fn test_inject_neither_provider() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = AnalyticsConfig {
            google: None,
            umami: None,
        };
        let result = inject_analytics(html, &config);
        assert_eq!(result, html, "no providers means no changes");
    }

    #[test]
    fn test_inject_case_insensitive_body_tag() {
        let html = "<html><body><p>test</p></BODY></html>";
        let config = umami_config_minimal("abc-123");
        let result = inject_analytics(html, &config);
        let snippet_pos = result.find("data-website-id").unwrap();
        let body_pos = result.find("</BODY>").unwrap();
        assert!(snippet_pos < body_pos);
    }

    #[test]
    fn test_inject_no_body_tag_appends() {
        let html = "<h1>Fragment</h1>";
        let config = google_config("G-FRAG");
        let result = inject_analytics(html, &config);
        assert!(result.contains("googletagmanager"));
        assert!(result.ends_with("</script>"));
    }

    #[test]
    fn test_inject_preserves_original_content() {
        let html = "<html><body><h1>Hello World</h1><p>Content</p></body></html>";
        let config = google_config("G-TEST");
        let result = inject_analytics(html, &config);
        assert!(result.contains("<h1>Hello World</h1>"));
        assert!(result.contains("<p>Content</p>"));
        assert!(result.contains("</body></html>"));
    }
}
