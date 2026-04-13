# Template Error Reporting

Improved error messages when template rendering fails, showing correct file
line numbers and a dump of the available template context.

## Line Number Adjustment

Minijinja does not know about frontmatter — it sees only the template body.
When a render error occurs, minijinja reports the line number relative to the
body, not the original file. Eigen adjusts this by adding the number of lines
the frontmatter block occupied (including `---` delimiters).

The adjustment is only applied when the error originates in the **page template
itself**. Errors in included or parent templates (e.g. `_base.html`) are left
as-is because those files have no frontmatter.

When the line number is adjusted, both numbers are shown:

```
  Line     : 14 (template line 5)
```

The frontmatter line count is computed once during page discovery using
`frontmatter::count_frontmatter_lines()` and stored on `PageDef`.

## Focused Context Summary (Degradation Chain)

When a render error occurs and the error is an `UndefinedError`, the error
output includes a *focused* context summary instead of a full dump. The
summary uses a degradation chain to show the most relevant information:

1. **Extract expression path:** From minijinja's debug info (`template_source()`
   and `range()`), extract the dotted path that failed (e.g. `page.seo.title`).
   When the range points to a sub-expression (`.seo.title`), walk backwards
   through the source to find the full path.

2. **Walk the context:** Traverse the path through the context to find where
   it breaks. This produces a `ContextWalkResult`: top-level miss, nested miss,
   or fully resolved.

3. **Loop variable detection:** If the root variable is not in the top-level
   context, search the template source for `{% for VAR in COLLECTION %}` and
   resolve the collection to show item shape.

4. **Format focused output:** Show what was accessed, what was missing, and
   what the parent actually contains:

```
  Available context:
    Tried to access: page.seo.title
                          ^^^ not found

    `page` is a map with 2 keys:
    url : string
    base : string
```

For loop variables:

```
  Available context:
    Tried to access: post.titl
    `post` comes from: {% for post in posts %}

    Each item is a map with 2 keys:
    title : string
    slug : string
```

5. **Fallback:** If debug info is unavailable, fall back to a full context
   shape dump with the relevant branch highlighted.

6. **Non-UndefinedError:** Context summary is omitted entirely (returns `None`)
   since context is not relevant for syntax errors, unknown filters, etc.

### Full Context Shape Dump (fallback)

When focused extraction is not possible, a recursive shape dump of the full
context is shown — every key, its type, and nested structure, but never values:

```
  Available context:
    title : string
    posts : sequence (12 items)
      [item] : map (4 keys)
        title : string
        slug : string
        date : string
        tags : sequence (3 items)
          [item] : string
    page : map (4 keys)
      current_url : string
      current_path : string
      base_url : string
      build_time : string
```

Maps list all their keys recursively. Sequences sample the first element to
infer the item shape. Nesting stops at 4 levels deep. Values are never shown
(collections can be large) — only the structure.

## Where It Appears

- **Dev server (browser):** The HTML error page shows both the adjusted line
  number and the "Available Context" section.
- **Console output:** The CLI error banner includes the same information.
- **Production build:** Same console output on render failure.

## Implementation Details

- `PageDef.frontmatter_line_count` — computed in `discovery::discover_pages()`
  via `frontmatter::count_frontmatter_lines()`.
- `TemplateError.raw_line` — the unadjusted line from minijinja.
- `TemplateError.line` — the file-adjusted line number.
- `TemplateError.context_summary` — built by `build_context_summary()` which
  orchestrates the focused degradation chain. For `UndefinedError`, it uses
  `extract_expression_path()`, `walk_context_path()`, `find_loop_collection()`,
  and `format_focused_context()`. Falls back to `summarize_context()` (full
  shape dump) when debug info is unavailable. Returns `None` for non-undefined
  errors. Depth is capped at `SHAPE_MAX_DEPTH` (4).
