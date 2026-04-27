# Focused Template Error Context

**Date:** 2026-04-13
**Branch:** improve-template-error-reporting
**File:** src/template/errors.rs

## Problem

When a template rendering error occurs, the entire context shape is dumped — every key, nested structure, all levels. For complex sites this is a wall of text that obscures the actual problem.

## Solution

Replace the full context dump with a **focused view** that shows only the part of the context relevant to the error. Fall back to a highlighted full dump when focused extraction isn't possible.

## Scope

Only applies to `UndefinedError`. Other error kinds (SyntaxError, InvalidOperation, etc.) skip the context dump entirely — the error message and source snippet are sufficient.

## Degradation Chain

```
Focused path extraction
  | (can't extract expression)
Loop variable tracing
  | (can't find the for-loop)
Highlighted full dump
  | (nothing to highlight)
Plain full dump (today's behavior)
```

## 1. Expression Extraction

When `err.kind()` is `UndefinedError` and debug info is available:

1. Get the failing expression via `err.template_source()` + `err.range()`.
2. Extract the dotted identifier path by scanning left-to-right, stopping at `|`, `(`, `[`, or whitespace-after-keyword.
3. Split into segments: `"page.seo.title"` -> `["page", "seo", "title"]`.

When debug info is not available, or the error is not UndefinedError, skip to fallback.

## 2. Context Path Walking

Given segments `["page", "seo", "title"]` and the template context:

1. Look up `page` in the top-level context.
2. If not found: **top-level miss** — show all top-level keys as the focused output.
3. If found, look up `seo` on that value.
4. If not found: **nested miss** — `page` is the "last valid parent". Show its shape (keys and types).
5. Continue until the break point is found or segments are exhausted.

The output captures:
- The full access path attempted (`page.seo.title`)
- Where it broke (`seo` not found on `page`)
- What the parent actually has (the shape of `page`)

If the root segment isn't in the top-level context, hand off to loop variable tracing.

## 3. Loop Variable Detection

When the root segment (e.g., `post`) isn't a top-level context key:

1. Regex-scan the template source for `{%[-\s]*for\s+{ROOT}\s+in\s+([\w.]+)` where `{ROOT}` is the literal root segment (e.g., `post`).
2. Resolve the captured collection path in the context (could be dotted like `site.posts`).
3. Get the first item of the sequence.
4. Show that item's shape as the focused output.

If the regex doesn't match (macro variable, nested comprehension, etc.), fall through to the highlighted full dump with a note: "variable `post` is not in the top-level context — it may come from a loop or macro."

## 4. Fallback: Highlighted Full Dump

When focused extraction fails:

1. Render the full context shape dump as today.
2. If a partial path is known (we know the root segment), prefix that branch's lines with `>` in console, or use a highlighted background in HTML.
3. If nothing to highlight, show the plain full dump as-is (current behavior).

## 5. Output Format

### Console — focused case
```
  Tried to access: page.seo.title
                        ^^^ not found

  `page` is a map with 4 keys:
    current_url : string
    current_path : string
    base_url : string
    build_time : string
```

### Console — loop variable case
```
  Tried to access: post.titl
                        ^^^^ not found

  `post` comes from: {% for post in posts %}
  Each item in `posts` is a map with 4 keys:
    title : string
    slug : string
    date : string
    tags : sequence (3 items)
```

### Console — highlighted fallback
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

### HTML
Same logic, styled with CSS (highlight background on the relevant branch) instead of `>` markers.

## 6. Integration Points

All changes contained to `src/template/errors.rs`:

1. **`from_minijinja`** — replace the unconditional `tpl_ctx.map(summarize_context)` with new logic that tries focused extraction first, then falls through the degradation chain. Result still lands in `context_summary: Option<String>`.

2. **New private functions:**
   - `extract_expression_path(err) -> Option<Vec<&str>>` — debug info to dotted segments
   - `walk_context_path(segments, ctx) -> ContextWalkResult` — walk and find break point
   - `find_loop_source(root, template_source) -> Option<String>` — regex for `{% for %}`
   - `format_focused_context(walk_result, ...) -> String` — render the focused output
   - `format_highlighted_context(ctx, highlight_root) -> String` — full dump with `>` markers

3. **`summarize_context`** — stays as-is, used by the plain fallback path.

4. **`format_console` / `to_error_html`** — no changes. They render whatever string is in `context_summary`.

5. **No changes to callers** (`src/build/render.rs`, `src/dev/rebuild.rs`). Public API unchanged.

## 7. Testing

All tests in the existing test module in `errors.rs`:

1. **Expression extraction** — dotted path, path with filter (`page.title | upper` -> `page.title`), path with function call, empty/missing debug info.
2. **Context walking** — top-level miss, nested miss at various depths, full path resolves.
3. **Loop variable detection** — simple `{% for x in items %}`, dotted collection `{% for x in site.posts %}`, no match (macro var), multiple loops with same variable name.
4. **Focused output formatting** — verify "Tried to access" / "not found" / parent shape output for both nested and top-level misses.
5. **Highlighted full dump** — verify `>` markers appear on the right branch.
6. **Integration via `from_minijinja`** — construct a real minijinja env with a template containing an undefined variable, call `from_minijinja`, verify `context_summary` contains focused output instead of a full dump.

All unit tests — no integration tests needed (pure formatting logic, no IO).
