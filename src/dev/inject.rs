//! Step 7.4: Live reload client injection.
//!
//! During dev builds, injects a small `<script>` tag before `</body>` that
//! connects to the `/_reload` SSE endpoint and reloads the page on events.
//! This is never active during `eigen build` — only during `eigen dev`.

/// The live-reload script injected into every full HTML page during dev mode.
const RELOAD_SCRIPT: &str = r#"<script>
  const es = new EventSource("/_reload");
  es.addEventListener("reload", () => window.location.reload());
  es.onerror = () => setTimeout(() => window.location.reload(), 1000);
</script>"#;

/// Inject the live-reload script into rendered HTML.
///
/// Inserts the script immediately before `</body>`. If `</body>` is not
/// found, the script is appended at the end.
pub fn inject_reload_script(html: &str) -> String {
    // Case-insensitive search for </body>.
    let lower = html.to_lowercase();
    if let Some(pos) = lower.rfind("</body>") {
        let mut result = String::with_capacity(html.len() + RELOAD_SCRIPT.len() + 1);
        result.push_str(&html[..pos]);
        result.push('\n');
        result.push_str(RELOAD_SCRIPT);
        result.push('\n');
        result.push_str(&html[pos..]);
        result
    } else {
        // No </body> tag — append at end.
        format!("{}\n{}", html, RELOAD_SCRIPT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_before_body_close() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let result = inject_reload_script(html);
        assert!(result.contains("EventSource"));
        assert!(result.contains("/_reload"));
        // Script should appear before </body>.
        let script_pos = result.find("EventSource").unwrap();
        let body_close_pos = result.find("</body>").unwrap();
        assert!(script_pos < body_close_pos);
    }

    #[test]
    fn test_inject_case_insensitive() {
        let html = "<html><body><p>test</p></BODY></html>";
        let result = inject_reload_script(html);
        assert!(result.contains("EventSource"));
    }

    #[test]
    fn test_inject_no_body_tag() {
        let html = "<h1>Fragment</h1>";
        let result = inject_reload_script(html);
        assert!(result.contains("EventSource"));
        assert!(result.ends_with("</script>"));
    }

    #[test]
    fn test_inject_preserves_content() {
        let html = "<html><body><h1>Hello</h1></body></html>";
        let result = inject_reload_script(html);
        assert!(result.contains("<h1>Hello</h1>"));
        assert!(result.contains("</body></html>"));
    }
}
