# Continue-on-Error for Template Rendering

## Problem

When a single item in a collection template fails to render, eigen aborts the entire build. In dev mode, the homepage is also overwritten with the error page. A data issue on one CMS item shouldn't prevent every other page from building.

## Design

### 1. Stop overwriting index.html on error

**File:** `src/dev/rebuild.rs`, `write_dev_error_pages()`

Remove the unconditional `index.html` overwrite (line 515). Error HTML is still written to:
- The item's own output path (navigating to the broken page shows the error)
- The `_error.html` sentinel (dev server detects the error state)

The homepage is only affected if the failing template IS the homepage.

### 2. Continue-on-error in dev dynamic page loop

**File:** `src/dev/rebuild.rs`, `render_dynamic_page_dev()`

Hardcoded behavior — dev mode always continues on render error.

On per-item render failure:
1. Write error HTML to the item's own output path
2. Write error HTML to `_error.html` sentinel
3. Log the detailed error to console
4. Increment error counter
5. `continue` to the next item

After the loop, emit a summary warning if any items failed. Return `Ok(rendered_pages)` with successfully rendered pages.

`UndefinedBehavior::Strict` remains unchanged — errors are raised and surfaced, they just don't abort sibling items.

### 3. Continue-on-error in prod with config flag

**File:** `src/config/mod.rs`, `BuildConfig`

```rust
#[serde(default)]
pub continue_on_render_error: bool,
```

Default: `false` (current behavior — first error aborts).

**File:** `src/build/render.rs`, result collection loop (~line 1241)

Per-item async blocks still return `Err` on failure (no change). The result collection loop changes:
- `continue_on_render_error = true`: collect errors into a vec, push successes, log summary warning
- `continue_on_render_error = false`: return on first error (current behavior)

Top-level build loop (~line 382) needs no change — when the flag is true, `render_dynamic_page()` returns `Ok` with partial results.

### 4. Summary logging

Both dev and prod emit `tracing::warn!` after the loop when errors occurred:

```
3 of 8 items in 'case_studies.html' failed to render — skipped
```

Per-item detailed error output (with context dump) remains as-is.

## What does NOT change

- **Strict undefined behavior stays.** Errors are surfaced clearly, not silently swallowed.
- **Static page errors still abort.** No siblings to continue with.
- **Per-item error pages still written.** Navigating to the broken page shows the full error.

## Implementation sequence

1. Remove `index.html` overwrite from `write_dev_error_pages()`
2. Add continue-on-error to `render_dynamic_page_dev()` (dev mode, hardcoded)
3. Add `continue_on_render_error` config flag to `BuildConfig` with `Default` impl
4. Wire flag into `render_dynamic_page()` result collection loop for prod
5. Add summary logging for both dev and prod
6. Write tests
7. Write docs
