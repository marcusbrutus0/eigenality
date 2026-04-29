# Continue-on-Error Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow eigen to continue rendering sibling collection items when one item fails, instead of aborting the entire build.

**Architecture:** Three isolated changes — (1) fix destructive `index.html` overwrite in dev error pages, (2) make the dev dynamic page loop continue on render errors instead of aborting, (3) add a `continue_on_render_error` config flag for prod with the same filter-instead-of-abort behavior.

**Important:** `eigen::build::build()` always uses the prod `render_dynamic_page` path. The dev-specific `render_dynamic_page_dev` is only called from the dev server rebuild loop (`src/dev/rebuild.rs:259`). Integration tests via `eigen::build::build()` exercise the prod path only. Dev path changes (Tasks 1-2) get a unit test for `write_dev_error_pages` and a compile check; the continue-on-error logic is the same pattern tested thoroughly in the prod path (Task 4).

**Tech Stack:** Rust, tokio, minijinja, eyre

---

### Task 1: Remove index.html overwrite from write_dev_error_pages

**Files:**
- Modify: `src/dev/rebuild.rs:500-516`
- Test: `src/dev/rebuild.rs` (new `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing unit test**

Add a `#[cfg(test)]` module at the bottom of `src/dev/rebuild.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_write_dev_error_pages_does_not_overwrite_index() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path();

        // Pre-existing index.html with real homepage content.
        std::fs::create_dir_all(dist).unwrap();
        std::fs::write(dist.join("index.html"), "<h1>Home</h1>").unwrap();

        // Simulate an error in a completely different page.
        let output_path = std::path::PathBuf::from("posts/bad-post.html");
        write_dev_error_pages(dist, &output_path, "<h1>Error: undefined var</h1>");

        // The error page should be written to the item's own path.
        let error_page = std::fs::read_to_string(dist.join("posts/bad-post.html")).unwrap();
        assert!(error_page.contains("Error: undefined var"));

        // The sentinel should be written.
        let sentinel = std::fs::read_to_string(dist.join("_error.html")).unwrap();
        assert!(sentinel.contains("Error: undefined var"));

        // index.html must NOT be overwritten.
        let index = std::fs::read_to_string(dist.join("index.html")).unwrap();
        assert_eq!(index, "<h1>Home</h1>", "index.html should not be overwritten by unrelated error");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib dev::rebuild::tests::test_write_dev_error_pages_does_not_overwrite_index -- --nocapture 2>&1 | tail -20`
Expected: FAIL — the current code writes error HTML to `index.html`.

- [ ] **Step 3: Remove the index.html overwrite**

In `src/dev/rebuild.rs`, change `write_dev_error_pages` (lines 500-516) from:

```rust
fn write_dev_error_pages(dist_dir: &Path, output_path: &Path, error_html: &str) {
    // Ensure dist dir exists.
    let _ = std::fs::create_dir_all(dist_dir);

    // Write to the actual output path so if the user is viewing that page, they see the error.
    let full_path = dist_dir.join(output_path);
    if let Some(parent) = full_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&full_path, error_html);

    // Write to _error.html sentinel.
    let _ = std::fs::write(dist_dir.join("_error.html"), error_html);

    // Also write to index.html so the root page shows the error.
    let _ = std::fs::write(dist_dir.join("index.html"), error_html);
}
```

to:

```rust
fn write_dev_error_pages(dist_dir: &Path, output_path: &Path, error_html: &str) {
    let _ = std::fs::create_dir_all(dist_dir);

    let full_path = dist_dir.join(output_path);
    if let Some(parent) = full_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&full_path, error_html);

    let _ = std::fs::write(dist_dir.join("_error.html"), error_html);
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib dev::rebuild::tests::test_write_dev_error_pages_does_not_overwrite_index -- --nocapture 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/dev/rebuild.rs
git commit -m "fix: stop overwriting index.html when unrelated template errors in dev mode"
```

---

### Task 2: Continue-on-error in dev dynamic page loop

**Files:**
- Modify: `src/dev/rebuild.rs:545-648` (`render_dynamic_page_dev`)

Note: `render_dynamic_page_dev` requires a full `DevRenderContext` with an async runtime, template environment, data fetcher, and plugin registry. A unit test for it would be pure ceremony — the actual logic change is replacing `return Err(...)` with `continue`, which is the same pattern we test thoroughly in the prod path (Task 4). The dev path is verified manually via the dev server.

- [ ] **Step 1: Implement continue-on-error in render_dynamic_page_dev**

In `src/dev/rebuild.rs`, replace lines 545-648. The full loop body from line 545 (`let mut rendered_pages`) to the end of the function becomes:

```rust
    let mut rendered_pages = Vec::new();
    let mut error_count: usize = 0;
    let total_count = items.len();

    for (idx, item) in items.iter().enumerate() {
        // Extract slug.
        let slug = match item.get(slug_field) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            Some(_) => {
                eprintln!("  Warning: item {} has non-string slug, skipping.", idx);
                continue;
            }
            None => {
                eprintln!(
                    "  Warning: item {} missing slug field '{}', skipping.",
                    idx, slug_field
                );
                continue;
            }
        };

        let slug = slug::slugify(&slug);
        if slug.is_empty() {
            continue;
        }

        // Resolve nested data.
        let item_data = data::resolve_dynamic_page_data_for_item(
            &page.frontmatter,
            item,
            ctx.fetcher,
            Some(ctx.plugin_registry),
        )
        .await?;

        let output_path = if ctx.config.build.clean_urls {
            page.output_dir.join(&slug).join("index.html")
        } else {
            page.output_dir.join(format!("{}.html", slug))
        };
        let url_path = format!("/{}", output_path.to_string_lossy().replace('\\', "/"));

        if rendered_pages
            .iter()
            .any(|rp: &RenderedPage| rp.url_path == url_path)
        {
            bail!("Duplicate output path '{}' in '{}'", url_path, tmpl_name);
        }

        let meta = PageMeta::new(&url_path, &output_path, ctx.config, ctx.build_time);

        let tpl_ctx = context::build_page_context(
            ctx.config,
            ctx.global_data,
            &item_data,
            meta,
            Some((item_as, item)),
        );

        let rendered = match tmpl.render(&tpl_ctx) {
            Ok(html) => html,
            Err(err) => {
                let te = TemplateError::from_minijinja(
                    &err,
                    &tmpl_name,
                    page.frontmatter_line_count,
                    Some(&tpl_ctx),
                );
                let console_msg = te.format_console(&tmpl_name, Some(&slug));
                let error_html = te.to_error_html(&tmpl_name, Some(&slug));

                write_dev_error_pages(ctx.dist_dir, &output_path, &error_html);

                eprintln!("{}", console_msg);
                error_count += 1;
                continue;
            }
        };

        finalize_page_html_dev(
            DevFinalizeInput {
                rendered: &rendered,
                output_path: &output_path,
                url_path: &url_path,
                page,
                draft_label: "",
            },
            ctx,
        )
        .await?;

        rendered_pages.push(RenderedPage {
            url_path,
            is_index: false,
            is_dynamic: true,
            template_path: Some(page.template_path.display().to_string()),
        });
    }

    if error_count > 0 {
        tracing::warn!(
            "{} of {} items in '{}' failed to render",
            error_count, total_count, tmpl_name,
        );
    }

    Ok(rendered_pages)
```

Key changes from original:
- Added `error_count` and `total_count` tracking
- Replaced `return Err(eyre::eyre!(...))` with `error_count += 1; continue;`
- Added summary warning after the loop

- [ ] **Step 2: Verify it compiles and existing tests pass**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass (no integration test exercises this path, so no regressions).

- [ ] **Step 3: Commit**

```bash
git add src/dev/rebuild.rs
git commit -m "feat: continue rendering sibling items on error in dev mode"
```

---

### Task 3: Add continue_on_render_error config flag

**Files:**
- Modify: `src/config/mod.rs:61-119` (BuildConfig struct)
- Modify: `src/config/mod.rs:121-139` (Default impl)

- [ ] **Step 1: Add the field to BuildConfig**

In `src/config/mod.rs`, add after the `rate_limit` field (line 118):

```rust
    #[serde(default)]
    pub continue_on_render_error: bool,
```

- [ ] **Step 2: Add to the Default impl**

In `src/config/mod.rs`, add after `rate_limit: None,` (line 137):

```rust
            continue_on_render_error: false,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat: add continue_on_render_error config flag to BuildConfig"
```

---

### Task 4: Wire config flag into prod render_dynamic_page

**Files:**
- Modify: `src/build/render.rs:1241-1248`
- Test: `tests/integration_test.rs` (two new tests)

- [ ] **Step 1: Write test — prod aborts by default**

Add to `tests/integration_test.rs`:

```rust
#[tokio::test]
async fn test_prod_dynamic_page_aborts_on_error_by_default() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write(
        root,
        "site.toml",
        r#"
[site]
name = "Prod Abort Test"
base_url = "https://test.com"

[build]
fragments = false
minify = false
"#,
    );

    write(
        root,
        "templates/_base.html",
        "<!DOCTYPE html><html><body>{% block content %}{% endblock %}</body></html>",
    );

    write(
        root,
        "templates/posts/[post].html",
        r#"---
collection:
  file: "posts.json"
slug_field: slug
item_as: post
---
{% extends "_base.html" %}
{% block content %}
<h1>{{ post.title }}</h1>
<p>{{ post.detail.nested }}</p>
{% endblock %}"#,
    );

    write(
        root,
        "_data/posts.json",
        r#"[
            {"slug": "good", "title": "Good", "detail": {"nested": "ok"}},
            {"slug": "bad", "title": "Bad"}
        ]"#,
    );

    // Prod build (dev=false) should fail by default.
    let result = eigen::build::build(root, false, false, false).await;
    assert!(result.is_err(), "Prod build should abort on render error by default");
}
```

- [ ] **Step 2: Run the test to verify it passes (testing current behavior)**

Run: `cargo test test_prod_dynamic_page_aborts_on_error_by_default -- --nocapture 2>&1 | tail -20`
Expected: PASS — this is the existing behavior.

- [ ] **Step 3: Write failing test — prod continues with flag**

Add to `tests/integration_test.rs`:

```rust
#[tokio::test]
async fn test_prod_dynamic_page_continues_with_flag() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write(
        root,
        "site.toml",
        r#"
[site]
name = "Prod Continue Test"
base_url = "https://test.com"

[build]
fragments = false
minify = false
continue_on_render_error = true
"#,
    );

    write(
        root,
        "templates/_base.html",
        "<!DOCTYPE html><html><body>{% block content %}{% endblock %}</body></html>",
    );

    write(
        root,
        "templates/posts/[post].html",
        r#"---
collection:
  file: "posts.json"
slug_field: slug
item_as: post
---
{% extends "_base.html" %}
{% block content %}
<h1>{{ post.title }}</h1>
<p>{{ post.detail.nested }}</p>
{% endblock %}"#,
    );

    write(
        root,
        "_data/posts.json",
        r#"[
            {"slug": "good", "title": "Good", "detail": {"nested": "ok"}},
            {"slug": "bad", "title": "Bad"},
            {"slug": "also-good", "title": "Also", "detail": {"nested": "fine"}}
        ]"#,
    );

    // Prod build with flag should succeed.
    let result = eigen::build::build(root, false, false, false).await;
    assert!(result.is_ok(), "Prod build should continue with flag: {:?}", result.err());

    // Good items rendered.
    assert!(root.join("dist/posts/good.html").exists());
    assert!(root.join("dist/posts/also-good.html").exists());

    let good = fs::read_to_string(root.join("dist/posts/good.html")).unwrap();
    assert!(good.contains("<h1>Good</h1>"));

    // Bad item should NOT have a file (prod skips, doesn't write error page).
    assert!(!root.join("dist/posts/bad.html").exists(), "bad item should be skipped in prod");
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test test_prod_dynamic_page_continues_with_flag -- --nocapture 2>&1 | tail -20`
Expected: FAIL — the flag isn't wired in yet.

- [ ] **Step 5: Wire the flag into render_dynamic_page**

In `src/build/render.rs`, replace lines 1241-1248:

```rust
    // Check for errors from concurrent renders — collect all, return first.
    let mut phase_two_items: Vec<PhaseTwoResult> = Vec::with_capacity(results.len());
    for result in results {
        match result {
            Ok(p2) => phase_two_items.push(p2),
            Err(e) => return Err(e),
        }
    }
```

with:

```rust
    let mut phase_two_items: Vec<PhaseTwoResult> = Vec::with_capacity(results.len());
    let mut render_errors: Vec<eyre::Report> = Vec::new();

    for result in results {
        match result {
            Ok(p2) => phase_two_items.push(p2),
            Err(e) => {
                if ctx.config.build.continue_on_render_error {
                    render_errors.push(e);
                } else {
                    return Err(e);
                }
            }
        }
    }

    if !render_errors.is_empty() {
        tracing::warn!(
            "{} of {} items in '{}' failed to render — skipped",
            render_errors.len(),
            render_errors.len() + phase_two_items.len(),
            tmpl_name,
        );
    }
```

- [ ] **Step 6: Run all tests to verify they pass**

Run: `cargo test test_prod_dynamic_page -- --nocapture 2>&1 | tail -20`
Expected: Both `test_prod_dynamic_page_aborts_on_error_by_default` and `test_prod_dynamic_page_continues_with_flag` PASS.

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/build/render.rs tests/integration_test.rs
git commit -m "feat: wire continue_on_render_error flag into prod dynamic page rendering"
```

---

### Task 5: Write feature documentation

**Files:**
- Create: `website/docs/continue_on_render_error.md`

- [ ] **Step 1: Write the docs**

```markdown
---
title: Continue on Render Error
---

# Continue on Render Error

When a single item in a dynamic collection fails to render (e.g. one blog post has missing data), eigen can skip the broken item and continue building the rest of the collection.

## Dev Mode

In dev mode, continue-on-error is **always enabled**. When an item fails:

- The broken item's output path gets an error page showing the full error with context
- The `_error.html` sentinel is written so the dev server knows something went wrong
- A summary warning is logged: `3 of 8 items in 'posts/[post].html' failed to render`
- All other items render normally

The homepage and other templates are never overwritten by unrelated errors.

## Production Mode

In production, the default behavior is to **abort on the first render error**. This is intentional — a broken item in production usually indicates a data problem that should be fixed before deploying.

To opt into continue-on-error in production, add to `site.toml`:

```toml
[build]
continue_on_render_error = true
```

When enabled, failing items are skipped (no output file written) and a summary warning is logged. The build exits successfully with all good items rendered.

## What errors are affected

- **Template render errors** (undefined variables, filter errors, etc.) in dynamic collection items
- Static page errors still abort the build — there are no siblings to continue with
- Data fetch errors still abort — if the collection can't be loaded, there's nothing to render

## Strict mode

`UndefinedBehavior::Strict` remains the default. Errors are raised and surfaced clearly in the console and error pages. Continue-on-error changes *what happens after the error*, not whether the error is detected.
```

- [ ] **Step 2: Commit**

```bash
git add website/docs/continue_on_render_error.md
git commit -m "docs: add continue_on_render_error feature documentation"
```

---

### Task 6: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -30`
Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: No new warnings.

- [ ] **Step 3: Verify the example site builds**

Run: `cargo run -- build example_site 2>&1 | tail -10`
Expected: Builds successfully with no regressions.
