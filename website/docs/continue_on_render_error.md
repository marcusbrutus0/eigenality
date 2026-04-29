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
