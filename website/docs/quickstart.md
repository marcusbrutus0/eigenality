# Quickstart

Get a site running with eigen in five minutes.

## Install

Install eigen via cargo:

```
cargo install eigen
```

Or use the [GitHub Action](/docs/github-action) in CI.

## Create a project

```
eigen init my-site
cd my-site
```

This creates:

```
my-site/
  site.toml          # Site configuration
  templates/
    _base.html        # Base layout
    _partials/
      nav.html        # Navigation partial
    index.html        # Home page
    about.html        # Example page
  _data/
    nav.yaml          # Navigation data
  static/
    css/style.css     # Stylesheet
```

## Write a template

Templates use Jinja2 syntax. Every page extends a base layout and fills in blocks:

```html
---
data:
  nav:
    file: "nav.yaml"
---
{% extends "_base.html" %}

{% block title %}About — {{ site.name }}{% endblock %}

{% block content %}
<h1>About</h1>
<p>This is a page built with eigen.</p>
{% endblock %}
```

The YAML frontmatter between `---` markers loads data into the template context. Here we load `nav.yaml` from the `_data/` directory.

## Add data

Create `_data/nav.yaml`:

```yaml
- label: Home
  url: /
- label: About
  url: /about
```

Use it in your template:

```html
<nav>
  {% for item in nav %}
  <li><a href="{{ item.url }}">{{ item.label }}</a></li>
  {% endfor %}
</nav>
```

## Fetch remote data

Define a source in `site.toml`:

```toml
[sources.api]
url = "https://jsonplaceholder.typicode.com"
```

Query it in frontmatter:

```yaml
---
data:
  posts:
    source: api
    path: /posts
    limit: 5
---
```

## Build

```
eigen build
```

Output goes to `dist/`. Every `{% block %}` is also extracted as a standalone HTML fragment in `dist/_fragments/`, ready for HTMX navigation.

## Development server

```
eigen dev
```

Starts a local server with live reload. Changes to templates, data, or config trigger an automatic rebuild.

## Add HTMX navigation

Load HTMX in your base template:

```html
<script src="https://unpkg.com/htmx.org@2.0.4"></script>
```

Use `link_to()` to generate HTMX-powered links:

```html
<a {{ link_to("/about.html") }}>About</a>
```

This emits both a regular `href` (for initial load and SEO) and `hx-get`/`hx-target` attributes (for fragment swaps). Navigation feels instant — only the content block is replaced.

## Next steps

- [Clean Links](/docs/clean-links) — remove `.html` extensions from URLs
- [Content Hashing](/docs/content-hashing) — cache-busting for static assets
- [View Transitions](/docs/view-transitions) — smooth page swap animations
- [Draft Pages](/docs/draft-pages) — hide work-in-progress pages from production
