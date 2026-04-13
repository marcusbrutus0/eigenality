# `$${...}` Env Var Escape Convention — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow literal `${VAR}` text in config, data files, and query strings by writing `$${VAR}` as an escape.

**Architecture:** Two-phase sentinel approach — before the env var regex runs, swap `$${...}` with null-byte sentinels; after substitution, restore sentinels to literal `${...}`. Applied in both substitution sites (`config/mod.rs` and `data/query.rs`).

**Tech Stack:** Rust, regex crate

---

### Task 1: Add escape handling to `interpolate_env_vars` (config/strict mode)

**Files:**
- Modify: `src/config/mod.rs:854-891`

- [ ] **Step 1: Write the failing test — escaped var produces literal output**

Add this test after the existing `test_env_interpolation_missing_var` test (after line 1273):

```rust
#[test]
fn test_env_escape_produces_literal() {
    let input = r#"example = "$${SOME_VALUE}""#;
    let result = interpolate_env_vars(input).unwrap();
    assert_eq!(result, r#"example = "${SOME_VALUE}""#);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_env_escape_produces_literal -- --nocapture`
Expected: FAIL — `$${SOME_VALUE}` is not currently handled, the regex sees `${SOME_VALUE}` inside it and errors on the missing env var.

- [ ] **Step 3: Implement escape handling in `interpolate_env_vars`**

Replace the function body at `src/config/mod.rs:857-891` with:

```rust
pub fn interpolate_env_vars(input: &str) -> Result<String> {
    let re = Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();

    // Phase 1: shelter escaped $${...} patterns behind sentinels.
    let escaped_re = Regex::new(r"\$\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
    let mut sheltered: Vec<String> = Vec::new();
    let working = escaped_re.replace_all(input, |caps: &regex::Captures| {
        let var_name = caps[1].to_string();
        sheltered.push(var_name);
        format!("\x00EIGEN_ESC_{}\x00", sheltered.len() - 1)
    }).into_owned();

    // Phase 2: normal env var substitution on the sheltered string.
    let mut result = working.clone();
    let mut errors: Vec<String> = Vec::new();

    let captures: Vec<(String, String)> = re
        .captures_iter(&working)
        .map(|cap| {
            let full_match = cap[0].to_string();
            let var_name = cap[1].to_string();
            (full_match, var_name)
        })
        .collect();

    for (full_match, var_name) in &captures {
        match std::env::var(var_name) {
            Ok(value) => {
                result = result.replace(full_match.as_str(), &value);
            }
            Err(_) => {
                errors.push(var_name.clone());
            }
        }
    }

    if !errors.is_empty() {
        bail!(
            "Missing environment variable(s): {}",
            errors.join(", ")
        );
    }

    // Phase 3: restore sentinels to literal ${VAR_NAME}.
    for (i, var_name) in sheltered.iter().enumerate() {
        result = result.replace(
            &format!("\x00EIGEN_ESC_{}\x00", i),
            &format!("${{{}}}", var_name),
        );
    }

    Ok(result)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_env_escape_produces_literal -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat: add \$\${...} escape to interpolate_env_vars (strict mode)"
```

---

### Task 2: Add remaining tests for strict-mode escape

**Files:**
- Modify: `src/config/mod.rs` (test section, after the test added in Task 1)

- [ ] **Step 1: Write test — escaped var with real var in same string**

```rust
#[test]
fn test_env_escape_mixed_with_real_var() {
    unsafe { std::env::set_var("EIGEN_ESC_HOST", "example.com") };
    let input = r#"Use $${API_KEY} for auth at ${EIGEN_ESC_HOST}"#;
    let result = interpolate_env_vars(input).unwrap();
    assert_eq!(result, r#"Use ${API_KEY} for auth at example.com"#);
    unsafe { std::env::remove_var("EIGEN_ESC_HOST") };
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test test_env_escape_mixed_with_real_var -- --nocapture`
Expected: PASS (implementation from Task 1 handles this).

- [ ] **Step 3: Write test — escaped var that would be missing does not error**

```rust
#[test]
fn test_env_escape_missing_var_no_error() {
    // $${NONEXISTENT} should NOT trigger the "missing env var" error.
    let input = r#"token = "$${NONEXISTENT}""#;
    let result = interpolate_env_vars(input).unwrap();
    assert_eq!(result, r#"token = "${NONEXISTENT}""#);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_env_escape_missing_var_no_error -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all existing env interpolation tests for regression**

Run: `cargo test test_env_interpolation -- --nocapture`
Expected: All 3 existing tests PASS (no regression).

- [ ] **Step 6: Commit**

```bash
git add src/config/mod.rs
git commit -m "test: add escape convention tests for strict-mode interpolation"
```

---

### Task 3: Add escape handling to `interpolate_env_in_string` (query/lenient mode)

**Files:**
- Modify: `src/data/query.rs:234-244`

- [ ] **Step 1: Write the failing test — escaped var in query body**

Add this test after the existing `test_interpolate_query_body_unresolved_env_var_left_as_is` test (after line 1186):

```rust
#[test]
fn test_interpolate_env_escape_in_string() {
    let input = "Use $${API_KEY} here";
    let result = interpolate_env_in_string(input);
    assert_eq!(result, "Use ${API_KEY} here");
}
```

Note: `interpolate_env_in_string` is `fn` (private), so the test must be inside the existing `#[cfg(test)] mod tests` block in `query.rs`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_interpolate_env_escape_in_string -- --nocapture`
Expected: FAIL — `$${API_KEY}` is not handled; the function leaves `$${API_KEY}` as-is (since `$$` prefix means the regex doesn't match `$${`... actually wait — the regex `\$\{` will match the `${` inside `$${API_KEY}` since regex doesn't anchor, so the function would try to resolve `API_KEY` as an env var). Either way, it won't produce the correct output.

- [ ] **Step 3: Implement escape handling in `interpolate_env_in_string`**

Replace the function at `src/data/query.rs:234-244` with:

```rust
fn interpolate_env_in_string(s: &str) -> String {
    // Phase 1: shelter escaped $${...} patterns behind sentinels.
    let escaped_re = Regex::new(r"\$\$\{([A-Za-z_][A-Za-z0-9_]*)\}").unwrap();
    let mut sheltered: Vec<String> = Vec::new();
    let working = escaped_re.replace_all(s, |caps: &regex::Captures| {
        let var_name = caps[1].to_string();
        sheltered.push(var_name);
        format!("\x00EIGEN_ESC_{}\x00", sheltered.len() - 1)
    }).into_owned();

    // Phase 2: normal env var substitution.
    let mut result = working.clone();
    for cap in ENV_VAR_RE.captures_iter(&working) {
        let full_match = &cap[0];
        let var_name = &cap[1];
        if let Ok(val) = std::env::var(var_name) {
            result = result.replace(full_match, &val);
        }
    }

    // Phase 3: restore sentinels to literal ${VAR_NAME}.
    for (i, var_name) in sheltered.iter().enumerate() {
        result = result.replace(
            &format!("\x00EIGEN_ESC_{}\x00", i),
            &format!("${{{}}}", var_name),
        );
    }

    result
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_interpolate_env_escape_in_string -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all query interpolation tests for regression**

Run: `cargo test -p eigen test_interpolate_query -- --nocapture`
Expected: All existing query tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/data/query.rs
git commit -m "feat: add \$\${...} escape to interpolate_env_in_string (lenient mode)"
```

---

### Task 4: Update `interpolate_value` fast-path check

**Files:**
- Modify: `src/data/query.rs:200-210`

The fast-path at line 206 checks `resolved.contains("${")` to skip env interpolation. The string `$${FOO}` also contains `${`, so this fast-path will correctly route escaped strings to `interpolate_env_in_string`. No code change needed, but we should verify with a test through the `interpolate_value` call path.

- [ ] **Step 1: Write test — escaped var flows through `interpolate_value`**

```rust
#[test]
fn test_interpolate_value_env_escape() {
    let item = json!({});
    let value = Value::String("Use $${API_KEY} here".into());
    let result = interpolate_value(&value, &item, "item").unwrap();
    assert_eq!(result, Value::String("Use ${API_KEY} here".into()));
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test test_interpolate_value_env_escape -- --nocapture`
Expected: PASS (uses the implementation from Task 3).

- [ ] **Step 3: Commit**

```bash
git add src/data/query.rs
git commit -m "test: verify env escape flows through interpolate_value"
```

---

### Task 5: Update Justfile to use escape convention

**Files:**
- Modify: `Justfile`

The current `Justfile` has a workaround on lines 1-3: it sets env vars like `NOTION_DB_ID='${NOTION_DB_ID}'` so that doc code examples pass through. With the new escape convention, the doc templates should use `$${NOTION_DB_ID}` instead, eliminating the need for this workaround.

- [ ] **Step 1: Find all `${...}` patterns in the website templates that use the workaround vars**

Run: `grep -rn '\${NOTION_DB_ID}\|${NOTION_API_KEY}\|${ENV_VAR}\|${API_TOKEN}\|${WORKSPACE_ID}\|${CMS_TOKEN}\|${STRAPI_TOKEN}' website/`

Note which files and lines contain these patterns.

- [ ] **Step 2: Replace each `${VAR}` with `$${VAR}` in those template files**

For each file found in step 1, replace `${VAR_NAME}` with `$${VAR_NAME}` where the intent is literal display text (doc examples).

- [ ] **Step 3: Remove the `_doc_env` workaround from Justfile**

Replace `Justfile` contents with:

```just
# Build the website
website-build:
    cargo run -- build -p website -v

# Start dev server for the website
website-dev:
    cargo run -- dev -p website -v

# Run audit on the website
website-audit:
    cargo run -- audit -p website

# Run all tests
test:
    cargo test

# Run clippy
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt
```

- [ ] **Step 4: Verify the website builds without the workaround**

Run: `just website-build`
Expected: Build succeeds. Doc examples render `${VAR_NAME}` literally.

- [ ] **Step 5: Commit**

```bash
git add Justfile website/
git commit -m "refactor: replace _doc_env workaround with \$\${...} escape in templates"
```

---

### Task 6: Write documentation

**Files:**
- Create: `docs/env_vars.md`

- [ ] **Step 1: Write `docs/env_vars.md`**

```markdown
# Environment Variable Substitution

Eigen replaces `${VAR_NAME}` patterns with environment variable values in
three contexts:

- **`site.toml`** — site configuration
- **`_data/` files** — global YAML and JSON data
- **Data query fields** — query paths, filters, and request bodies

## Basic usage

```toml
# site.toml
[site]
name = "My Site"
base_url = "${BASE_URL}"
```

```yaml
# _data/secrets.yaml
api_key: "${API_TOKEN}"
```

## Strict vs lenient mode

| Context | Missing variable behavior |
|---|---|
| `site.toml` | **Error** — build fails with message listing missing variables |
| `_data/` files | **Error** — same as above |
| Data query fields | **Silent** — `${MISSING}` is left as literal text |

## Escaping: literal `${...}` in content

To include a literal `${VAR_NAME}` in your output (e.g., in documentation or
code examples), double the dollar sign:

```
$${SOME_VALUE}
```

This renders as `${SOME_VALUE}` in the output. The doubled `$$` tells eigen to
skip substitution and produce the literal text.

### Examples

| Input | Output |
|---|---|
| `${HOME}` | Value of HOME env var |
| `$${HOME}` | `${HOME}` (literal text) |
| `$${MISSING}` | `${MISSING}` (no error, even in strict mode) |
| `Use $${API_KEY} at ${HOST}` | `Use ${API_KEY} at example.com` |

### When to use escaping

- Documentation pages showing environment variable examples
- Code snippets that include shell syntax
- Template examples meant to be read, not evaluated

## Variable naming rules

Variable names must match `[A-Za-z_][A-Za-z0-9_]*`:

- Start with a letter or underscore
- Followed by letters, digits, or underscores
- Examples: `HOME`, `API_TOKEN`, `_INTERNAL`, `my_var_2`
```

- [ ] **Step 2: Run docs build to verify**

Run: `just website-build`
Expected: Build succeeds with the new doc page.

- [ ] **Step 3: Commit**

```bash
git add docs/env_vars.md
git commit -m "docs: add environment variable substitution and escape convention docs"
```

---

### Task 7: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Run `/simplify` on changed code**

As required by project conventions.
