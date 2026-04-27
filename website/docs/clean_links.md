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
