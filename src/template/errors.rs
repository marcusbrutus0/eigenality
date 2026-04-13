//! Template error formatting utilities.
//!
//! Provides detailed, human-readable error messages for minijinja template
//! rendering failures, including the template name, line number, error kind,
//! a source context snippet, and a summary of the template context when available.

use minijinja::Value;

/// A structured representation of a template rendering error with all
/// the detail we can extract from minijinja.
#[derive(Debug, Clone)]
pub struct TemplateError {
    /// Which template file caused the error (e.g. `"index.html"`).
    pub template_name: Option<String>,
    /// The line number in the *original file* (adjusted for frontmatter).
    pub line: Option<usize>,
    /// The raw line number from minijinja (before frontmatter adjustment).
    /// Only differs from `line` when the error is in the page template and
    /// frontmatter was stripped. Used to show both numbers in error output.
    pub raw_line: Option<usize>,
    /// The kind of error (e.g. `"UndefinedError"`, `"SyntaxError"`).
    pub kind: String,
    /// The short error description from minijinja.
    pub short_msg: String,
    /// The full detailed output from `Error::display_detail()`, which includes
    /// a source code snippet with line numbers and an arrow pointing to the
    /// offending line.
    pub detail: String,
    /// A summary of the top-level template context keys and their types,
    /// to help diagnose missing-variable errors.
    pub context_summary: Option<String>,
}

impl TemplateError {
    /// Extract a `TemplateError` from a `minijinja::Error`.
    ///
    /// `frontmatter_line_count` is the number of lines the frontmatter block
    /// occupies in the original file (including `---` delimiters). The line
    /// number is only adjusted when the error originated in the page template
    /// itself — errors in included/parent templates (e.g. `_base.html`) are
    /// left as-is because those files have no frontmatter.
    ///
    /// `tpl_ctx` is the template rendering context; when provided, a summary
    /// of the top-level keys and their types is included in the error output
    /// to help diagnose missing-variable errors.
    pub fn from_minijinja(
        err: &minijinja::Error,
        rendering_template: &str,
        frontmatter_line_count: usize,
        tpl_ctx: Option<&Value>,
    ) -> Self {
        let template_name = err.name()
            .map(|n| n.to_string())
            .or_else(|| Some(rendering_template.to_string()));

        // Only adjust the line number when the error is in the page template
        // itself (not in an included _base.html or partial).
        let error_in_page_template = match &template_name {
            Some(name) => name == rendering_template,
            None => true,
        };
        let raw_line = err.line();
        let line = raw_line.map(|l| {
            if error_in_page_template && frontmatter_line_count > 0 {
                l + frontmatter_line_count
            } else {
                l
            }
        });

        let kind = format!("{:?}", err.kind());

        let short_msg = err.to_string();

        // Build the full detail string. `display_detail()` shows a formatted
        // error with template source context (when available).
        let mut detail = format!("Could not render template: {:#}", err);

        let mut err = &err as &dyn std::error::Error;
        while let Some(next_err) = err.source() {
            detail.push_str("\n\n");
            detail.push_str(&format!("caused by: {:#}", next_err));
            err = next_err;
        }

        let context_summary = tpl_ctx.map(summarize_context);

        TemplateError {
            template_name,
            line,
            raw_line,
            kind,
            short_msg,
            detail,
            context_summary,
        }
    }

    /// Format a rich console error message with colors (via ANSI) for terminal output.
    pub fn format_console(&self, rendering_template: &str, slug: Option<&str>) -> String {
        let mut out = String::new();
        out.push('\n');
        out.push_str("  ── Template Render Error ─────────────────────────────────\n");
        out.push('\n');

        // Template info
        out.push_str(&format!("  Template : {}\n", rendering_template));
        if let Some(slug) = slug {
            out.push_str(&format!("  Item slug: {}\n", slug));
        }
        if let Some(ref name) = self.template_name {
            if name != rendering_template {
                out.push_str(&format!("  Error in : {}\n", name));
            }
        }
        if let Some(line) = self.line {
            if self.raw_line != self.line {
                out.push_str(&format!(
                    "  Line     : {} (template line {})\n",
                    line,
                    self.raw_line.unwrap_or(0),
                ));
            } else {
                out.push_str(&format!("  Line     : {}\n", line));
            }
        }
        out.push_str(&format!("  Kind     : {}\n", self.kind));
        out.push('\n');

        // The short message
        out.push_str(&format!("  Error: {}\n", self.short_msg));
        out.push('\n');

        // The detailed display with source context
        if !self.detail.is_empty() {
            out.push_str("  Detail:\n");
            for line in self.detail.lines() {
                out.push_str(&format!("    {}\n", line));
            }
        }

        // Context dump — shows which variables were available.
        if let Some(ref ctx) = self.context_summary {
            out.push('\n');
            out.push_str("  Available context:\n");
            for line in ctx.lines() {
                out.push_str(&format!("    {}\n", line));
            }
        }

        out.push('\n');
        out.push_str("  ─────────────────────────────────────────────────────────\n");
        out
    }

    /// Generate an HTML error page suitable for display in the browser during dev mode.
    ///
    /// Includes the live-reload script so the browser will auto-refresh when
    /// the user fixes the error.
    pub fn to_error_html(&self, rendering_template: &str, slug: Option<&str>) -> String {
        let escaped_template = html_escape(rendering_template);
        let escaped_kind = html_escape(&self.kind);
        let escaped_msg = html_escape(&self.short_msg);
        let escaped_detail = html_escape(&self.detail);
        let context_section = match self.context_summary.as_deref().map(html_escape) {
            Some(escaped_ctx) => format!(
                r#"
      <div class="detail-label" style="margin-top:1rem">Available Context</div>
      <div class="detail">{escaped_ctx}</div>"#
            ),
            None => String::new(),
        };

        let slug_row = if let Some(slug) = slug {
            format!(
                r#"<tr><td class="label">Item slug</td><td>{}</td></tr>"#,
                html_escape(slug)
            )
        } else {
            String::new()
        };

        let error_in_row = if let Some(ref name) = self.template_name {
            if name != rendering_template {
                format!(
                    r#"<tr><td class="label">Error in</td><td><code>{}</code></td></tr>"#,
                    html_escape(name)
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let line_row = if let Some(line) = self.line {
            if self.raw_line != self.line {
                format!(
                    r#"<tr><td class="label">Line</td><td>{} <span style="color:#888">(template line {})</span></td></tr>"#,
                    line, self.raw_line.unwrap_or(0),
                )
            } else {
                format!(
                    r#"<tr><td class="label">Line</td><td>{}</td></tr>"#,
                    line
                )
            }
        } else {
            String::new()
        };

        format!(
            r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Build Error – {escaped_template}</title>
  <style>
    * {{ margin: 0; padding: 0; box-sizing: border-box; }}
    body {{
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace;
      background: #1a1a2e;
      color: #e0e0e0;
      padding: 2rem;
      min-height: 100vh;
    }}
    .error-container {{
      max-width: 900px;
      margin: 0 auto;
    }}
    .error-banner {{
      background: #e74c3c;
      color: #fff;
      padding: 1rem 1.5rem;
      border-radius: 8px 8px 0 0;
      font-size: 1.1rem;
      font-weight: 600;
    }}
    .error-banner svg {{
      vertical-align: middle;
      margin-right: 0.5rem;
    }}
    .error-body {{
      background: #16213e;
      border: 1px solid #e74c3c;
      border-top: none;
      border-radius: 0 0 8px 8px;
      padding: 1.5rem;
    }}
    table {{
      border-collapse: collapse;
      margin-bottom: 1.5rem;
      width: 100%;
    }}
    table td {{
      padding: 0.35rem 1rem 0.35rem 0;
      vertical-align: top;
    }}
    td.label {{
      color: #888;
      white-space: nowrap;
      width: 120px;
      font-size: 0.9rem;
    }}
    td code {{
      background: #0f3460;
      padding: 0.15rem 0.5rem;
      border-radius: 3px;
      font-size: 0.95rem;
    }}
    .message {{
      background: #0f3460;
      border-left: 4px solid #e74c3c;
      padding: 1rem 1.25rem;
      margin-bottom: 1.5rem;
      border-radius: 0 6px 6px 0;
      font-size: 1rem;
      line-height: 1.5;
      word-break: break-word;
    }}
    .detail {{
      background: #0a0a1a;
      border: 1px solid #333;
      border-radius: 6px;
      padding: 1rem 1.25rem;
      overflow-x: auto;
      font-family: "Fira Code", "Cascadia Code", "JetBrains Mono", monospace;
      font-size: 0.85rem;
      line-height: 1.6;
      white-space: pre;
      color: #ccc;
    }}
    .detail-label {{
      color: #888;
      font-size: 0.85rem;
      margin-bottom: 0.5rem;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    }}
    .hint {{
      margin-top: 1.5rem;
      padding: 0.75rem 1rem;
      background: #1b2838;
      border-radius: 6px;
      font-size: 0.85rem;
      color: #aaa;
    }}
    .hint strong {{
      color: #4fc3f7;
    }}
  </style>
</head>
<body>
  <div class="error-container">
    <div class="error-banner">
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="10" r="9" stroke="#fff" stroke-width="2"/><line x1="10" y1="5" x2="10" y2="11" stroke="#fff" stroke-width="2" stroke-linecap="round"/><circle cx="10" cy="14.5" r="1.2" fill="#fff"/></svg>
      Template Render Error
    </div>
    <div class="error-body">
      <table>
        <tr><td class="label">Template</td><td><code>{escaped_template}</code></td></tr>
        {slug_row}
        {error_in_row}
        {line_row}
        <tr><td class="label">Kind</td><td>{escaped_kind}</td></tr>
      </table>

      <div class="message">{escaped_msg}</div>

      <div class="detail-label">Detail</div>
      <div class="detail">{escaped_detail}</div>
      {context_section}

      <div class="hint">
        <strong>Tip:</strong> Fix the error in your template and save — the page will automatically reload.
      </div>
    </div>
  </div>

<script>
  const es = new EventSource("/_reload");
  es.addEventListener("reload", () => window.location.reload());
  es.onerror = () => setTimeout(() => window.location.reload(), 1000);
</script>
</body>
</html>"##,
        )
    }
}

/// Build a one-line-per-key summary of the template context, showing each
/// top-level variable name and the kind of value it holds (map, sequence,
/// string, etc.). For sequences and maps the element count is included.
fn summarize_context(ctx: &Value) -> String {
    let mut lines = Vec::new();
    if let Ok(keys) = ctx.try_iter() {
        for key in keys {
            let label = match ctx.get_attr(key.as_str().unwrap_or("?")) {
                Ok(val) => {
                    let kind = val.kind();
                    match kind {
                        minijinja::value::ValueKind::Seq => {
                            let len = val.try_iter().map(|i| i.count()).unwrap_or(0);
                            format!("{kind} ({len} items)")
                        }
                        minijinja::value::ValueKind::Map => {
                            let len = val.try_iter().map(|i| i.count()).unwrap_or(0);
                            format!("{kind} ({len} keys)")
                        }
                        _ => kind.to_string(),
                    }
                }
                Err(_) => "?".into(),
            };
            lines.push(format!("{key} : {label}"));
        }
    }
    if lines.is_empty() {
        "(empty context)".into()
    } else {
        lines.join("\n")
    }
}

/// Simple HTML escaping for inserting text into HTML.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<b>hello</b>"), "&lt;b&gt;hello&lt;/b&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape(r#""quoted""#), "&quot;quoted&quot;");
    }

    #[test]
    fn test_template_error_console_format() {
        let te = TemplateError {
            template_name: Some("_base.html".into()),
            line: Some(5),
            raw_line: Some(5),
            kind: "UndefinedError".into(),
            short_msg: "undefined variable `foo`".into(),
            detail: "line 5\n  --> {{ foo }}\n      ^^^".into(),
            context_summary: None,
        };

        let formatted = te.format_console("index.html", None);
        assert!(formatted.contains("Template : index.html"));
        assert!(formatted.contains("Error in : _base.html"));
        assert!(formatted.contains("Line     : 5"));
        assert!(formatted.contains("UndefinedError"));
        assert!(formatted.contains("undefined variable `foo`"));
    }

    #[test]
    fn test_template_error_html_output() {
        let te = TemplateError {
            template_name: Some("index.html".into()),
            line: Some(3),
            raw_line: Some(3),
            kind: "UndefinedError".into(),
            short_msg: "variable `missing_var` is undefined".into(),
            detail: "line 3: {{ missing_var }}".into(),
            context_summary: None,
        };

        let html = te.to_error_html("index.html", None);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Template Render Error"));
        assert!(html.contains("index.html"));
        assert!(html.contains("UndefinedError"));
        assert!(html.contains("missing_var"));
        assert!(html.contains("/_reload")); // Has live-reload script
    }

    #[test]
    fn test_template_error_html_with_slug() {
        let te = TemplateError {
            template_name: Some("posts/[post].html".into()),
            line: Some(10),
            raw_line: Some(10),
            kind: "UndefinedError".into(),
            short_msg: "variable `post.missing` is undefined".into(),
            detail: "line 10: {{ post.missing }}".into(),
            context_summary: None,
        };

        let html = te.to_error_html("posts/[post].html", Some("hello-world"));
        assert!(html.contains("hello-world"));
        assert!(html.contains("Item slug"));
    }

    // --- Line offset adjustment tests ---

    #[test]
    fn test_line_offset_adjusted_for_page_template() {
        // When the error is in the same template being rendered, the line
        // number should be offset by the frontmatter line count.
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_template("index.html", "line1\n{{ missing }}").ok();
        let tmpl = env.get_template("index.html").unwrap();
        let err = tmpl.render(minijinja::context! {}).unwrap_err();

        // 4 lines of frontmatter (---, two yaml lines, ---)
        let te = TemplateError::from_minijinja(&err, "index.html", 4, None);
        // minijinja reports line 2, so adjusted should be 2 + 4 = 6
        assert_eq!(te.raw_line, Some(2));
        assert_eq!(te.line, Some(6));
    }

    #[test]
    fn test_line_offset_not_adjusted_for_included_template() {
        // When the error is in a different template (e.g. _base.html),
        // the line number should NOT be adjusted.
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_template("_base.html", "{{ missing }}").ok();
        env.add_template("index.html", r#"{% extends "_base.html" %}"#).ok();
        let tmpl = env.get_template("index.html").unwrap();
        let err = tmpl.render(minijinja::context! {}).unwrap_err();

        let te = TemplateError::from_minijinja(&err, "index.html", 4, None);
        // Error is in _base.html line 1, should stay at 1 (not 1+4).
        assert_eq!(te.template_name.as_deref(), Some("_base.html"));
        assert_eq!(te.line, Some(1));
    }

    #[test]
    fn test_line_offset_zero_frontmatter_unchanged() {
        let mut env = minijinja::Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_template("plain.html", "{{ nope }}").ok();
        let tmpl = env.get_template("plain.html").unwrap();
        let err = tmpl.render(minijinja::context! {}).unwrap_err();

        let te = TemplateError::from_minijinja(&err, "plain.html", 0, None);
        assert_eq!(te.line, Some(1));
    }

    // --- Context summary tests ---

    #[test]
    fn test_context_summary_shows_keys() {
        let ctx = Value::from_iter([
            ("title", Value::from("Hello")),
            ("page", Value::from_iter([
                ("current_url", Value::from("/about")),
            ])),
        ]);
        let summary = summarize_context(&ctx);
        assert!(summary.contains("title : string"));
        assert!(summary.contains("page : map"));
    }

    #[test]
    fn test_context_summary_shows_sequence_length() {
        let items = Value::from(vec![
            Value::from("a"),
            Value::from("b"),
            Value::from("c"),
        ]);
        let ctx = Value::from_iter([("items", items)]);
        let summary = summarize_context(&ctx);
        assert!(summary.contains("items : sequence (3 items)"));
    }

    #[test]
    fn test_context_summary_empty() {
        let ctx = Value::from_iter(std::iter::empty::<(&str, Value)>());
        let summary = summarize_context(&ctx);
        assert_eq!(summary, "(empty context)");
    }

    #[test]
    fn test_console_format_includes_context() {
        let te = TemplateError {
            template_name: Some("index.html".into()),
            line: Some(3),
            raw_line: Some(3),
            kind: "UndefinedError".into(),
            short_msg: "variable `foo` is undefined".into(),
            detail: String::new(),
            context_summary: Some("title : string\npage : map (3 keys)".into()),
        };

        let formatted = te.format_console("index.html", None);
        assert!(formatted.contains("Available context:"));
        assert!(formatted.contains("title : string"));
        assert!(formatted.contains("page : map (3 keys)"));
    }

    #[test]
    fn test_html_output_includes_context() {
        let te = TemplateError {
            template_name: Some("index.html".into()),
            line: Some(3),
            raw_line: Some(3),
            kind: "UndefinedError".into(),
            short_msg: "variable `foo` is undefined".into(),
            detail: "detail here".into(),
            context_summary: Some("title : string\npage : map (3 keys)".into()),
        };

        let html = te.to_error_html("index.html", None);
        assert!(html.contains("Available Context"));
        assert!(html.contains("title : string"));
    }
}
