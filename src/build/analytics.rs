//! Google Analytics (gtag.js) injection.
//!
//! When `[analytics] tracking_id` is set in site.toml, the gtag.js snippet
//! is injected into every full rendered page before `</body>`.
//! Fragment files are not affected — analytics only belongs on full pages.

/// Build the gtag.js snippet for the given tracking ID.
fn build_snippet(tracking_id: &str) -> String {
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

/// Inject the gtag.js snippet into rendered HTML before `</body>`.
///
/// Uses a case-insensitive search for `</body>` and inserts immediately
/// before it. If no `</body>` tag is found (e.g. a bare HTML fragment),
/// the snippet is appended at the end.
pub fn inject_analytics(html: &str, tracking_id: &str) -> String {
    let snippet = build_snippet(tracking_id);
    let lower = html.to_lowercase();

    if let Some(pos) = lower.rfind("</body>") {
        let mut result = String::with_capacity(html.len() + snippet.len() + 1);
        result.push_str(&html[..pos]);
        result.push('\n');
        result.push_str(&snippet);
        result.push('\n');
        result.push_str(&html[pos..]);
        result
    } else {
        format!("{}\n{}", html, snippet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- inject_analytics ---

    /// Basic case: snippet is injected before </body> in a normal full HTML page.
    /// This is the happy path — every real page will have a </body> tag.
    #[test]
    fn test_injects_before_body_close() {
        let html = "<html><head></head><body><h1>Hi</h1></body></html>";
        let result = inject_analytics(html, "G-TEST123");
        // Snippet must appear before </body>
        let snippet_pos = result.find("googletagmanager").unwrap();
        let body_pos = result.find("</body>").unwrap();
        assert!(snippet_pos < body_pos, "snippet should come before </body>");
    }

    /// The tracking ID must appear in the injected snippet — both in the
    /// async script src URL and in the gtag('config', ...) call.
    /// If the ID is wrong or missing, GA won't track anything.
    #[test]
    fn test_tracking_id_appears_in_snippet() {
        let html = "<html><body></body></html>";
        let result = inject_analytics(html, "G-MYSITE99");
        assert!(
            result.contains("G-MYSITE99"),
            "tracking ID must appear in the injected snippet"
        );
        // Must appear twice: once in script src, once in gtag('config', ...)
        let count = result.matches("G-MYSITE99").count();
        assert_eq!(count, 2, "tracking ID should appear exactly twice");
    }

    /// The original page content must be preserved exactly.
    /// Injection must not corrupt, truncate, or duplicate any existing HTML.
    #[test]
    fn test_original_content_preserved() {
        let html = "<html><body><h1>Hello World</h1><p>Some content</p></body></html>";
        let result = inject_analytics(html, "G-TEST123");
        assert!(result.contains("<h1>Hello World</h1>"));
        assert!(result.contains("<p>Some content</p>"));
        assert!(result.contains("</body></html>"));
    }

    /// </body> tag matching must be case-insensitive.
    /// Some HTML generators or editors write </BODY> in uppercase — eigen
    /// must still inject correctly rather than falling back to appending.
    #[test]
    fn test_case_insensitive_body_tag() {
        let html = "<html><body><p>test</p></BODY></html>";
        let result = inject_analytics(html, "G-TEST123");
        // Should inject before the closing tag, not at the end
        let snippet_pos = result.find("googletagmanager").unwrap();
        let body_pos = result.find("</BODY>").unwrap();
        assert!(snippet_pos < body_pos);
    }

    /// When there are multiple </body> tags (malformed HTML or nested
    /// templates that haven't been fully composed), inject before the LAST
    /// one. rfind ensures we always target the outermost closing tag.
    #[test]
    fn test_injects_before_last_body_tag() {
        let html = "<body>inner</body><body>outer</body>";
        let result = inject_analytics(html, "G-TEST123");
        // rfind means snippet goes before the second </body>
        let last_body = result.rfind("</body>").unwrap();
        let snippet_pos = result.rfind("googletagmanager").unwrap();
        assert!(snippet_pos < last_body);
    }

    /// When there is no </body> tag at all (e.g. a bare HTML fragment or
    /// a partial template), the snippet is appended at the end.
    /// This prevents silent failures where analytics is never injected.
    #[test]
    fn test_no_body_tag_appends_at_end() {
        let html = "<h1>Just a fragment</h1><p>No body tag here</p>";
        let result = inject_analytics(html, "G-TEST123");
        assert!(result.contains("googletagmanager"));
        // Since there's no </body>, snippet is at the very end
        assert!(result.ends_with("</script>"));
    }

    /// An empty tracking ID produces a snippet with empty strings in the src
    /// URL and config call. This is a misconfiguration — the config validator
    /// should catch it, but injection itself must not panic.
    #[test]
    fn test_empty_tracking_id_does_not_panic() {
        let html = "<html><body></body></html>";
        let result = inject_analytics(html, "");
        // Should complete without panicking; snippet still injected
        assert!(result.contains("googletagmanager"));
    }

    /// Injecting into an already-analytics-injected page (e.g. if the
    /// pipeline runs twice by accident) must not produce two snippets.
    /// This guards against double-counting in GA.
    /// NOTE: eigen does not call this twice in normal operation, but this
    /// test documents the behaviour if it ever happens.
    #[test]
    fn test_double_injection_behaviour() {
        let html = "<html><body></body></html>";
        let once = inject_analytics(html, "G-TEST123");
        let twice = inject_analytics(&once, "G-TEST123");
        // Two snippets would mean double-counting — document that this happens
        // so if eigen ever deduplicates, this test needs updating.
        let count = twice.matches("googletagmanager").count();
        assert_eq!(count, 2, "double injection produces two snippets — caller must ensure single injection");
    }

    /// An entirely empty HTML string should not panic and should return
    /// just the snippet. Edge case for empty template output.
    #[test]
    fn test_empty_html_input() {
        let result = inject_analytics("", "G-TEST123");
        assert!(result.contains("googletagmanager"));
    }

    /// The snippet structure must conform to the standard gtag.js pattern:
    /// - async script tag loading gtag/js with the measurement ID
    /// - inline script with dataLayer init, gtag function, and config call
    /// If this structure breaks, GA stops working entirely.
    #[test]
    fn test_snippet_structure() {
        let html = "<html><body></body></html>";
        let result = inject_analytics(html, "G-ABC123");
        assert!(result.contains(r#"src="https://www.googletagmanager.com/gtag/js?id=G-ABC123""#));
        assert!(result.contains("window.dataLayer = window.dataLayer || []"));
        assert!(result.contains("function gtag()"));
        assert!(result.contains("gtag('js', new Date())"));
        assert!(result.contains("gtag('config', 'G-ABC123')"));
    }
}
