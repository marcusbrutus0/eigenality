//! Template error formatting utilities.
//!
//! Provides detailed, human-readable error messages for minijinja template
//! rendering failures, including the template name, line number, error kind,
//! a source context snippet, and a summary of the template context when available.

use minijinja::Value;
use regex::Regex;

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

/// Maximum nesting depth for the recursive context shape dump.
const SHAPE_MAX_DEPTH: usize = 4;

/// Build a recursive shape dump of the template context, showing variable
/// names, types, and nested structure (but never values).
///
/// Maps show all their keys; sequences sample the first element to infer
/// the item shape. Recursion stops at [`SHAPE_MAX_DEPTH`].
fn summarize_context(ctx: &Value) -> String {
    let mut out = String::new();
    if let Ok(keys) = ctx.try_iter() {
        let mut any = false;
        for key in keys {
            any = true;
            let name = key.as_str().unwrap_or("?");
            match ctx.get_attr(name) {
                Ok(val) => describe_shape(name, &val, 0, &mut out),
                Err(_) => {
                    out.push_str(name);
                    out.push_str(" : ?\n");
                }
            }
        }
        if !any {
            return "(empty context)".into();
        }
    }
    // Trim trailing newline for consistency with callers that append their own.
    if out.ends_with('\n') {
        out.truncate(out.len() - 1);
    }
    if out.is_empty() {
        "(empty context)".into()
    } else {
        out
    }
}

/// Append a description of `val` at the given indentation `depth`.
///
/// For maps, each key is listed recursively. For sequences, the first
/// element is sampled to show the item shape. Primitive kinds are shown
/// on a single line. Recursion stops at `SHAPE_MAX_DEPTH`.
fn describe_shape(name: &str, val: &Value, depth: usize, out: &mut String) {
    use std::fmt::Write;
    use minijinja::value::ValueKind;

    fn push_indent(out: &mut String, depth: usize) {
        for _ in 0..depth {
            out.push_str("  ");
        }
    }

    let kind = val.kind();

    match kind {
        ValueKind::Map => {
            // Single iteration: collect keys and recurse.
            let keys: Vec<_> = val
                .try_iter()
                .map(|i| i.collect())
                .unwrap_or_default();
            push_indent(out, depth);
            let _ = writeln!(out, "{name} : map ({} keys)", keys.len());

            if depth < SHAPE_MAX_DEPTH {
                for k in &keys {
                    let k_str = k.as_str().unwrap_or("?");
                    if let Ok(child) = val.get_attr(k_str) {
                        describe_shape(k_str, &child, depth + 1, out);
                    }
                }
            }
        }
        ValueKind::Seq => {
            // Single iteration: grab the first element and count the rest.
            let (len, first) = match val.try_iter() {
                Ok(mut iter) => {
                    let first = iter.next();
                    let rest = iter.count();
                    let total = if first.is_some() { rest + 1 } else { 0 };
                    (total, first)
                }
                Err(_) => (0, None),
            };
            push_indent(out, depth);
            let _ = writeln!(out, "{name} : sequence ({len} items)");

            if depth < SHAPE_MAX_DEPTH {
                if let Some(first) = first {
                    describe_shape("[item]", &first, depth + 1, out);
                }
            }
        }
        _ => {
            push_indent(out, depth);
            let _ = writeln!(out, "{name} : {kind}");
        }
    }
}

/// Extract a dotted identifier path from an expression string.
///
/// Scans left-to-right collecting `identifier.identifier.identifier`,
/// stopping at the first non-identifier character (`|`, `(`, `[`, space
/// not followed by `.`, etc.). Returns an empty vec if the string doesn't
/// start with a valid identifier.
fn extract_expression_path_from_str(expr: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut rest = expr.trim();

    loop {
        // Identifiers must start with a letter or underscore (not a digit).
        if !rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            break;
        }
        let ident_end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        segments.push(&rest[..ident_end]);
        rest = &rest[ident_end..];

        if rest.starts_with('.') {
            let after_dot = &rest[1..];
            if after_dot.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                rest = after_dot;
                continue;
            }
        }
        break;
    }
    segments
}

/// Extract the dotted expression path from a minijinja error's debug info.
///
/// `source` should come from `err.template_source()`. Combined with
/// `err.range()`, the failing expression is sliced and parsed into path
/// segments. Returns `None` if either is unavailable.
fn extract_expression_path<'a>(
    err: &minijinja::Error,
    source: Option<&'a str>,
) -> Option<Vec<&'a str>> {
    let source = source?;
    let range = err.range()?;
    let expr = source.get(range)?;
    let segments = extract_expression_path_from_str(expr);
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

/// Result of walking a dotted path through the template context.
#[derive(Debug)]
enum ContextWalkResult {
    /// The root segment was not found in the top-level context.
    TopLevelMiss {
        path: String,
    },
    /// A nested segment was not found. `parent` is the last value that
    /// resolved successfully.
    NestedMiss {
        path: String,
        missing_segment: String,
        parent: Value,
    },
    /// Every segment resolved (shouldn't happen for an UndefinedError, but
    /// handled gracefully).
    FullyResolved {
        path: String,
    },
}

/// Walk a dotted path through the template context and find where it breaks.
///
/// `segments` is something like `["page", "seo", "title"]`. We look up each
/// segment in turn, tracking the last successfully resolved value.
fn walk_context_path(segments: &[&str], ctx: &Value) -> ContextWalkResult {
    let path = segments.join(".");

    if segments.is_empty() {
        return ContextWalkResult::FullyResolved { path };
    }

    // Try the root segment in the top-level context.
    let root = segments[0];
    let mut current = match ctx.get_attr(root) {
        Ok(val) if !val.is_undefined() => val,
        _ => return ContextWalkResult::TopLevelMiss { path },
    };

    // Walk remaining segments.
    for &segment in &segments[1..] {
        match current.get_attr(segment) {
            Ok(val) if !val.is_undefined() => {
                current = val;
            }
            _ => {
                return ContextWalkResult::NestedMiss {
                    path,
                    missing_segment: segment.to_string(),
                    parent: current,
                };
            }
        }
    }

    ContextWalkResult::FullyResolved { path }
}

/// Search the template source for a `{% for VAR in COLLECTION %}` where
/// `VAR` matches the given root variable name. Returns the collection
/// expression (e.g. `"posts"` or `"site.posts"`).
fn find_loop_collection(root_var: &str, template_source: &str) -> Option<String> {
    let pattern = format!(
        r"\{{\%-?\s*for\s+{}\s+in\s+([\w.]+)",
        regex::escape(root_var)
    );
    let re = Regex::new(&pattern).ok()?;
    re.captures(template_source)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Simple HTML escaping for inserting text into HTML.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Information about a loop variable traced back to its collection.
struct LoopInfo {
    /// The collection expression from the template (e.g. `"posts"` or `"site.posts"`).
    collection_expr: String,
    /// The collection value from the context (used to show item shape).
    item_shape: Option<Value>,
}

/// Format a focused context message for a `ContextWalkResult`.
///
/// For `TopLevelMiss`: shows the access path and lists top-level context keys.
/// For `NestedMiss`: shows the access path, which segment was missing, and the
/// parent's shape. If `loop_info` is provided (for loop variables), shows the
/// loop source and item shape instead of top-level keys.
fn format_focused_context(
    walk: &ContextWalkResult,
    ctx: &Value,
    loop_info: Option<&LoopInfo>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    match walk {
        ContextWalkResult::TopLevelMiss { path } => {
            let _ = writeln!(out, "Tried to access: {path}");

            if let Some(info) = loop_info {
                let root = path.split('.').next().unwrap_or(path);
                let _ = writeln!(
                    out,
                    "`{root}` comes from: {{% for {root} in {} %}}",
                    info.collection_expr
                );
                if let Some(ref collection) = info.item_shape {
                    format_collection_item_shape(collection, &mut out);
                }
            } else {
                let _ = writeln!(out);
                let _ = writeln!(out, "Top-level context keys:");
                append_top_level_keys(ctx, &mut out);
            }
        }
        ContextWalkResult::NestedMiss { path, missing_segment, parent } => {
            let _ = writeln!(out, "Tried to access: {path}");

            if let Some(pos) = path.find(missing_segment.as_str()) {
                let prefix_len = "Tried to access: ".len() + pos;
                let _ = writeln!(
                    out,
                    "{:>width$} not found",
                    "^".repeat(missing_segment.len()),
                    width = prefix_len + missing_segment.len()
                );
            }

            if let Some(info) = loop_info {
                let root = path.split('.').next().unwrap_or(path);
                let _ = writeln!(out);
                let _ = writeln!(
                    out,
                    "`{root}` comes from: {{% for {root} in {} %}}",
                    info.collection_expr
                );
                if let Some(ref collection) = info.item_shape {
                    format_collection_item_shape(collection, &mut out);
                }
            } else {
                // Build the parent label from the path segments that resolved
                // before the missing segment, not from rfind which would pick
                // the last dot and land on the wrong segment.
                let parent_path: String = path
                    .split('.')
                    .take_while(|s| *s != missing_segment.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let _ = writeln!(out);
                format_value_shape(&parent_path, parent, &mut out);
            }
        }
        ContextWalkResult::FullyResolved { path } => {
            let _ = writeln!(out, "Path `{path}` resolved successfully (unexpected)");
            let _ = writeln!(out);
            let _ = writeln!(out, "Top-level context keys:");
            append_top_level_keys(ctx, &mut out);
        }
    }

    if out.ends_with('\n') {
        out.truncate(out.len() - 1);
    }
    out
}

/// Append all top-level context key names and types.
fn append_top_level_keys(ctx: &Value, out: &mut String) {
    if let Ok(keys) = ctx.try_iter() {
        for key in keys {
            let name = key.as_str().unwrap_or("?");
            match ctx.get_attr(name) {
                Ok(val) => describe_shape(name, &val, 0, out),
                Err(_) => {
                    out.push_str(name);
                    out.push_str(" : ?\n");
                }
            }
        }
    }
}

/// Show the shape of a value with a label like `` `page` is a map with N keys: ``.
fn format_value_shape(label: &str, val: &Value, out: &mut String) {
    use std::fmt::Write;
    use minijinja::value::ValueKind;

    match val.kind() {
        ValueKind::Map => {
            let keys: Vec<_> = val.try_iter()
                .map(|i| i.collect())
                .unwrap_or_default();
            let _ = writeln!(out, "`{label}` is a map with {} keys:", keys.len());
            for k in &keys {
                let k_str = k.as_str().unwrap_or("?");
                if let Ok(child) = val.get_attr(k_str) {
                    describe_shape(k_str, &child, 0, out);
                }
            }
        }
        ValueKind::Seq => {
            let len = val.try_iter().map(|i| i.count()).unwrap_or(0);
            let _ = writeln!(out, "`{label}` is a sequence with {len} items");
            if let Ok(mut iter) = val.try_iter() {
                if let Some(first) = iter.next() {
                    describe_shape("[item]", &first, 0, out);
                }
            }
        }
        other => {
            let _ = writeln!(out, "`{label}` is a {other}");
        }
    }
}

/// Show the item shape of a collection (sequence) value.
fn format_collection_item_shape(collection: &Value, out: &mut String) {
    use std::fmt::Write;
    use minijinja::value::ValueKind;

    if collection.kind() == ValueKind::Seq {
        if let Ok(mut iter) = collection.try_iter() {
            if let Some(first) = iter.next() {
                let _ = writeln!(out);
                match first.kind() {
                    ValueKind::Map => {
                        let keys: Vec<_> = first.try_iter()
                            .map(|i| i.collect())
                            .unwrap_or_default();
                        let _ = writeln!(out, "Each item is a map with {} keys:", keys.len());
                        for k in &keys {
                            let k_str = k.as_str().unwrap_or("?");
                            if let Ok(child) = first.get_attr(k_str) {
                                describe_shape(k_str, &child, 0, out);
                            }
                        }
                    }
                    other => {
                        let _ = writeln!(out, "Each item is a {other}");
                    }
                }
            }
        }
    }
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
    fn test_context_summary_shows_keys_and_nested_shape() {
        let ctx = Value::from_iter([
            ("title", Value::from("Hello")),
            ("page", Value::from_iter([
                ("current_url", Value::from("/about")),
                ("base_url", Value::from("https://example.com")),
            ])),
        ]);
        let summary = summarize_context(&ctx);
        assert!(summary.contains("title : string"));
        assert!(summary.contains("page : map (2 keys)"));
        // Nested keys should appear indented.
        assert!(summary.contains("  current_url : string"));
        assert!(summary.contains("  base_url : string"));
    }

    #[test]
    fn test_context_summary_sequence_shows_item_shape() {
        let items = Value::from(vec![
            Value::from_iter([
                ("title", Value::from("Post 1")),
                ("slug", Value::from("post-1")),
            ]),
            Value::from_iter([
                ("title", Value::from("Post 2")),
                ("slug", Value::from("post-2")),
            ]),
        ]);
        let ctx = Value::from_iter([("posts", items)]);
        let summary = summarize_context(&ctx);
        assert!(summary.contains("posts : sequence (2 items)"));
        // First item sampled to show shape.
        assert!(summary.contains("  [item] : map (2 keys)"));
        assert!(summary.contains("    title : string"));
        assert!(summary.contains("    slug : string"));
    }

    #[test]
    fn test_context_summary_sequence_of_primitives() {
        let items = Value::from(vec![
            Value::from("a"),
            Value::from("b"),
            Value::from("c"),
        ]);
        let ctx = Value::from_iter([("tags", items)]);
        let summary = summarize_context(&ctx);
        assert!(summary.contains("tags : sequence (3 items)"));
        assert!(summary.contains("  [item] : string"));
    }

    #[test]
    fn test_context_summary_empty() {
        let ctx = Value::from_iter(std::iter::empty::<(&str, Value)>());
        let summary = summarize_context(&ctx);
        assert_eq!(summary, "(empty context)");
    }

    #[test]
    fn test_context_summary_empty_sequence() {
        let items = Value::from(Vec::<Value>::new());
        let ctx = Value::from_iter([("items", items)]);
        let summary = summarize_context(&ctx);
        // Empty sequence — no item shape to sample.
        assert!(summary.contains("items : sequence (0 items)"));
        assert!(!summary.contains("[item]"));
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
            context_summary: Some("title : string\npage : map (3 keys)\n  current_url : string".into()),
        };

        let formatted = te.format_console("index.html", None);
        assert!(formatted.contains("Available context:"));
        assert!(formatted.contains("title : string"));
        assert!(formatted.contains("page : map (3 keys)"));
    }

    // --- Expression path extraction tests ---

    #[test]
    fn test_extract_expression_path_simple() {
        assert_eq!(extract_expression_path_from_str("page"), vec!["page"]);
    }

    #[test]
    fn test_extract_expression_path_dotted() {
        assert_eq!(
            extract_expression_path_from_str("page.seo.title"),
            vec!["page", "seo", "title"]
        );
    }

    #[test]
    fn test_extract_expression_path_with_filter() {
        assert_eq!(
            extract_expression_path_from_str("page.title | upper"),
            vec!["page", "title"]
        );
    }

    #[test]
    fn test_extract_expression_path_with_function_call() {
        assert_eq!(
            extract_expression_path_from_str("items.count()"),
            vec!["items", "count"]
        );
    }

    #[test]
    fn test_extract_expression_path_with_bracket() {
        assert_eq!(
            extract_expression_path_from_str("items[0].name"),
            vec!["items"]
        );
    }

    #[test]
    fn test_extract_expression_path_empty() {
        assert!(extract_expression_path_from_str("").is_empty());
    }

    #[test]
    fn test_extract_expression_path_non_identifier_start() {
        assert!(extract_expression_path_from_str("42 + foo").is_empty());
    }

    // --- Context path walking tests ---

    #[test]
    fn test_walk_context_top_level_miss() {
        let ctx = Value::from_iter([
            ("title", Value::from("Hello")),
            ("page", Value::from_iter([
                ("url", Value::from("/about")),
            ])),
        ]);
        let result = walk_context_path(&["missing"], &ctx);
        match result {
            ContextWalkResult::TopLevelMiss { path } => {
                assert_eq!(path, "missing");
            }
            other => panic!("expected TopLevelMiss, got {:?}", other),
        }
    }

    #[test]
    fn test_walk_context_nested_miss() {
        let ctx = Value::from_iter([
            ("page", Value::from_iter([
                ("url", Value::from("/about")),
                ("base", Value::from("https://example.com")),
            ])),
        ]);
        let result = walk_context_path(&["page", "seo", "title"], &ctx);
        match result {
            ContextWalkResult::NestedMiss { path, missing_segment, parent } => {
                assert_eq!(path, "page.seo.title");
                assert_eq!(missing_segment, "seo");
                assert!(parent.kind() == minijinja::value::ValueKind::Map);
            }
            other => panic!("expected NestedMiss, got {:?}", other),
        }
    }

    #[test]
    fn test_walk_context_fully_resolved() {
        let ctx = Value::from_iter([
            ("page", Value::from_iter([
                ("title", Value::from("Hello")),
            ])),
        ]);
        let result = walk_context_path(&["page", "title"], &ctx);
        match result {
            ContextWalkResult::FullyResolved { path } => {
                assert_eq!(path, "page.title");
            }
            other => panic!("expected FullyResolved, got {:?}", other),
        }
    }

    #[test]
    fn test_walk_context_single_segment_hit() {
        let ctx = Value::from_iter([("title", Value::from("Hello"))]);
        let result = walk_context_path(&["title"], &ctx);
        match result {
            ContextWalkResult::FullyResolved { path } => {
                assert_eq!(path, "title");
            }
            other => panic!("expected FullyResolved, got {:?}", other),
        }
    }

    // --- Loop variable detection tests ---

    #[test]
    fn test_find_loop_collection_simple() {
        let source = "{% for post in posts %}{{ post.title }}{% endfor %}";
        assert_eq!(
            find_loop_collection("post", source).as_deref(),
            Some("posts")
        );
    }

    #[test]
    fn test_find_loop_collection_dotted() {
        let source = "{% for item in site.posts %}{{ item.title }}{% endfor %}";
        assert_eq!(
            find_loop_collection("item", source).as_deref(),
            Some("site.posts")
        );
    }

    #[test]
    fn test_find_loop_collection_whitespace_control() {
        let source = "{%- for tag in tags -%}{{ tag }}{% endfor %}";
        assert_eq!(
            find_loop_collection("tag", source).as_deref(),
            Some("tags")
        );
    }

    #[test]
    fn test_find_loop_collection_no_match() {
        let source = "{% if show %}{{ missing }}{% endif %}";
        assert_eq!(find_loop_collection("missing", source), None);
    }

    #[test]
    fn test_find_loop_collection_different_var() {
        let source = "{% for post in posts %}{{ post.title }}{% endfor %}";
        assert_eq!(find_loop_collection("item", source), None);
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
            context_summary: Some("title : string\npage : map (3 keys)\n  base_url : string".into()),
        };

        let html = te.to_error_html("index.html", None);
        assert!(html.contains("Available Context"));
        assert!(html.contains("title : string"));
    }

    // --- Focused context formatting tests ---

    #[test]
    fn test_format_focused_top_level_miss() {
        let ctx = Value::from_iter([
            ("title", Value::from("Hello")),
            ("page", Value::from_iter([
                ("url", Value::from("/about")),
            ])),
        ]);
        let result = walk_context_path(&["missing"], &ctx);
        let output = format_focused_context(&result, &ctx, None);
        assert!(output.contains("Tried to access: missing"));
        // Should list top-level keys
        assert!(output.contains("title : string"));
        assert!(output.contains("page : map"));
    }

    #[test]
    fn test_format_focused_nested_miss() {
        let ctx = Value::from_iter([
            ("page", Value::from_iter([
                ("url", Value::from("/about")),
                ("base", Value::from("https://example.com")),
            ])),
        ]);
        let result = walk_context_path(&["page", "seo", "title"], &ctx);
        let output = format_focused_context(&result, &ctx, None);
        assert!(output.contains("Tried to access: page.seo.title"));
        assert!(output.contains("seo"));
        assert!(output.contains("not found"));
        // Should show page's actual keys
        assert!(output.contains("`page` is a map with 2 keys"));
        assert!(output.contains("url : string"));
        assert!(output.contains("base : string"));
    }

    #[test]
    fn test_format_focused_loop_variable() {
        let posts = Value::from(vec![
            Value::from_iter([
                ("title", Value::from("Post 1")),
                ("slug", Value::from("post-1")),
            ]),
        ]);
        let ctx = Value::from_iter([("posts", posts)]);
        let result = walk_context_path(&["post", "titl"], &ctx);
        let loop_info = Some(LoopInfo {
            collection_expr: "posts".to_string(),
            item_shape: ctx.get_attr("posts").ok(),
        });
        let output = format_focused_context(&result, &ctx, loop_info.as_ref());
        assert!(output.contains("Tried to access: post.titl"));
        assert!(output.contains("`post` comes from: {% for post in posts %}"));
        assert!(output.contains("title : string"));
        assert!(output.contains("slug : string"));
    }
}
