# Design System Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace eigen's custom website CSS and templates with the Wave Funk design system, following the substrukt integration pattern.

**Architecture:** Copy the design system CSS via a `sync-design` Justfile recipe. Rewrite all templates to use `wf-*` and `mk-*` classes. Preserve existing YAML data files and htmx fragment navigation. Create a thin `eigen.css` override for the accent color.

**Tech Stack:** Wave Funk CSS design system, Minijinja templates, htmx 2.0.4

---

## File Map

**Create:**
- `website/static/css/eigen.css` — accent color override (4 lines)
- `website/templates/_marketing.html` — marketing page wrapper
- `website/templates/_partials/docs-toc.html` — table of contents partial

**Modify:**
- `Justfile` — add `sync-design` recipe, update `website-build` and `website-dev` deps
- `website/templates/_base.html` — link wavefunk.css + eigen.css, set `data-mode="dark"`
- `website/templates/index.html` — rewrite to use `mk-*` marketing classes
- `website/templates/_docs.html` — rewrite to use `wf-docs-shell` 3-column layout
- `website/templates/docs/[doc].html` — update to use new `_docs.html` structure
- `website/templates/docs/index.html` — restyle with design system classes
- `website/templates/_partials/nav.html` — rewrite to `wf-mnav`
- `website/templates/_partials/footer.html` — rewrite to `mk-foot` + `mk-colophon`
- `website/templates/_partials/sidebar.html` — rewrite to `wf-docs-side`

**Delete:**
- `website/static/css/style.css`

---

### Task 1: Create branch and sync design system CSS

**Files:**
- Modify: `Justfile`
- Create: `website/static/css/wavefunk/` (copied from `../design/css/`)
- Create: `website/static/css/eigen.css`
- Delete: `website/static/css/style.css`

- [ ] **Step 1: Create feature branch**

```bash
git checkout -b feat/design-system-integration
```

- [ ] **Step 2: Add sync-design recipe to Justfile**

Replace the current Justfile with:

```just
# Sync design system CSS + fonts from sibling repo
sync-design:
    rm -rf website/static/css/wavefunk
    cp -r ../design/css website/static/css/wavefunk

# Build the website
website-build: sync-design
    cargo run -- build -p website -v

# Start dev server for the website
website-dev: sync-design
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

- [ ] **Step 3: Run sync-design**

```bash
just sync-design
```

Expected: `website/static/css/wavefunk/` directory created with `wavefunk.css`, `01-tokens.css` through `06-marketing.css`, and `fonts/` directory.

- [ ] **Step 4: Create eigen.css**

Create `website/static/css/eigen.css`:

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

- [ ] **Step 5: Delete old style.css**

```bash
rm website/static/css/style.css
```

- [ ] **Step 6: Add wavefunk CSS to .gitignore**

Append to `website/.gitignore` (or create it):

```
static/css/wavefunk/
```

The synced CSS should not be committed — it's copied from `../design` at build time.

- [ ] **Step 7: Commit**

```bash
git add Justfile website/static/css/eigen.css website/.gitignore
git rm website/static/css/style.css
git commit -m "feat: add sync-design recipe and eigen accent override"
```

---

### Task 2: Rewrite _base.html

**Files:**
- Modify: `website/templates/_base.html`

- [ ] **Step 1: Rewrite _base.html**

Replace `website/templates/_base.html` with:

```html
<!doctype html>
<html lang="en" data-mode="dark">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{% block title %}{{ site.name }}{% endblock %}</title>
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="{{ asset('/css/wavefunk/wavefunk.css') }}">
<link rel="stylesheet" href="{{ asset('/css/eigen.css') }}">
<script src="https://unpkg.com/htmx.org@2.0.4" defer></script>
{% block head %}{% endblock %}
</head>
<body>
{% block body %}{% endblock %}
</body>
</html>
```

Key changes from old version:
- Added `data-mode="dark"` on `<html>`
- Replaced `style.css` with `wavefunk.css` + `eigen.css`
- Removed inline JS (sidebar toggle, copy button) — will be added back where needed
- Changed from `<body hx-boost="true">` + `<main id="content">` structure to a bare `{% block body %}` — each page type (_marketing, _docs) provides its own body structure
- Added `{% block head %}` for per-page head additions

- [ ] **Step 2: Commit**

```bash
git add website/templates/_base.html
git commit -m "feat: rewrite _base.html for Wave Funk design system"
```

---

### Task 3: Create _marketing.html and rewrite nav + footer partials

**Files:**
- Create: `website/templates/_marketing.html`
- Modify: `website/templates/_partials/nav.html`
- Modify: `website/templates/_partials/footer.html`

- [ ] **Step 1: Create _marketing.html**

Create `website/templates/_marketing.html`:

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

- [ ] **Step 2: Rewrite nav.html**

Replace `website/templates/_partials/nav.html` with:

```html
<div class="mk-nav">
  <div class="wf-mnav" style="padding: 0;">
    <div class="wf-wordmark">
      <img src="/favicon.svg" alt="eigen" width="22" height="22">
      <span class="wf-wordmark-name">EIGEN</span>
    </div>
    {% for item in nav %}
      {% if item.external %}
        <a href="{{ item.url }}" target="_blank" rel="noopener">{{ item.label }}</a>
      {% else %}
        <a {{ link_to(item.url) }}>{{ item.label }}</a>
      {% endif %}
    {% endfor %}
    <div class="wf-mnav-spacer"></div>
  </div>
</div>
```

- [ ] **Step 3: Rewrite footer.html**

Replace `website/templates/_partials/footer.html` with:

```html
<footer>
  <div class="mk-foot">
    <div>
      <div class="wf-wordmark" style="margin-bottom: 14px;">
        <img src="/favicon.svg" alt="eigen" width="20" height="20">
        <span class="wf-wordmark-name">EIGEN</span>
      </div>
      <p style="font-size: 12px; color: var(--fg-muted); max-width: 32ch; line-height: 1.55; font-family: var(--font-mono);">A static site generator that thinks in fragments. Fast builds, htmx navigation, data from anywhere.</p>
    </div>
    <div>
      <div class="mk-foot-h">DOCUMENTATION</div>
      <a href="/docs/quickstart">Quick Start</a>
      <a href="/docs/clean-links">Templates</a>
      <a href="/docs/data-fetching">Data Sources</a>
      <a href="/docs/fragments">Fragments</a>
    </div>
    <div>
      <div class="mk-foot-h">PROJECT</div>
      <a href="https://github.com/wavefunk/eigen" target="_blank" rel="noopener">GitHub</a>
      <a href="https://github.com/wavefunk/eigen/blob/main/LICENSE" target="_blank" rel="noopener">License</a>
      <a href="https://github.com/wavefunk/eigen/issues" target="_blank" rel="noopener">Issues</a>
    </div>
    <div>
      <div class="mk-foot-h">RESOURCES</div>
      <a href="/docs/quickstart">Installation</a>
      <a href="/docs/site-config">Configuration</a>
      <a href="/docs/github-action">CI/CD</a>
    </div>
  </div>
  <div class="mk-colophon">
    <span>&copy; {{ current_year() }} EIGEN</span>
    <a href="https://wavefunk.io" target="_blank" rel="noopener" class="mk-colophon-badge">
      <svg width="14" height="12" viewBox="0 0 64 42" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><rect width="20" height="20" x="22" y="0" rx="4"/><rect width="20" height="20" x="44" y="0" rx="4"/><rect width="20" height="20" x="22" y="22" rx="4"/><rect width="20" height="20" x="0" y="22" rx="4"/></svg>
      BUILT BY WAVEFUNK
    </a>
    <span>OPEN SOURCE &middot; MIT</span>
  </div>
</footer>
```

- [ ] **Step 4: Verify marketing structure builds**

```bash
just website-build
```

Expected: Build succeeds. The homepage may look broken (index.html still uses old classes) but the build should complete without template errors.

- [ ] **Step 5: Commit**

```bash
git add website/templates/_marketing.html website/templates/_partials/nav.html website/templates/_partials/footer.html
git commit -m "feat: add _marketing.html wrapper, rewrite nav and footer for design system"
```

---

### Task 4: Rewrite index.html (homepage)

**Files:**
- Modify: `website/templates/index.html`

- [ ] **Step 1: Rewrite index.html**

Replace `website/templates/index.html` with:

```html
---
data:
  nav:
    file: "nav.yaml"
  features:
    file: "features.yaml"
seo:
  title: "eigen — a static site generator that thinks in fragments"
  description: "A Rust-powered static site generator with HTMX fragment navigation, data fetching, and built-in optimization."
schema:
  type: WebSite
---
{% extends "_marketing.html" %}

{% block title %}eigen — a static site generator that thinks in fragments{% endblock %}

{% block content %}
<section class="mk-hero">
  <div class="mk-hero-grid" aria-hidden="true"></div>
  <div class="mk-hero-inner">
    <div class="mk-hero-eyebrow">EIGEN &middot; STATIC SITE GENERATOR</div>
    <h1>Static sites that think in <em>fragments</em>.</h1>
    <p>A Rust-powered static site generator with htmx fragment navigation, data fetching from anywhere, and built-in optimization. Every template block becomes a swappable partial.</p>
    <div class="mk-hero-cta">
      <a href="/docs/quickstart" class="wf-btn lg primary">Get Started</a>
      <a href="https://github.com/wavefunk/eigen" class="wf-btn lg" target="_blank" rel="noopener">View on GitHub</a>
      <span class="sep"></span>
      <code class="shell-line"><span class="prompt">$</span>cargo install eigen</code>
    </div>
  </div>
  <div class="mk-hero-stats">
    <div><div class="l">FRAGMENTS</div><div class="v">&#x2713;</div></div>
    <div><div class="l">DATA SOURCES</div><div class="v">&#x221E;</div></div>
    <div><div class="l">BUILD</div><div class="v">ASYNC</div></div>
    <div><div class="l">RUNTIME JS</div><div class="v">0</div></div>
  </div>
</section>

<section class="mk-sect">
  <div class="mk-sect-head">
    <div>
      <div class="mk-sect-kicker">&mdash; 01 / FEATURES</div>
      <h2 class="mk-sect-title">Everything you need, nothing you don't.</h2>
    </div>
    <p class="mk-sect-sub">Templates, data fetching, optimization, and SEO — configured in one file, built in milliseconds.</p>
  </div>
  <div class="mk-features">
    {% for feature in features %}
    <div class="mk-feat">
      <div class="mk-feat-num">&mdash; {{ "%02d" | format(loop.index) }}</div>
      <h3 class="mk-feat-t">{{ feature.title }}</h3>
      <p class="mk-feat-b">{{ feature.description }}</p>
    </div>
    {% endfor %}
  </div>
</section>

<section class="mk-sect">
  <div class="mk-sect-head">
    <div>
      <div class="mk-sect-kicker">&mdash; 02 / HOW IT WORKS</div>
      <h2 class="mk-sect-title">Three steps to production.</h2>
    </div>
    <p class="mk-sect-sub">Write templates, configure data, build and deploy. No plugins, no framework lock-in.</p>
  </div>
  <div class="mk-steps">
    <div class="mk-step">
      <div class="mk-step-num">&mdash; 01</div>
      <h3 class="mk-step-t">Write templates</h3>
      <p class="mk-step-b">Jinja2 templates with frontmatter. Define data sources, SEO, and schema in YAML. Use blocks for fragment boundaries.</p>
    </div>
    <div class="mk-step">
      <div class="mk-step-num">&mdash; 02</div>
      <h3 class="mk-step-t">Configure data</h3>
      <p class="mk-step-b">Pull from local YAML, REST APIs, GraphQL, or Notion. One config block per source with caching and auth built in.</p>
    </div>
    <div class="mk-step">
      <div class="mk-step-num">&mdash; 03</div>
      <h3 class="mk-step-t">Build &amp; deploy</h3>
      <p class="mk-step-b">Async Rust pipeline renders pages, extracts fragments, optimizes assets. Deploy the dist folder to any CDN.</p>
    </div>
  </div>
</section>

<section class="mk-sect mk-code">
  <div class="mk-sect-head">
    <div>
      <div class="mk-sect-kicker">&mdash; 03 / QUICK START</div>
      <h2 class="mk-sect-title">Up and running in seconds.</h2>
    </div>
    <p class="mk-sect-sub">One config file. One build command. Zero JavaScript frameworks.</p>
  </div>
  <pre><code><span class="comment"># site.toml</span>
[site]
name = "my-site"
base_url = "https://example.com"

[build]
fragments = true
clean_links = true

<span class="comment"># Data from anywhere</span>
[sources.api]
url = "https://api.example.com"</code></pre>
</section>

<section class="mk-sect" style="text-align: center; padding: 100px 32px;">
  <div class="mk-sect-kicker" style="justify-content: center;">&mdash; 04 / START HERE</div>
  <h2 class="mk-sect-title" style="max-width: none; margin: 0 auto 20px;">Build your next site with eigen.</h2>
  <p class="mk-sect-sub" style="margin: 0 auto 32px;">One config file. Millisecond builds. Fragment navigation out of the box.</p>
  <div style="display: flex; gap: 12px; justify-content: center;">
    <a href="/docs/quickstart" class="wf-btn lg primary">Get Started</a>
    <a href="/docs/index" class="wf-btn lg">Read the Docs</a>
  </div>
</section>
{% endblock %}
```

- [ ] **Step 2: Build and verify**

```bash
just website-build
```

Expected: Build succeeds. Homepage renders with Wave Funk marketing layout.

- [ ] **Step 3: Commit**

```bash
git add website/templates/index.html
git commit -m "feat: rewrite homepage with Wave Funk marketing classes"
```

---

### Task 5: Rewrite _docs.html, sidebar, and add TOC partial

**Files:**
- Modify: `website/templates/_docs.html`
- Modify: `website/templates/_partials/sidebar.html`
- Create: `website/templates/_partials/docs-toc.html`

- [ ] **Step 1: Rewrite _docs.html**

Replace `website/templates/_docs.html` with:

```html
{% extends "_base.html" %}

{% block body %}
<div class="wf-docs-shell">
  {% block sidebar %}
    {% include "_partials/sidebar.html" %}
  {% endblock %}

  {% block doc_content %}
    <div id="doc-content" class="wf-docs-content">
      <article class="wf-prose">
        <div class="wf-crumbs">
          <a href="/">HOME</a><span class="sep">/</span>
          <a href="/docs/index">DOCS</a><span class="sep">/</span>
          {% if doc.category is defined %}
            <span>{{ doc.category | upper }}</span><span class="sep">/</span>
          {% endif %}
          <span aria-current="page">{{ doc.title | upper }}</span>
        </div>

        <h1>{{ doc.title }}</h1>
        {% if doc.description is defined and doc.description %}
          <p class="wf-lead">{{ doc.description }}</p>
        {% endif %}

        <div id="docs-content">
          {% block docs_body %}{% endblock %}
        </div>
      </article>
      <div class="wf-docs-pager">
        {% if doc.prev_slug is defined and doc.prev_slug %}
          <a href="/docs/{{ doc.prev_slug }}"
             hx-get="/_fragments/docs/{{ doc.prev_slug }}/doc_content.html"
             hx-target="#doc-content"
             hx-swap="outerHTML"
             hx-push-url="/docs/{{ doc.prev_slug }}">
            <span class="wf-docs-pager-k">&larr; PREV</span>
            <span class="wf-docs-pager-t">{{ doc.prev_title }}</span>
          </a>
        {% else %}
          <span></span>
        {% endif %}
        {% if doc.next_slug is defined and doc.next_slug %}
          <a href="/docs/{{ doc.next_slug }}" class="next"
             hx-get="/_fragments/docs/{{ doc.next_slug }}/doc_content.html"
             hx-target="#doc-content"
             hx-swap="outerHTML"
             hx-push-url="/docs/{{ doc.next_slug }}">
            <span class="wf-docs-pager-k">NEXT &rarr;</span>
            <span class="wf-docs-pager-t">{{ doc.next_title }}</span>
          </a>
        {% else %}
          <span></span>
        {% endif %}
      </div>
    </div>
  {% endblock %}

  {% include "_partials/docs-toc.html" %}
</div>

<script>
function buildToc() {
  var content = document.getElementById('docs-content');
  var toc = document.getElementById('docs-toc-links');
  if (!content || !toc) return;
  while (toc.firstChild) toc.removeChild(toc.firstChild);
  var headings = content.querySelectorAll('h2');
  headings.forEach(function(h) {
    var id = h.textContent.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '');
    h.id = id;
    var a = document.createElement('a');
    a.href = '#' + id;
    a.textContent = h.textContent;
    toc.appendChild(a);
  });
}
buildToc();
document.body.addEventListener('htmx:afterSettle', function() {
  buildToc();
  window.scrollTo(0, 0);
});
document.addEventListener('click', function(e) {
  var a = e.target.closest('a[href]');
  if (!a) return;
  var href = a.getAttribute('href').replace(/\/+$/, '');
  var path = window.location.pathname.replace(/\/+$/, '');
  if (href === path) e.preventDefault();
});
</script>
{% endblock %}
```

- [ ] **Step 2: Rewrite sidebar.html**

Replace `website/templates/_partials/sidebar.html` with:

```html
<aside class="wf-docs-side" id="sidebar">
  <a href="/" class="wf-brand">
    <img src="/favicon.svg" alt="eigen" width="24" height="24">
    <div>
      <div class="wf-brand-name">eigen</div>
      <div class="wf-caption">Docs</div>
    </div>
  </a>

  <div>
    {% for cat in categories %}
    <div class="wf-docs-side-section">{{ cat.name }}</div>
    {% for d in docs_list %}
      {% if d.category == cat.name %}
      <a href="/docs/{{ d.slug }}"
         hx-get="/_fragments/docs/{{ d.slug }}/doc_content.html"
         hx-target="#doc-content"
         hx-swap="outerHTML"
         hx-push-url="/docs/{{ d.slug }}"
         {% if d.slug == doc.slug %} class="is-active"{% endif %}>
        {{ d.title }}
      </a>
      {% endif %}
    {% endfor %}
    {% endfor %}
  </div>

  <a href="https://wavefunk.io" target="_blank" rel="noopener" style="display: flex; align-items: center; gap: 6px; padding: 12px 20px; margin-top: auto; font-size: 10px; letter-spacing: 0.12em; text-transform: uppercase; color: var(--fg-faint); text-decoration: none; border-top: 1px solid var(--hairline);">
    <svg width="12" height="10" viewBox="0 0 64 42" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><rect width="20" height="20" x="22" y="0" rx="4"/><rect width="20" height="20" x="44" y="0" rx="4"/><rect width="20" height="20" x="22" y="22" rx="4"/><rect width="20" height="20" x="0" y="22" rx="4"/></svg>
    Built by Wavefunk
  </a>
</aside>
```

- [ ] **Step 3: Create docs-toc.html**

Create `website/templates/_partials/docs-toc.html`:

```html
<aside class="wf-toc">
  <h4>ON THIS PAGE</h4>
  <div id="docs-toc-links"></div>

  <h4 class="wf-mt-6">ACTIONS</h4>
  <a href="https://github.com/wavefunk/eigen/edit/master/website/docs/" target="_blank" rel="noopener">Edit on GitHub &#x2197;</a>
</aside>
```

- [ ] **Step 4: Commit**

```bash
git add website/templates/_docs.html website/templates/_partials/sidebar.html website/templates/_partials/docs-toc.html
git commit -m "feat: rewrite docs layout with 3-column wf-docs-shell"
```

---

### Task 6: Update doc page templates

**Files:**
- Modify: `website/templates/docs/[doc].html`
- Modify: `website/templates/docs/index.html`

- [ ] **Step 1: Update [doc].html**

Replace `website/templates/docs/[doc].html` with:

```html
---
collection:
  file: "docs.yaml"
slug_field: slug
item_as: doc
fragment_blocks:
  - doc_content
  - sidebar
data:
  nav:
    file: "nav.yaml"
  docs_list:
    file: "docs.yaml"
  categories:
    file: "categories.yaml"
seo:
  title: "{{ doc.title }} — eigen docs"
  description: "{{ doc.description }}"
schema:
  type: Article
  breadcrumb_names:
    docs: "Documentation"
---
{% extends "_docs.html" %}

{% block title %}{{ doc.title }} — {{ site.name }} docs{% endblock %}

{% block docs_body %}
{{ doc.content | markdown }}
{% endblock %}
```

Key change: `{% block doc_content %}` → `{% block docs_body %}`. The `doc_content` block is now defined in `_docs.html` as the outer wrapper (with crumbs, pager, etc.). The individual doc page only fills the inner `docs_body` block with rendered markdown.

- [ ] **Step 2: Update docs/index.html**

Replace `website/templates/docs/index.html` with:

```html
---
data:
  nav:
    file: "nav.yaml"
  docs_list:
    file: "docs.yaml"
  categories:
    file: "categories.yaml"
seo:
  title: "Documentation — eigen"
  description: "Comprehensive documentation for eigen — templates, data fetching, optimization, SEO, and more."
schema:
  type: WebSite
  breadcrumb_names:
    docs: "Documentation"
---
{% extends "_marketing.html" %}

{% block title %}Documentation — {{ site.name }}{% endblock %}

{% block content %}
<section class="mk-sect">
  <div class="mk-sect-head">
    <div>
      <div class="mk-sect-kicker">&mdash; REFERENCE</div>
      <h2 class="mk-sect-title">Documentation</h2>
    </div>
    <p class="mk-sect-sub">Everything you need to build fast, fragment-driven static sites with eigen.</p>
  </div>

  <div class="mk-features">
    {% for cat in categories %}
    <div class="mk-feat">
      <div class="mk-feat-num">&mdash; {{ "%02d" | format(loop.index) }}</div>
      <h3 class="mk-feat-t">{{ cat.name }}</h3>
      <p class="mk-feat-b">{{ cat.desc }}</p>
      <div style="margin-top: auto; padding-top: 16px;">
        {% for d in docs_list %}
          {% if d.category == cat.name %}
          <a href="/docs/{{ d.slug }}" style="display: block; font-family: var(--font-mono); font-size: 11px; letter-spacing: 0.04em; text-transform: uppercase; color: var(--fg-muted); text-decoration: none; padding: 3px 0;">{{ d.title }}</a>
          {% endif %}
        {% endfor %}
      </div>
    </div>
    {% endfor %}
  </div>
</section>
{% endblock %}
```

- [ ] **Step 3: Build and verify**

```bash
just website-build
```

Expected: Build succeeds. All doc pages render with the 3-column layout. Doc index page uses marketing feature grid.

- [ ] **Step 4: Commit**

```bash
git add website/templates/docs/\[doc\].html website/templates/docs/index.html
git commit -m "feat: update doc templates for design system"
```

---

### Task 7: Visual verification and cleanup

- [ ] **Step 1: Start dev server**

```bash
just website-dev
```

- [ ] **Step 2: Verify homepage**

Open `http://localhost:3000` (or whatever port eigen dev uses). Check:
- Hero section renders with grid background, eyebrow, large title, CTA buttons, stat strip
- Features grid shows all 8 features from features.yaml in numbered cards
- How It Works shows 3 steps
- Quick Start code block renders
- CTA section centered
- Footer has 4 columns with correct links
- Colophon shows Wavefunk badge SVG

- [ ] **Step 3: Verify docs**

Open `http://localhost:3000/docs/quickstart`. Check:
- 3-column layout: sidebar, prose, TOC
- Sidebar shows categories with links, current page highlighted with `is-active`
- Breadcrumbs show HOME / DOCS / GETTING STARTED / QUICKSTART
- TOC builds dynamically from h2 headings
- Click a sidebar link: htmx swaps `#doc-content` without full page reload
- Prev/next pager appears at bottom (if the doc has prev/next configured)

- [ ] **Step 4: Verify docs index**

Open `http://localhost:3000/docs/index`. Check:
- Uses marketing layout with feature grid
- Each category shows as a card with doc links

- [ ] **Step 5: Fix any issues found**

Address any rendering issues, broken links, or template errors discovered during verification.

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix: post-verification cleanup for design system integration"
```
