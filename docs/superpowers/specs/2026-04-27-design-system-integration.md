# Design Spec: Eigen Website → Wave Funk Design System

## Goal

Replace eigen's custom CSS website with the Wave Funk design system, using the `marketing.html` and `docs.html` reference templates. Follow the same integration pattern as the substrukt website.

## Decisions

- **CSS delivery:** Copy via `sync-design` Justfile recipe (not symlink, not submodule)
- **Accent color:** Orange `#d65d0e` (dark), `#d65d0e` (light), ink `#fff`
- **Footer:** Full 4-column layout with "Built by Wavefunk" badge (SVG from substrukt)
- **Data files:** Reuse existing YAML data, adapt templates to consume them

## CSS Strategy

### sync-design recipe

```just
sync-design:
    rm -rf website/static/css/wavefunk
    cp -r ../design/css website/static/css/wavefunk
```

`website-build` and `website-dev` depend on `sync-design`.

### eigen.css

Minimal override file at `website/static/css/eigen.css`:

```css
:root {
  --accent: #d65d0e;
  --accent-ink: #ffffff;
}

[data-mode="light"] {
  --accent: #d65d0e;
  --accent-ink: #ffffff;
}
```

### Deleted

- `website/static/css/style.css` — replaced entirely by wavefunk.css + eigen.css

## Template Structure

### _base.html

Links wavefunk.css + eigen.css, sets `data-mode="dark"`, loads htmx. Provides `body` block.

```html
<!doctype html>
<html lang="en" data-mode="dark">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>{% block title %}{{ site.name }}{% endblock %}</title>
  <link rel="icon" href="/favicon.svg" type="image/svg+xml">
  <link rel="stylesheet" href="/css/wavefunk/wavefunk.css">
  <link rel="stylesheet" href="/css/eigen.css">
  <script src="https://unpkg.com/htmx.org@2.0.4" defer></script>
  {% block head %}{% endblock %}
</head>
<body>
{% block body %}{% endblock %}
</body>
</html>
```

### _marketing.html (new)

Wraps marketing pages in `mk-wrap` with nav and footer.

```html
{% extends "_base.html" %}
{% block body %}
<div class="mk-wrap">
  {% include "_partials/nav.html" %}
  {% block content %}{% endblock %}
  {% include "_partials/footer.html" %}
</div>
{% endblock %}
```

### index.html

Extends `_marketing.html`. Sections:

1. **Hero** (`mk-hero`) — eyebrow, h1 "eigen", tagline, CTA buttons (Get Started + GitHub), shell line (`cargo install eigen`), stat strip
2. **Features** (`mk-sect` + `mk-features`) — data-driven from `features.yaml`, numbered grid
3. **How It Works** (`mk-sect` + `mk-steps`) — 3-step grid with code snippets
4. **Quick Start** (`mk-sect mk-code`) — code block showing site.toml config
5. **CTA** (`mk-sect`) — centered install CTA
6. **Footer** — included via partial

### _docs.html

Extends `_base.html`. Uses `wf-docs-shell` 3-column layout (sidebar + prose + TOC). Includes dynamic TOC JS that rebuilds on `htmx:afterSettle`.

```html
{% extends "_base.html" %}
{% block body %}
<div class="wf-docs-shell">
  {% block sidebar %}{% include "_partials/sidebar.html" %}{% endblock %}
  {% block doc_content %}
    <div id="doc-content" class="wf-docs-content">
      <article class="wf-prose">
        <div class="wf-crumbs">...</div>
        <h1>{{ doc.title }}</h1>
        <div id="docs-content">{{ doc.body | markdown }}</div>
      </article>
      <div class="wf-docs-pager">...</div>
    </div>
  {% endblock %}
  {% include "_partials/docs-toc.html" %}
</div>
<script>/* buildToc() + htmx:afterSettle listener */</script>
{% endblock %}
```

## Partials

### nav.html

Uses `wf-mnav` with `wf-wordmark`. Iterates over `nav` YAML data.

```html
<div class="wf-mnav">
  <div class="wf-wordmark">
    <img src="/favicon.svg" alt="eigen" width="22" height="22">
    <span class="wf-wordmark-name">EIGEN</span>
  </div>
  {% for item in nav %}
    <a href="...">{{ item.label }}</a>
  {% endfor %}
  <div class="wf-mnav-spacer"></div>
</div>
```

### footer.html

4-column `mk-foot` with brand description, three link columns, and `mk-colophon` with Wavefunk badge.

Columns:
1. **Brand** — eigen wordmark + one-line description
2. **DOCUMENTATION** — Quick Start, Features, Templates, Data Sources
3. **PROJECT** — GitHub, License, Issues, Changelog
4. **RESOURCES** — Installation, Configuration, Deployment

Colophon: `© 2026 EIGEN` | Wavefunk badge SVG + "BUILT BY WAVEFUNK" | `OPEN SOURCE · MIT`

### sidebar.html

Uses `wf-docs-side` + `wf-docs-side-nav`. Iterates categories with section headers. Each link has htmx attributes for fragment navigation targeting `#doc-content`.

### docs-toc.html (new)

Uses `wf-toc` with empty `#docs-toc-links` div populated by JS.

## Data Files

Reuse existing:
- `nav.yaml` — marketing nav links
- `features.yaml` — feature cards for homepage
- `categories.yaml` — doc sidebar sections
- `docs.yaml` — doc hierarchy

No new data files needed unless the sidebar grouping structure requires `docs-nav.yaml` (evaluate during implementation).

## Justfile Changes

```just
sync-design:
    rm -rf website/static/css/wavefunk
    cp -r ../design/css website/static/css/wavefunk

website-build: sync-design
    cargo run -- build -p website -v

website-dev: sync-design
    cargo run -- dev -p website -v
```

## Out of Scope

- Light mode toggle UI (design system supports it, but no toggle button in this iteration)
- New content or documentation pages
- Image optimization changes
- site.toml changes (existing config works as-is)
