# Clean Links Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow developers to use clean URLs (e.g. `/about` instead of `/about.html`) in templates and have them work both in production (Cloudflare) and the local dev server.

**Architecture:** A shared `to_clean_link()` utility function strips `.html` extensions from paths. It is wired into `link_to()`, `page.current_url`, and sitemap generation via a new `build.clean_links` config flag. The dev server gets an always-on `.html` fallback middleware independent of the config.

**Tech Stack:** Rust, Axum, tower-http, minijinja

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `src/config/mod.rs` | Modify | Add `clean_links` field to `BuildConfig` |
| `src/build/clean_link.rs` | Create | `to_clean_link()` utility function + tests |
| `src/build/mod.rs` | Modify | Add `pub mod clean_link;` |
| `src/template/functions.rs` | Modify | Wire `clean_links` into `link_to()` |
| `src/build/render.rs` | Modify | Clean `page.current_url` when `clean_links` is on |
| `src/build/sitemap.rs` | Modify | Use `to_clean_link()` when `clean_links` is on |
| `src/dev/server.rs` | Modify | Add `.html` fallback to `ServeDir` |
| `tests/integration_test.rs` | Modify | Add integration tests |

---

### Task 1: Add `clean_links` to `BuildConfig`

**Files:**
- Modify: `src/config/mod.rs:54-101` (BuildConfig struct)
- Modify: `src/config/mod.rs:103-120` (BuildConfig Default impl)

- [ ] **Step 1: Add the field to `BuildConfig` struct**

In `src/config/mod.rs`, add `clean_links` field after `clean_urls` (line 78):

```rust
    /// Whether to strip `.html` extensions from generated links.
    /// When enabled, `link_to()` emits `/about` instead of `/about.html`,
    /// `page.current_url` is cleaned, and sitemap URLs use clean paths.
    /// Designed for deployment targets like Cloudflare that resolve
    /// `/about` to `about.html` automatically. Default: false.
    #[serde(default)]
    pub clean_links: bool,
```

- [ ] **Step 2: Add the field to `Default` impl**

In `src/config/mod.rs`, add to the `Default` impl (after `clean_urls: false,` around line 111):

```rust
            clean_links: false,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: Compiler may show warnings about unused field, but no errors.

- [ ] **Step 4: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat(config): add build.clean_links option"
```

---

### Task 2: Create `to_clean_link()` utility

**Files:**
- Create: `src/build/clean_link.rs`
- Modify: `src/build/mod.rs`

- [ ] **Step 1: Add module declaration**

In `src/build/mod.rs`, add:

```rust
pub mod clean_link;
```

- [ ] **Step 2: Write failing tests**

Create `src/build/clean_link.rs` with tests first:

```rust
//! Clean link URL transformation.
//!
//! Strips `.html` extensions from URL paths for clean link generation.

/// Strip `.html` extension from a URL path, producing a clean link.
///
/// - `/about.html` → `/about`
/// - `/posts/my-post.html` → `/posts/my-post`
/// - `/about/index.html` → `/about`
/// - `/index.html` → `/`
/// - `/` → `/`
/// - Non-`.html` paths pass through unchanged.
pub fn to_clean_link(path: &str) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_html() {
        assert_eq!(to_clean_link("/about.html"), "/about");
    }

    #[test]
    fn test_nested_html() {
        assert_eq!(to_clean_link("/posts/my-post.html"), "/posts/my-post");
    }

    #[test]
    fn test_index_in_directory() {
        assert_eq!(to_clean_link("/about/index.html"), "/about");
    }

    #[test]
    fn test_root_index() {
        assert_eq!(to_clean_link("/index.html"), "/");
    }

    #[test]
    fn test_root_slash() {
        assert_eq!(to_clean_link("/"), "/");
    }

    #[test]
    fn test_non_html_passthrough() {
        assert_eq!(to_clean_link("/style.css"), "/style.css");
    }

    #[test]
    fn test_no_extension_passthrough() {
        assert_eq!(to_clean_link("/about"), "/about");
    }

    #[test]
    fn test_deeply_nested() {
        assert_eq!(to_clean_link("/blog/2024/post.html"), "/blog/2024/post");
    }

    #[test]
    fn test_nested_index() {
        assert_eq!(to_clean_link("/blog/posts/index.html"), "/blog/posts");
    }

    #[test]
    fn test_404_html() {
        assert_eq!(to_clean_link("/404.html"), "/404");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p eigen --lib clean_link 2>&1 | tail -5`
Expected: FAIL — `not yet implemented`

- [ ] **Step 4: Implement `to_clean_link()`**

Replace the `todo!()` body with:

```rust
pub fn to_clean_link(path: &str) -> String {
    // Strip /index.html suffix → parent directory path.
    if let Some(prefix) = path.strip_suffix("/index.html") {
        return if prefix.is_empty() {
            "/".to_string()
        } else {
            prefix.to_string()
        };
    }

    // Strip .html extension.
    if let Some(without_ext) = path.strip_suffix(".html") {
        return without_ext.to_string();
    }

    // Non-.html paths pass through unchanged.
    path.to_string()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p eigen --lib clean_link 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/build/clean_link.rs src/build/mod.rs
git commit -m "feat: add to_clean_link() utility function"
```

---

### Task 3: Wire `clean_links` into `link_to()`

**Files:**
- Modify: `src/template/functions.rs:17-51` (register_functions and link_to closure)

- [ ] **Step 1: Write failing tests**

Add these tests to `src/template/functions.rs` in the existing `tests` module (after the last test):

```rust
    #[test]
    fn test_link_to_clean_links_enabled() {
        let mut env = Environment::new();
        let mut config = test_config();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None);

        env.add_template("test", r##"<a {{ link_to("/about.html") }}>About</a>"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"href="/about""##));
        assert!(result.contains(r##"hx-get="/_fragments/about.html""##));
        assert!(result.contains(r##"hx-push-url="/about""##));
    }

    #[test]
    fn test_link_to_clean_links_index() {
        let mut env = Environment::new();
        let mut config = test_config();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None);

        env.add_template("test", r##"{{ link_to("/index.html") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"href="/""##));
        assert!(result.contains(r##"hx-push-url="/""##));
    }

    #[test]
    fn test_link_to_clean_links_already_clean() {
        let mut env = Environment::new();
        let mut config = test_config();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None);

        env.add_template("test", r##"{{ link_to("/about") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"href="/about""##));
        assert!(result.contains(r##"hx-push-url="/about""##));
    }

    #[test]
    fn test_link_to_clean_links_no_fragments() {
        let mut env = Environment::new();
        let mut config = test_config_no_fragments();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None);

        env.add_template("test", r##"{{ link_to("/about.html") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert_eq!(result.trim(), r##"href="/about""##);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p eigen --lib functions::tests::test_link_to_clean 2>&1 | tail -10`
Expected: FAIL — href still contains `.html`

- [ ] **Step 3: Wire `clean_links` into `link_to()`**

In `src/template/functions.rs`, modify the `register_functions` function.

Add at the top of `register_functions` (after line 25, `let content_block = ...`):

```rust
    let clean_links = config.build.clean_links;
```

Add a `use` import at the top of the file:

```rust
use crate::build::clean_link::to_clean_link;
```

Modify the `link_to` closure (lines 30-50). Replace the entire closure body:

```rust
    env.add_function(
        "link_to",
        move |path: &str,
              target: Option<&str>,
              block: Option<&str>|
              -> String {
            let target = target.unwrap_or("#content");

            let display_path = if clean_links {
                to_clean_link(path)
            } else {
                path.to_string()
            };

            if !fragments_enabled {
                return format!(r#"href="{}""#, display_path);
            }

            let block_name = block.unwrap_or(&content_block);
            let fragment_path = compute_fragment_path(path, &fragment_dir, block_name);

            format!(
                r#"href="{path}" hx-get="{fragment_path}" hx-target="{target}" hx-push-url="{path}""#,
                path = display_path,
                fragment_path = fragment_path,
                target = target,
            )
        },
    );
```

Note: `compute_fragment_path` still receives the original `path` (not `display_path`) because fragment files always have `.html` extensions on disk.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p eigen --lib functions 2>&1 | tail -10`
Expected: all tests pass (both old and new)

- [ ] **Step 5: Commit**

```bash
git add src/template/functions.rs
git commit -m "feat: link_to() emits clean URLs when clean_links is enabled"
```

---

### Task 4: Clean `page.current_url`

**Files:**
- Modify: `src/build/render.rs:399-404` (static page PageMeta)
- Modify: `src/build/render.rs:712-717` (dynamic page PageMeta)

- [ ] **Step 1: Add import**

At the top of `src/build/render.rs`, add:

```rust
use super::clean_link::to_clean_link;
```

- [ ] **Step 2: Modify static page PageMeta construction**

At `src/build/render.rs:399-404`, change:

```rust
    let meta = PageMeta {
        current_url: url_path.clone(),
        current_path: output_path.to_string_lossy().to_string(),
        base_url: config.site.base_url.clone(),
        build_time: build_time.to_string(),
    };
```

to:

```rust
    let current_url = if config.build.clean_links {
        to_clean_link(&url_path)
    } else {
        url_path.clone()
    };

    let meta = PageMeta {
        current_url,
        current_path: output_path.to_string_lossy().to_string(),
        base_url: config.site.base_url.clone(),
        build_time: build_time.to_string(),
    };
```

- [ ] **Step 3: Modify dynamic page PageMeta construction**

At `src/build/render.rs:712-717`, apply the same pattern:

```rust
    let current_url = if config.build.clean_links {
        to_clean_link(&url_path)
    } else {
        url_path.clone()
    };

    let meta = PageMeta {
        current_url,
        current_path: output_path.to_string_lossy().to_string(),
        base_url: config.site.base_url.clone(),
        build_time: build_time.to_string(),
    };
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -5`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add src/build/render.rs
git commit -m "feat: page.current_url uses clean links when enabled"
```

---

### Task 5: Wire `clean_links` into sitemap

**Files:**
- Modify: `src/build/sitemap.rs:16-59` (generate_sitemap function)

- [ ] **Step 1: Write failing test**

Add this test to the existing `tests` module in `src/build/sitemap.rs`:

```rust
    #[test]
    fn test_sitemap_clean_links() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();

        let mut config = test_config();
        config.build.clean_links = true;

        let pages = vec![
            RenderedPage {
                url_path: "/index.html".into(),
                is_index: true,
                is_dynamic: false,
                template_path: None,
            },
            RenderedPage {
                url_path: "/about.html".into(),
                is_index: false,
                is_dynamic: false,
                template_path: None,
            },
            RenderedPage {
                url_path: "/posts/hello.html".into(),
                is_index: false,
                is_dynamic: true,
                template_path: None,
            },
        ];

        generate_sitemap(&dist, &pages, &config, "2024-01-01").unwrap();

        let xml = fs::read_to_string(dist.join("sitemap.xml")).unwrap();
        assert!(xml.contains("https://example.com/"), "root should be /");
        assert!(xml.contains("https://example.com/about"), "about should be clean");
        assert!(!xml.contains("about.html"), "should not contain .html");
        assert!(xml.contains("https://example.com/posts/hello"), "nested should be clean");
    }

    #[test]
    fn test_sitemap_clean_links_overrides_clean_urls() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path().join("dist");
        fs::create_dir_all(&dist).unwrap();

        let mut config = test_config();
        config.build.clean_links = true;
        config.sitemap.clean_urls = true;

        let pages = vec![
            RenderedPage {
                url_path: "/about.html".into(),
                is_index: false,
                is_dynamic: false,
                template_path: None,
            },
        ];

        generate_sitemap(&dist, &pages, &config, "2024-01-01").unwrap();

        let xml = fs::read_to_string(dist.join("sitemap.xml")).unwrap();
        // clean_links produces /about (no trailing slash), not /about/ (clean_urls style).
        assert!(xml.contains("https://example.com/about"));
        assert!(!xml.contains("/about/"), "clean_links should override clean_urls — no trailing slash");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p eigen --lib sitemap::tests::test_sitemap_clean_links 2>&1 | tail -10`
Expected: FAIL — sitemap still has `.html` URLs

- [ ] **Step 3: Modify `generate_sitemap()`**

In `src/build/sitemap.rs`, add the import at the top:

```rust
use super::clean_link::to_clean_link;
```

Replace the URL path resolution block (lines 38-42):

```rust
        let url_path = if clean_urls {
            to_clean_url(&page.url_path)
        } else {
            normalize_url_path(&page.url_path)
        };
```

with:

```rust
        let url_path = if config.build.clean_links {
            to_clean_link(&normalize_url_path(&page.url_path))
        } else if clean_urls {
            to_clean_url(&page.url_path)
        } else {
            normalize_url_path(&page.url_path)
        };
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p eigen --lib sitemap 2>&1 | tail -10`
Expected: all sitemap tests pass (old and new)

- [ ] **Step 5: Commit**

```bash
git add src/build/sitemap.rs
git commit -m "feat: sitemap uses clean links when enabled"
```

---

### Task 6: Dev server `.html` fallback

**Files:**
- Modify: `src/dev/server.rs:149-198` (build_router function)

- [ ] **Step 1: Modify `build_router()` to add `.html` fallback**

In `src/dev/server.rs`, replace the static file serving block (lines 193-195):

```rust
    // Static file serving from dist/ as the fallback.
    let serve_dir = ServeDir::new(&dist_dir);
    app = app.fallback_service(serve_dir);
```

with:

```rust
    // Static file serving from dist/ as the fallback.
    // Try the exact path first, then try appending `.html` to support
    // clean URLs (e.g. `/about` → `about.html`).  This mirrors CDN
    // behaviour (Cloudflare, Netlify) during local development.
    let dist_for_fallback = dist_dir.clone();
    let serve_dir = ServeDir::new(&dist_dir)
        .fallback(tower::service_fn(move |req: axum::http::Request<axum::body::Body>| {
            let dist = dist_for_fallback.clone();
            async move {
                let path = req.uri().path();

                // Only try .html fallback for extensionless paths.
                if !path.contains('.') && path != "/" {
                    let html_path = format!("{}.html", path.trim_start_matches('/'));
                    let file_path = dist.join(&html_path);
                    if file_path.is_file() {
                        // Re-issue the request to ServeDir with the .html path.
                        let new_uri: axum::http::Uri = format!("/{}", html_path).parse().unwrap();
                        let (mut parts, body) = req.into_parts();
                        parts.uri = new_uri;
                        let new_req = axum::http::Request::from_parts(parts, body);
                        return ServeDir::new(&dist)
                            .try_call(new_req)
                            .await;
                    }
                }

                // Fall through to 404.
                ServeDir::new(&dist)
                    .try_call(req)
                    .await
            }
        }));
    app = app.fallback_service(serve_dir);
```

- [ ] **Step 2: Add required imports**

At the top of `src/dev/server.rs`, check if `tower` is imported. Add if missing:

```rust
use tower_http::services::ServeDir;
```

This is already imported. We also need the `ServiceExt` trait from tower-http for `try_call`. Check if `tower` is a direct dependency — if not, we need an alternative approach.

- [ ] **Step 3: Verify approach compiles**

Run: `cargo check 2>&1 | head -20`

If `try_call` or `tower::service_fn` is not available, use this simpler alternative approach instead:

Replace the static file serving block (lines 193-195) with:

```rust
    // Static file serving from dist/ as the fallback.
    // Wraps ServeDir with a middleware that tries `.html` extension for
    // extensionless paths, mimicking CDN behaviour (Cloudflare, Netlify).
    let dist_for_html = dist_dir.clone();
    let html_fallback = tower::service_fn(move |req: axum::http::Request<axum::body::Body>| {
        let dist = dist_for_html.clone();
        async move {
            let path = req.uri().path();

            // Only try .html fallback for extensionless paths.
            if !path.contains('.') && path != "/" {
                let html_path = format!("{}.html", path.trim_end_matches('/').trim_start_matches('/'));
                let file_path = dist.join(&html_path);
                if file_path.is_file() {
                    let body = tokio::fs::read(&file_path).await.map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    })?;
                    let response = axum::http::Response::builder()
                        .header("content-type", "text/html; charset=utf-8")
                        .body(axum::body::Body::from(body))
                        .unwrap();
                    return Ok(response);
                }
            }

            // Return 404.
            let response = axum::http::Response::builder()
                .status(axum::http::StatusCode::NOT_FOUND)
                .body(axum::body::Body::empty())
                .unwrap();
            Ok::<_, std::io::Error>(response)
        }
    });

    let serve_dir = ServeDir::new(&dist_dir).fallback(html_fallback);
    app = app.fallback_service(serve_dir);
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -20`
Expected: no errors. Adjust imports as needed.

- [ ] **Step 5: Manual test**

Run: `cargo run -- dev`
Then in another terminal: `curl -s http://localhost:3000/about | head -5`
Expected: returns the content of `about.html` (if it exists in your test site)

- [ ] **Step 6: Commit**

```bash
git add src/dev/server.rs
git commit -m "feat: dev server resolves /about to about.html"
```

---

### Task 7: Integration tests

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Add integration test for clean_links in build output**

Add to `tests/integration_test.rs`:

```rust
#[test]
fn test_clean_links_link_to_output() {
    // Setup: create a temp project with clean_links = true
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Write site.toml with clean_links enabled.
    fs::write(
        root.join("site.toml"),
        r#"
[site]
name = "Test"
base_url = "https://example.com"

[build]
clean_links = true
"#,
    ).unwrap();

    // Create templates directory and a layout + page.
    fs::create_dir_all(root.join("templates")).unwrap();
    fs::write(
        root.join("templates/_layout.html"),
        r#"<!DOCTYPE html><html><body>{% block content %}{% endblock %}</body></html>"#,
    ).unwrap();
    fs::write(
        root.join("templates/index.html"),
        r#"---
---
{% extends "_layout.html" %}
{% block content %}<a {{ link_to("/about.html") }}>About</a>{% endblock %}"#,
    ).unwrap();
    fs::write(
        root.join("templates/about.html"),
        r#"---
---
{% extends "_layout.html" %}
{% block content %}<h1>About</h1>{% endblock %}"#,
    ).unwrap();

    // Build.
    let result = build(root);
    assert!(result.is_ok(), "Build failed: {:?}", result.err());

    // Check that the rendered index.html has clean links.
    let index_html = fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(index_html.contains(r#"href="/about""#), "href should be clean: {}", index_html);
    assert!(!index_html.contains(r#"href="/about.html""#), "href should not have .html");
}
```

Adapt the test setup to match the existing integration test patterns in the file (check how other tests create temp projects and call the build function).

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test integration_test test_clean_links 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: integration test for clean_links feature"
```

---

### Task 8: Documentation

**Files:**
- Create: `docs/clean_links.md`

- [ ] **Step 1: Write feature documentation**

Create `docs/clean_links.md`:

```markdown
# Clean Links

## Overview

The `clean_links` option strips `.html` extensions from generated links, producing URLs like `/about` instead of `/about.html`. This is designed for deployment targets like Cloudflare Pages that automatically resolve `/about` to `about.html`.

## Configuration

```toml
[build]
clean_links = true
```

## What it affects

### `link_to()` function

With `clean_links = true`:

```jinja
<a {{ link_to("/about.html") }}>About</a>
```

Produces:
```html
<a href="/about" hx-get="/_fragments/about.html" hx-target="#content" hx-push-url="/about">About</a>
```

You can also write clean paths directly:
```jinja
<a {{ link_to("/about") }}>About</a>
```

### `page.current_url`

With `clean_links = true`, `page.current_url` returns `/about` instead of `/about.html`. Useful for active-link highlighting:

```jinja
<a href="/about" class="{{ 'active' if page.current_url == '/about' }}">About</a>
```

### Sitemap

When `clean_links` is enabled, sitemap URLs use clean paths (`/about`) instead of file paths (`/about.html`). This takes precedence over `sitemap.clean_urls`.

### Dev server

The dev server always resolves extensionless paths to `.html` files (e.g. `/about` serves `about.html`). This works regardless of the `clean_links` setting.

## Interaction with `clean_urls`

`clean_links` and `clean_urls` are independent:

- `clean_urls` controls **output file structure** (`about.html` vs `about/index.html`)
- `clean_links` controls **generated link format** (`/about.html` vs `/about`)

They compose correctly:

| `clean_urls` | `clean_links` | File on disk | Link in template |
|---|---|---|---|
| off | off | `about.html` | `/about.html` |
| off | on | `about.html` | `/about` |
| on | off | `about/index.html` | `/about/index.html` |
| on | on | `about/index.html` | `/about` |
```

- [ ] **Step 2: Commit**

```bash
git add docs/clean_links.md
git commit -m "docs: add clean_links feature documentation"
```

---

### Task 9: Run full test suite

- [ ] **Step 1: Run all tests**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1 | tail -20`
Expected: no warnings in modified files

- [ ] **Step 3: Fix any issues found**

If any tests fail or clippy warns, fix and re-run.
