# File-Linked Data Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow `_data` YAML/JSON files to reference external files via a `_file` key suffix, resolved to file contents at load time.

**Architecture:** A standalone `resolve_file_links(value, project_root)` function recursively walks a `serde_json::Value` tree, finds keys ending in `_file`, reads the referenced file, and replaces the `_file` key with a sibling key (suffix stripped) containing the file contents. Called from both `load_global_data()` and `DataFetcher::fetch_file()`.

**Tech Stack:** Rust, serde_json, eyre

---

## File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `src/data/file_links.rs` | `resolve_file_links` function + unit tests |
| Modify | `src/data/mod.rs` | Register `file_links` module |
| Modify | `src/data/global.rs` | Call `resolve_file_links` after parsing each data file |
| Modify | `src/data/fetcher.rs` | Store `project_root`, call `resolve_file_links` after parsing in `fetch_file()` |
| Modify | `website/_data/docs.yaml` | Replace inline `content:` with `content_file:` references |
| Create | `docs/file_links.md` | Feature documentation |

---

### Task 1: Create `resolve_file_links` with tests

**Files:**
- Create: `src/data/file_links.rs`
- Modify: `src/data/mod.rs`

- [ ] **Step 1: Write the failing test for basic resolution**

In `src/data/file_links.rs`:

```rust
//! Resolve `_file` key suffix references in data values.

use eyre::{Result, WrapErr, bail};
use serde_json::Value;
use std::path::Path;

pub fn resolve_file_links(value: Value, project_root: &Path) -> Result<Value> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_basic_resolution() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "docs/hello.md", "# Hello World");

        let input = serde_json::json!({
            "title": "Hello",
            "content_file": "docs/hello.md"
        });

        let result = resolve_file_links(input, root).unwrap();
        assert_eq!(result["title"], "Hello");
        assert_eq!(result["content"], "# Hello World");
        assert!(result.get("content_file").is_none());
    }
}
```

- [ ] **Step 2: Register the module in `src/data/mod.rs`**

Add to `src/data/mod.rs` after the existing module declarations:

```rust
mod file_links;
pub use file_links::resolve_file_links;
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib data::file_links::tests::test_basic_resolution`

Expected: FAIL with "not yet implemented"

- [ ] **Step 4: Implement `resolve_file_links`**

Replace the `todo!()` in `src/data/file_links.rs` with:

```rust
pub fn resolve_file_links(value: Value, project_root: &Path) -> Result<Value> {
    match value {
        Value::Object(map) => resolve_object(map, project_root),
        Value::Array(arr) => {
            let resolved: Result<Vec<Value>> = arr
                .into_iter()
                .map(|v| resolve_file_links(v, project_root))
                .collect();
            Ok(Value::Array(resolved?))
        }
        other => Ok(other),
    }
}

fn resolve_object(
    map: serde_json::Map<String, Value>,
    project_root: &Path,
) -> Result<Value> {
    let mut result = serde_json::Map::with_capacity(map.len());

    let file_keys: Vec<String> = map
        .keys()
        .filter(|k| k.ends_with("_file"))
        .cloned()
        .collect();

    for key in &file_keys {
        let target_key = key.strip_suffix("_file").unwrap_or(key).to_string();
        if map.contains_key(&target_key) {
            bail!(
                "Conflict: both '{}' and '{}' exist in the same object",
                target_key,
                key
            );
        }
    }

    for (key, value) in map {
        if let Some(target_key) = key.strip_suffix("_file") {
            let path_str = value
                .as_str()
                .ok_or_else(|| eyre::eyre!("'{}' must be a string file path", key))?;

            let full_path = project_root.join(path_str);
            let contents = std::fs::read_to_string(&full_path)
                .wrap_err_with(|| format!("Failed to read file linked by '{}': {}", key, full_path.display()))?;

            result.insert(target_key.to_string(), Value::String(contents));
        } else {
            result.insert(key, resolve_file_links(value, project_root)?);
        }
    }

    Ok(Value::Object(result))
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib data::file_links::tests::test_basic_resolution`

Expected: PASS

- [ ] **Step 6: Add remaining unit tests**

Append to the `tests` module in `src/data/file_links.rs`:

```rust
    #[test]
    fn test_array_of_objects() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "docs/a.md", "# A");
        write(root, "docs/b.md", "# B");

        let input = serde_json::json!([
            { "title": "A", "content_file": "docs/a.md" },
            { "title": "B", "content_file": "docs/b.md" }
        ]);

        let result = resolve_file_links(input, root).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["content"], "# A");
        assert_eq!(arr[1]["content"], "# B");
        assert!(arr[0].get("content_file").is_none());
    }

    #[test]
    fn test_nested_objects() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "data/bio.txt", "A short bio.");

        let input = serde_json::json!({
            "author": {
                "name": "Alice",
                "bio_file": "data/bio.txt"
            }
        });

        let result = resolve_file_links(input, root).unwrap();
        assert_eq!(result["author"]["bio"], "A short bio.");
        assert!(result["author"].get("bio_file").is_none());
    }

    #[test]
    fn test_no_file_keys_passthrough() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!({
            "title": "Hello",
            "count": 42
        });

        let result = resolve_file_links(input.clone(), root).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_scalar_passthrough() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!("just a string");
        let result = resolve_file_links(input.clone(), root).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_missing_file_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!({
            "content_file": "nonexistent.md"
        });

        let err = resolve_file_links(input, root).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("content_file"), "error should name the key: {}", msg);
    }

    #[test]
    fn test_non_string_value_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!({
            "content_file": 42
        });

        let err = resolve_file_links(input, root).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("content_file"), "error should name the key: {}", msg);
        assert!(msg.contains("string"), "error should mention string: {}", msg);
    }

    #[test]
    fn test_conflict_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "docs/hello.md", "# Hello");

        let input = serde_json::json!({
            "content": "inline",
            "content_file": "docs/hello.md"
        });

        let err = resolve_file_links(input, root).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Conflict"), "error should mention conflict: {}", msg);
    }

    #[test]
    fn test_deeply_nested_in_array() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "x.txt", "deep content");

        let input = serde_json::json!([
            [{ "data_file": "x.txt" }]
        ]);

        let result = resolve_file_links(input, root).unwrap();
        assert_eq!(result[0][0]["data"], "deep content");
    }
```

- [ ] **Step 7: Run all file_links tests**

Run: `cargo test --lib data::file_links`

Expected: All tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/data/file_links.rs src/data/mod.rs
git commit -m "feat: add resolve_file_links for _file suffix convention"
```

---

### Task 2: Integrate into global data loader

**Files:**
- Modify: `src/data/global.rs:21-79`

- [ ] **Step 1: Write a failing integration test in `global.rs`**

Append to the `tests` module in `src/data/global.rs`:

```rust
    #[test]
    fn test_file_links_resolved() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "docs/hello.md", "# Hello from file");
        write(
            root,
            "_data/pages.yaml",
            "- title: Hello\n  content_file: \"docs/hello.md\"\n",
        );

        let data = load_global_data(root).unwrap();
        let pages = data["pages"].as_array().unwrap();
        assert_eq!(pages[0]["content"], "# Hello from file");
        assert!(pages[0].get("content_file").is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib data::global::tests::test_file_links_resolved`

Expected: FAIL — `content_file` key is still present, `content` key is missing

- [ ] **Step 3: Add `resolve_file_links` call to `load_global_data`**

In `src/data/global.rs`, add the import at the top:

```rust
use super::file_links::resolve_file_links;
```

Then in `load_global_data`, change the line `data.insert(key, value);` (line 76) to:

```rust
        let value = resolve_file_links(value, project_root)
            .wrap_err_with(|| format!("Failed to resolve file links in {}", path.display()))?;
        data.insert(key, value);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib data::global::tests::test_file_links_resolved`

Expected: PASS

- [ ] **Step 5: Run all global tests to check for regressions**

Run: `cargo test --lib data::global`

Expected: All tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/data/global.rs
git commit -m "feat: resolve _file links in global data loader"
```

---

### Task 3: Integrate into DataFetcher

**Files:**
- Modify: `src/data/fetcher.rs:73-109` (struct + constructor)
- Modify: `src/data/fetcher.rs:300-325` (`fetch_file`)

- [ ] **Step 1: Add `project_root` field to `DataFetcher`**

In `src/data/fetcher.rs`, add a field to the `DataFetcher` struct after `data_dir`:

```rust
    project_root: PathBuf,
```

And in `DataFetcher::new`, store it:

```rust
        Self {
            sources: sources.clone(),
            url_cache: HashMap::new(),
            file_cache: HashMap::new(),
            data_dir: project_root.join("_data"),
            project_root: project_root.to_path_buf(),
            client: reqwest::Client::new(),
            data_cache,
            rate_limiter,
        }
```

- [ ] **Step 2: Add `resolve_file_links` call to `fetch_file`**

Add the import at the top of `src/data/fetcher.rs`:

```rust
use super::file_links::resolve_file_links;
```

In `fetch_file`, change the block just before `self.file_cache.insert(...)` from:

```rust
        self.file_cache.insert(file_path.to_string(), value.clone());
        Ok(value)
```

To:

```rust
        let value = resolve_file_links(value, &self.project_root)
            .wrap_err_with(|| format!("Failed to resolve file links in {}", full_path.display()))?;

        self.file_cache.insert(file_path.to_string(), value.clone());
        Ok(value)
```

- [ ] **Step 3: Verify the project compiles**

Run: `cargo check`

Expected: No errors

- [ ] **Step 4: Run all existing tests to check for regressions**

Run: `cargo test`

Expected: All tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/data/fetcher.rs
git commit -m "feat: resolve _file links in DataFetcher::fetch_file"
```

---

### Task 4: Update website docs.yaml to use file links

**Files:**
- Modify: `website/_data/docs.yaml`

- [ ] **Step 1: Replace inline content with `content_file` references**

Rewrite `website/_data/docs.yaml` so each entry uses `content_file` instead of `content`. The mapping is slug → file path (slugs use hyphens, filenames use underscores):

```yaml
# Getting Started

- title: "Quickstart"
  slug: quickstart
  category: Getting Started
  description: "Install eigen and build your first site in five minutes."
  content_file: "docs/quickstart.md"
```

The full slug-to-file mapping:

| slug | content_file |
|------|-------------|
| quickstart | `docs/quickstart.md` |
| github-action | `docs/github_action.md` |
| draft-pages | `docs/draft_pages.md` |
| clean-links | `docs/clean_links.md` |
| data-cache | `docs/data_cache.md` |
| post-method | `docs/post_method.md` |
| rate-limiting | `docs/rate_limiting.md` |
| source-asset | `docs/source_asset.md` |
| async-build | `docs/async_build.md` |
| incremental-builds | `docs/incremental_builds.md` |
| content-hashing | `docs/content_hashing.md` |
| critical-css | `docs/critical_css.md` |
| css-js-bundling | `docs/css_js_bundling.md` |
| lazy-loading | `docs/lazy_loading.md` |
| preload-prefetch | `docs/preload_prefetch.md` |
| audit | `docs/audit.md` |
| json-ld | `docs/json_ld.md` |
| og-twitter-meta | `docs/og_twitter_meta.md` |
| robots-txt | `docs/robots_txt.md` |
| rss-feed | `docs/rss_feed.md` |
| redirects | `docs/redirects.md` |
| analytics | `docs/analytics.md` |
| view-transitions | `docs/view_transitions.md` |

Note: `quickstart` doesn't currently exist as `docs/quickstart.md`. Extract the inline content from the current `docs.yaml` entry into a new `docs/quickstart.md` file before replacing it with a `content_file` reference. Check each slug — if the corresponding `docs/*.md` file doesn't exist, create it from the inline content.

- [ ] **Step 2: Verify the docs.yaml is valid YAML**

Run: `python3 -c "import yaml; yaml.safe_load(open('website/_data/docs.yaml'))"`

Expected: No errors

- [ ] **Step 3: Build the website to verify rendering works**

Run: `cargo run -- build website`

Or use the project's just command if available. Verify the build succeeds and docs pages are generated in `website/dist/docs/`.

- [ ] **Step 4: Spot-check a rendered doc page**

Pick one doc (e.g., analytics) and verify the rendered HTML contains the expected content from `docs/analytics.md`.

- [ ] **Step 5: Commit**

```bash
git add website/_data/docs.yaml docs/
git commit -m "refactor: replace inline docs content with _file references"
```

---

### Task 5: Write feature documentation

**Files:**
- Create: `docs/file_links.md`

- [ ] **Step 1: Write docs/file_links.md**

```markdown
# File-Linked Data

Reference external files from `_data/` YAML and JSON files using the `_file`
key suffix. At build time, eigen reads the linked file and replaces the `_file`
key with a sibling key containing the file contents as a string.

## Usage

Add a key ending in `_file` to any object in a data file. The value is a path
relative to the project root.

```yaml
# _data/docs.yaml
- title: "Analytics"
  slug: analytics
  content_file: "docs/analytics.md"
```

At build time this resolves to:

```yaml
- title: "Analytics"
  slug: analytics
  content: "# Analytics\n\nEigen injects analytics tracking..."
```

Templates access the resolved key as normal:

```html
{{ doc.content | markdown }}
```

## Rules

- Paths are relative to the **project root**, not `_data/`.
- The `_file` suffix is stripped to produce the target key: `bio_file` → `bio`.
- If both the target key and `_file` key exist (e.g., `content` and
  `content_file`), the build fails with a conflict error.
- The linked file is read as UTF-8 text. It is not parsed as YAML or JSON.
- Resolution applies to both global data (`_data/` directory) and per-page
  data queries (`data:` in frontmatter).
- Nesting works: `_file` keys inside nested objects and arrays are resolved.
```

- [ ] **Step 2: Commit**

```bash
git add docs/file_links.md
git commit -m "docs: add file-linked data feature documentation"
```

---

### Task 6: Integration test

**Files:**
- Modify: `tests/integration_test.rs`

- [ ] **Step 1: Add an integration test**

Append to `tests/integration_test.rs`:

```rust
#[tokio::test]
async fn test_file_linked_data() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // site.toml
    write(
        root,
        "site.toml",
        r#"
[site]
name = "File Link Test"
base_url = "http://localhost"

[build]
minify = false
"#,
    );

    // External markdown file
    write(root, "content/hello.md", "# Hello\n\nFrom a linked file.");

    // Data file with _file reference
    write(
        root,
        "_data/pages.yaml",
        "- title: Hello\n  slug: hello\n  body_file: \"content/hello.md\"\n",
    );

    // Base template
    write(
        root,
        "templates/_base.html",
        "<!DOCTYPE html><html><body>{% block content %}{% endblock %}</body></html>",
    );

    // Dynamic page template
    write(
        root,
        "templates/[page].html",
        "---\ncollection:\n  file: \"pages.yaml\"\nslug_field: slug\nitem_as: page\n---\n{% extends \"_base.html\" %}\n{% block content %}{{ page.body | markdown }}{% endblock %}",
    );

    eigen::build::build(root, true, false, false).await.unwrap();

    let output = fs::read_to_string(root.join("dist/hello/index.html")).unwrap();
    assert!(
        output.contains("<h1>Hello</h1>"),
        "rendered page should contain markdown from linked file: {}",
        output
    );
    assert!(
        output.contains("From a linked file."),
        "rendered page should contain body text from linked file: {}",
        output
    );
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test integration_test test_file_linked_data`

Expected: PASS

- [ ] **Step 3: Run all tests for final regression check**

Run: `cargo test`

Expected: All tests PASS

- [ ] **Step 4: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration test for file-linked data"
```
