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

## Context Dump

When a render error occurs, the error output includes a summary of the
top-level template context variables and their types. This helps diagnose
missing-variable errors by showing what *is* available:

```
  Available context:
    title : string
    posts : sequence (12 items)
    page : map (4 keys)
    nav : map (3 keys)
```

For sequences and maps, the element count is included. Values are not dumped
(collections can be large) — only the key names and their `ValueKind`.

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
- `TemplateError.context_summary` — built by `summarize_context()` from the
  minijinja `Value` passed at the call site.
