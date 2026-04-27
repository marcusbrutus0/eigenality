# Focused Template Error Context — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the full context dump on UndefinedError with a focused view that shows only the relevant part of the context tree, falling back to a highlighted full dump when extraction fails.

**Architecture:** All changes in `src/template/errors.rs`. New private functions handle expression extraction, context path walking, loop variable detection, and focused/highlighted formatting. The `from_minijinja` constructor switches from unconditional `summarize_context` to the new degradation chain. Public API unchanged.

**Tech Stack:** Rust, minijinja 2.19.0 (with `debug` feature), `regex` crate (already a dependency).

**Spec:** `docs/superpowers/specs/2026-04-13-focused-template-error-context-design.md`

---

### Task 1: Extract dotted expression path from minijinja error

**Files:**
- Modify: `src/template/errors.rs` (add `extract_expression_path` + tests)

- [ ] **Step 1: Write failing tests for expression extraction**

Add to the `#[cfg(test)] mod tests` block in `src/template/errors.rs`:

```rust
#[test]
fn test_extract_expression_path_simple() {
    // "page" -> ["page"]
    assert_eq!(extract_expression_path_from_str("page"), vec!["page"]);
}

#[test]
fn test_extract_expression_path_dotted() {
    // "page.seo.title" -> ["page", "seo", "title"]
    assert_eq!(
        extract_expression_path_from_str("page.seo.title"),
        vec!["page", "seo", "title"]
    );
}

#[test]
fn test_extract_expression_path_with_filter() {
    // "page.title | upper" -> ["page", "title"]
    assert_eq!(
        extract_expression_path_from_str("page.title | upper"),
        vec!["page", "title"]
    );
}

#[test]
fn test_extract_expression_path_with_function_call() {
    // "items.count()" -> ["items"]
    // Stops before `(` because `count` is a method call, not an attr.
    // Actually "items.count()" — the `.count` part IS a dotted access,
    // we stop at `(`. So: ["items", "count"].
    assert_eq!(
        extract_expression_path_from_str("items.count()"),
        vec!["items", "count"]
    );
}

#[test]
fn test_extract_expression_path_with_bracket() {
    // "items[0].name" -> ["items"]
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
    // "42 + foo" -> [] (starts with non-identifier)
    assert!(extract_expression_path_from_str("42 + foo").is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib template::errors::tests::test_extract_expression_path -- --nocapture`
Expected: compilation error — `extract_expression_path_from_str` not defined.

- [ ] **Step 3: Implement `extract_expression_path_from_str`**

Add this function above the `#[cfg(test)]` block in `src/template/errors.rs`:

```rust
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
        // Find the next identifier: consecutive [a-zA-Z_][a-zA-Z0-9_]*
        let ident_end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if ident_end == 0 {
            break;
        }
        segments.push(&rest[..ident_end]);
        rest = &rest[ident_end..];

        // If next char is `.` followed by an identifier char, continue.
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
```

- [ ] **Step 4: Also implement the wrapper that extracts from a `minijinja::Error`**

Add this function right after `extract_expression_path_from_str`:

```rust
/// Extract the dotted expression path from a minijinja error's debug info.
///
/// Uses `err.template_source()` + `err.range()` to get the exact expression
/// that failed, then parses it into path segments. Returns `None` if debug
/// info is unavailable.
fn extract_expression_path<'a>(
    err: &'a minijinja::Error,
    source_holder: &'a Option<String>,
) -> Option<Vec<&'a str>> {
    let source = source_holder.as_deref()?;
    let range = err.range()?;
    let expr = source.get(range)?;
    let segments = extract_expression_path_from_str(expr);
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}
```

Note: we pass `source_holder: &'a Option<String>` because `err.template_source()` returns a borrowed `&str` tied to the error's lifetime, but we need the source to outlive the segments. We'll call `err.template_source().map(|s| s.to_string())` at the call site and pass that owned `Option<String>` in.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib template::errors::tests::test_extract_expression_path -- --nocapture`
Expected: all 7 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/template/errors.rs
git commit -m "feat(errors): add expression path extraction from error debug info"
```

---

### Task 2: Walk context path and find break point

**Files:**
- Modify: `src/template/errors.rs` (add `ContextWalkResult`, `walk_context_path` + tests)

- [ ] **Step 1: Write failing tests for context walking**

Add to the test module:

```rust
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
            // parent should be the `page` map value
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
    // This shouldn't normally happen (if it resolved, there'd be no error),
    // but we handle it gracefully.
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
    // Single segment that exists — fully resolved.
    let ctx = Value::from_iter([("title", Value::from("Hello"))]);
    let result = walk_context_path(&["title"], &ctx);
    match result {
        ContextWalkResult::FullyResolved { path } => {
            assert_eq!(path, "title");
        }
        other => panic!("expected FullyResolved, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib template::errors::tests::test_walk_context -- --nocapture`
Expected: compilation error — `ContextWalkResult` and `walk_context_path` not defined.

- [ ] **Step 3: Implement `ContextWalkResult` and `walk_context_path`**

Add above the `#[cfg(test)]` block:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib template::errors::tests::test_walk_context -- --nocapture`
Expected: all 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/template/errors.rs
git commit -m "feat(errors): add context path walking to find undefined break point"
```

---

### Task 3: Loop variable detection via regex

**Files:**
- Modify: `src/template/errors.rs` (add `find_loop_collection` + tests)

- [ ] **Step 1: Write failing tests for loop detection**

Add to the test module:

```rust
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
    // `post` is the loop var, not `item`
    let source = "{% for post in posts %}{{ post.title }}{% endfor %}";
    assert_eq!(find_loop_collection("item", source), None);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib template::errors::tests::test_find_loop_collection -- --nocapture`
Expected: compilation error — `find_loop_collection` not defined.

- [ ] **Step 3: Implement `find_loop_collection`**

Add the `use regex::Regex;` to the top of the file (alongside the existing `use minijinja::Value;`). Then add the function above `#[cfg(test)]`:

```rust
/// Search the template source for a `{% for VAR in COLLECTION %}` where
/// `VAR` matches the given root variable name. Returns the collection
/// expression (e.g. `"posts"` or `"site.posts"`).
fn find_loop_collection(root_var: &str, template_source: &str) -> Option<String> {
    // Match: {% or {%- , then `for`, then the exact variable name as a whole
    // word, then `in`, then the collection (dotted identifiers).
    let pattern = format!(
        r"\{{\%-?\s*for\s+{}\s+in\s+([\w.]+)",
        regex::escape(root_var)
    );
    let re = Regex::new(&pattern).ok()?;
    re.captures(template_source)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib template::errors::tests::test_find_loop_collection -- --nocapture`
Expected: all 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/template/errors.rs
git commit -m "feat(errors): add loop variable detection via regex"
```

---

### Task 4: Format focused context output

**Files:**
- Modify: `src/template/errors.rs` (add `format_focused_context` + tests)

- [ ] **Step 1: Write failing tests for focused formatting**

Add to the test module:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib template::errors::tests::test_format_focused -- --nocapture`
Expected: compilation error — `format_focused_context` and `LoopInfo` not defined.

- [ ] **Step 3: Implement `LoopInfo` and `format_focused_context`**

Add above `#[cfg(test)]`:

```rust
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

            // If this is a loop variable, show where it comes from.
            if let Some(info) = loop_info {
                let root = path.split('.').next().unwrap_or(path);
                let _ = writeln!(
                    out, "`{root}` comes from: {{% for {root} in {} %}}",
                    info.collection_expr
                );
                // Show item shape from the collection.
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
            // Build the pointer line: spaces under the missing segment.
            let _ = writeln!(out, "Tried to access: {path}");

            // Find position of missing_segment in path for the pointer.
            if let Some(pos) = path.find(missing_segment) {
                let prefix_len = "Tried to access: ".len() + pos;
                let _ = writeln!(
                    out, "{:>width$} not found",
                    "^".repeat(missing_segment.len()),
                    width = prefix_len + missing_segment.len()
                );
            }

            // If this is a loop variable, show the loop source and item shape.
            if let Some(info) = loop_info {
                let root = path.split('.').next().unwrap_or(path);
                let _ = writeln!(out);
                let _ = writeln!(
                    out, "`{root}` comes from: {{% for {root} in {} %}}",
                    info.collection_expr
                );
                if let Some(ref collection) = info.item_shape {
                    format_collection_item_shape(collection, &mut out);
                }
            } else {
                // Show the parent's shape.
                let parent_path = &path[..path.rfind('.').unwrap_or(path.len())];
                let _ = writeln!(out);
                format_value_shape(parent_path, parent, &mut out);
            }
        }
        ContextWalkResult::FullyResolved { path } => {
            // Shouldn't happen for UndefinedError, but handle gracefully.
            let _ = writeln!(out, "Path `{path}` resolved successfully (unexpected)");
            let _ = writeln!(out);
            let _ = writeln!(out, "Top-level context keys:");
            append_top_level_keys(ctx, &mut out);
        }
    }

    // Trim trailing newline.
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

/// Show the shape of a value with a label like "`page` is a map with N keys:".
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib template::errors::tests::test_format_focused -- --nocapture`
Expected: all 3 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/template/errors.rs
git commit -m "feat(errors): add focused context formatting for walk results"
```

---

### Task 5: Format highlighted full dump (fallback)

**Files:**
- Modify: `src/template/errors.rs` (add `format_highlighted_context` + tests)

- [ ] **Step 1: Write failing tests for highlighted dump**

Add to the test module:

```rust
#[test]
fn test_format_highlighted_context_marks_branch() {
    let ctx = Value::from_iter([
        ("title", Value::from("Hello")),
        ("page", Value::from_iter([
            ("url", Value::from("/about")),
            ("base", Value::from("https://example.com")),
        ])),
        ("nav", Value::from_iter([
            ("items", Value::from(Vec::<Value>::new())),
        ])),
    ]);
    let output = format_highlighted_context(&ctx, Some("page"));
    // `page` branch should be highlighted with `>`
    assert!(output.contains("> page : map"));
    assert!(output.contains(">   url : string"));
    assert!(output.contains(">   base : string"));
    // Other keys should NOT be highlighted.
    assert!(output.contains("title : string"));
    assert!(!output.contains("> title"));
    assert!(output.contains("nav : map"));
    assert!(!output.contains("> nav"));
}

#[test]
fn test_format_highlighted_context_no_highlight() {
    let ctx = Value::from_iter([
        ("title", Value::from("Hello")),
    ]);
    let output = format_highlighted_context(&ctx, None);
    // No `>` markers, just a normal dump.
    assert!(output.contains("title : string"));
    assert!(!output.contains(">"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib template::errors::tests::test_format_highlighted_context -- --nocapture`
Expected: compilation error — `format_highlighted_context` not defined.

- [ ] **Step 3: Implement `format_highlighted_context`**

Add above `#[cfg(test)]`:

```rust
/// Render the full context shape dump, highlighting the branch matching
/// `highlight_root` with `>` prefix markers.
///
/// If `highlight_root` is `None`, produces the same output as `summarize_context`.
fn format_highlighted_context(ctx: &Value, highlight_root: Option<&str>) -> String {
    let full = summarize_context(ctx);
    let highlight_root = match highlight_root {
        Some(r) => r,
        None => return full,
    };

    let mut out = String::new();
    let mut in_highlighted_branch = false;

    for line in full.lines() {
        // A top-level key line starts with no indentation (no leading spaces).
        let is_top_level = !line.starts_with(' ');

        if is_top_level {
            // Check if this top-level key matches the highlight root.
            in_highlighted_branch = line.starts_with(highlight_root)
                && line[highlight_root.len()..].starts_with(" : ");
        }

        if in_highlighted_branch {
            out.push_str("> ");
            out.push_str(line);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }

    // Trim trailing newline for consistency.
    if out.ends_with('\n') {
        out.truncate(out.len() - 1);
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib template::errors::tests::test_format_highlighted_context -- --nocapture`
Expected: all 2 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/template/errors.rs
git commit -m "feat(errors): add highlighted full context dump fallback"
```

---

### Task 6: Wire up the degradation chain in `from_minijinja`

**Files:**
- Modify: `src/template/errors.rs` (change `from_minijinja`, add `build_context_summary`)

- [ ] **Step 1: Write failing integration test**

Add to the test module:

```rust
#[test]
fn test_from_minijinja_produces_focused_context_on_undefined() {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template("test.html", "{{ page.seo.title }}").ok();
    let tmpl = env.get_template("test.html").unwrap();

    let ctx = minijinja::context! {
        page => minijinja::context! {
            url => "/about",
            base => "https://example.com",
        },
        title => "Hello",
    };
    let err = tmpl.render(&ctx).unwrap_err();

    let te = TemplateError::from_minijinja(&err, "test.html", 0, Some(&ctx));

    let summary = te.context_summary.as_deref().expect("should have context summary");
    // Should contain focused output, not full dump.
    assert!(summary.contains("Tried to access:"), "summary was: {summary}");
    assert!(summary.contains("not found"), "summary was: {summary}");
    // Should show page's keys.
    assert!(summary.contains("url : string"), "summary was: {summary}");
}

#[test]
fn test_from_minijinja_non_undefined_skips_context() {
    let mut env = minijinja::Environment::new();
    // SyntaxError — context dump should be None.
    env.add_template("bad.html", "{{ 1 +/ 2 }}").ok();
    // add_template with a syntax error might fail at add time; let's test
    // a different error kind.  Use an unknown filter instead.
    env.add_template("filter.html", "{{ x | nonexistent }}").ok();
    let tmpl = env.get_template("filter.html").unwrap();
    let ctx = minijinja::context! { x => "hello" };
    let err = tmpl.render(&ctx).unwrap_err();

    let te = TemplateError::from_minijinja(&err, "filter.html", 0, Some(&ctx));

    // Non-UndefinedError — context_summary should be None.
    assert!(te.context_summary.is_none(),
        "expected None for non-UndefinedError, got: {:?}", te.context_summary);
}

#[test]
fn test_from_minijinja_loop_variable_shows_item_shape() {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template(
        "loop.html",
        "{% for post in posts %}{{ post.titl }}{% endfor %}"
    ).ok();
    let tmpl = env.get_template("loop.html").unwrap();

    let posts = vec![
        minijinja::context! { title => "Post 1", slug => "p1" },
    ];
    let ctx = minijinja::context! { posts => posts };
    let err = tmpl.render(&ctx).unwrap_err();

    let te = TemplateError::from_minijinja(&err, "loop.html", 0, Some(&ctx));

    let summary = te.context_summary.as_deref().expect("should have context summary");
    assert!(summary.contains("post"), "summary was: {summary}");
    // Should show the item shape from the collection.
    assert!(summary.contains("title : string"), "summary was: {summary}");
    assert!(summary.contains("slug : string"), "summary was: {summary}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib template::errors::tests::test_from_minijinja_produces -- --nocapture`
Expected: FAIL — current `from_minijinja` still produces old full-dump output.

- [ ] **Step 3: Implement `build_context_summary` and rewire `from_minijinja`**

Add above `#[cfg(test)]`:

```rust
/// Build the context summary string using the focused degradation chain.
///
/// 1. If the error is `UndefinedError` and debug info is available, try
///    focused path extraction.
/// 2. If the root is a loop variable, trace it back to the collection.
/// 3. Otherwise, fall back to a highlighted (or plain) full dump.
/// 4. For non-`UndefinedError`, return `None` (context not relevant).
fn build_context_summary(
    err: &minijinja::Error,
    ctx: &Value,
) -> Option<String> {
    use minijinja::ErrorKind;

    // Only UndefinedError benefits from context drilling.
    if err.kind() != ErrorKind::UndefinedError {
        return None;
    }

    // Try to extract the expression path from debug info.
    let source = err.template_source().map(|s| s.to_string());
    let segments = extract_expression_path(err, &source);

    let Some(segments) = segments else {
        // No debug info — fall back to full dump.
        return Some(summarize_context(ctx));
    };

    let walk = walk_context_path(&segments, ctx);

    // If top-level miss, try loop variable detection.
    let loop_info = match &walk {
        ContextWalkResult::TopLevelMiss { .. } | ContextWalkResult::NestedMiss { .. } => {
            let root = segments[0];
            // Check if root is NOT a top-level key (i.e., might be a loop var).
            let is_top_level_key = ctx.get_attr(root)
                .map(|v| !v.is_undefined())
                .unwrap_or(false);

            if !is_top_level_key {
                source.as_deref().and_then(|src| {
                    let collection_expr = find_loop_collection(root, src)?;
                    // Resolve the collection in the context.
                    let item_shape = resolve_dotted_path(&collection_expr, ctx);
                    Some(LoopInfo { collection_expr, item_shape })
                })
            } else {
                None
            }
        }
        _ => None,
    };

    // If we have a walk result and it's useful, format focused output.
    match &walk {
        ContextWalkResult::TopLevelMiss { .. } if loop_info.is_none() => {
            // Top-level miss with no loop info.
            let root = segments[0];
            // Check if the root is NOT a top-level key — it may come
            // from a loop or macro we couldn't trace.
            let is_top_level_key = ctx.get_attr(root)
                .map(|v| !v.is_undefined())
                .unwrap_or(false);
            let mut out = if !is_top_level_key {
                format!(
                    "Variable `{root}` is not in the top-level context \
                     — it may come from a loop or macro.\n\n"
                )
            } else {
                String::new()
            };
            out.push_str(&format_highlighted_context(ctx, Some(root)));
            Some(out)
        }
        ContextWalkResult::FullyResolved { .. } => {
            // Shouldn't happen — fall back to full dump.
            Some(summarize_context(ctx))
        }
        _ => {
            // Focused output (possibly with loop info).
            Some(format_focused_context(&walk, ctx, loop_info.as_ref()))
        }
    }
}

/// Resolve a dotted path like `"site.posts"` against the context.
fn resolve_dotted_path(path: &str, ctx: &Value) -> Option<Value> {
    let mut current = ctx.clone();
    for segment in path.split('.') {
        current = match current.get_attr(segment) {
            Ok(val) if !val.is_undefined() => val,
            _ => return None,
        };
    }
    Some(current)
}
```

Now update `from_minijinja` to use the new function. Replace line 86:

```rust
// OLD:
let context_summary = tpl_ctx.map(summarize_context);

// NEW:
let context_summary = tpl_ctx.and_then(|ctx| build_context_summary(err, ctx));
```

**Important:** this requires changing the function to use the original `err` reference before it gets shadowed by the error-chain loop on line 79. Move the `build_context_summary` call before the error-chain loop, or capture the original error in a separate binding.

The cleanest fix: rename the loop variable from `err` to `source_err` to avoid shadowing. Replace lines 79-84:

```rust
let mut source_err = &err as &dyn std::error::Error;
while let Some(next_err) = source_err.source() {
    detail.push_str("\n\n");
    detail.push_str(&format!("caused by: {:#}", next_err));
    source_err = next_err;
}
```

And line 86 becomes:

```rust
let context_summary = tpl_ctx.and_then(|ctx| build_context_summary(err, ctx));
```

- [ ] **Step 4: Run all tests**

Run: `cargo test --lib template::errors -- --nocapture`
Expected: all tests PASS (new integration tests + existing tests).

- [ ] **Step 5: Verify existing tests still pass**

Some existing tests construct `TemplateError` manually with `context_summary` set to old-style full dumps. These test `format_console` and `to_error_html` — they should still pass because those functions just render whatever string is in `context_summary`. Verify:

Run: `cargo test --lib template::errors::tests::test_console_format_includes_context -- --nocapture`
Run: `cargo test --lib template::errors::tests::test_html_output_includes_context -- --nocapture`
Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add src/template/errors.rs
git commit -m "feat(errors): wire focused context into from_minijinja degradation chain"
```

---

### Task 7: Update docs and clean up

**Files:**
- Modify: `docs/template_error_reporting.md`

- [ ] **Step 1: Update the docs**

Replace the "Context Shape Dump" section in `docs/template_error_reporting.md` with the new behavior. The updated section:

```markdown
## Focused Context on UndefinedError

When an `UndefinedError` occurs, the error output shows a **focused view** of
the context — only the part relevant to the undefined access, not the entire
context tree.

### How it works

1. The failing expression is extracted from minijinja's debug info (e.g.
   `page.seo.title`).
2. The expression is walked through the context to find where it breaks.
3. The error output shows what was attempted, which segment was missing, and
   what the parent value actually contains.

### Focused output (nested miss)

```
  Tried to access: page.seo.title
                        ^^^ not found

  `page` is a map with 4 keys:
    current_url : string
    current_path : string
    base_url : string
    build_time : string
```

### Loop variables

When the undefined variable comes from a `{% for %}` loop, the error traces
back to the collection and shows the item shape:

```
  Tried to access: post.titl
                        ^^^^ not found

  `post` comes from: {% for post in posts %}
  Each item is a map with 4 keys:
    title : string
    slug : string
    date : string
    tags : sequence (3 items)
```

### Fallback

When focused extraction isn't possible (no debug info, non-identifier
expressions), the full context dump is shown with the relevant branch
highlighted using `>` markers:

```
  Available context:
    title : string
  > page : map (4 keys)
  >   current_url : string
  >   current_path : string
  >   base_url : string
  >   build_time : string
    nav : map (3 keys)
      ...
```

For non-`UndefinedError` errors (SyntaxError, InvalidOperation, etc.), the
context dump is omitted entirely — the error message and source snippet are
sufficient.

Maps list all their keys recursively. Sequences sample the first element to
infer the item shape. Nesting stops at 4 levels deep. Values are never shown
(collections can be large) — only the structure.
```

- [ ] **Step 2: Run the full test suite**

Run: `cargo test`
Expected: all tests PASS.

- [ ] **Step 3: Commit**

```bash
git add docs/template_error_reporting.md
git commit -m "docs: update template error reporting for focused context"
```
