# Eigen Website Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current single-file `website/index.html` with eigen's own website built using eigen — warm Gruvbox-dark palette, Recursive font, landing page + categorized docs section.

**Architecture:** Eigen site in `website/` directory. Static YAML data in `_data/`. Dynamic `[doc].html` template for doc pages driven by `docs.yaml` collection. HTMX fragment navigation in docs via `doc_content` block. All eigen optimization features enabled.

**Tech Stack:** Eigen (SSG), Minijinja (templates), HTMX 2.0, Recursive font (self-hosted woff2), CSS (no framework)

**Spec:** `docs/superpowers/specs/2026-04-11-eigen-website-design.md`

---

### Task 1: Project Scaffolding

**Files:**
- Remove: `website/index.html`
- Create: `website/site.toml`
- Create: `website/templates/` (empty dir, populated in later tasks)
- Create: `website/_data/` (empty dir, populated in later tasks)
- Create: `website/static/css/` (empty dir)
- Create: `website/static/fonts/` (empty dir)
- Create: `Justfile`

- [ ] **Step 1: Create directory structure**

```bash
cd /home/nambiar/projects/wavefunk/eigen
rm website/index.html
mkdir -p website/templates/_partials
mkdir -p website/templates/docs
mkdir -p website/_data
mkdir -p website/static/css
mkdir -p website/static/fonts
```

- [ ] **Step 2: Create site.toml**

Write `website/site.toml`:

```toml
[site]
name = "eigen"
base_url = "https://eigen.wavefunk.dev"

[site.seo]
title = "eigen — a static site generator that thinks in fragments"
description = "A Rust-powered static site generator with HTMX fragment navigation, data fetching, and built-in optimization."
og_type = "website"

[site.schema]
default_types = ["BreadcrumbList"]

[build]
fragments = true
fragment_dir = "_fragments"
content_block = "content"
minify = true
clean_urls = false
clean_links = true
not_found = true

[build.critical_css]
enabled = true
max_inline_size = 50000
preload_full = true

[build.hints]
enabled = true
auto_detect_hero = true
prefetch_links = true
max_prefetch = 5

[build.content_hash]
enabled = true

[build.bundling]
enabled = true
css = true
js = true
tree_shake_css = true

[build.view_transitions]
enabled = true

[sitemap]
enabled = true
clean_urls = false

[robots]
enabled = true
sitemap = true

[[robots.rules]]
user_agent = "*"
allow = ["/"]
```

- [ ] **Step 3: Create Justfile**

Write `Justfile` at the project root:

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

- [ ] **Step 4: Commit**

```bash
git add -A website/ Justfile
git commit -m "chore: scaffold eigen website project structure"
```

---

### Task 2: Font Files

**Files:**
- Create: `website/static/fonts/recursive-heading.woff2`
- Create: `website/static/fonts/recursive-body.woff2`

We need two static woff2 instances of Recursive:
- **Heading:** CASL=0.5, wght=800, MONO=0 (~15KB)
- **Body/Code:** CASL=0, wght=300..500, MONO=0..1 (~15KB)

- [ ] **Step 1: Download Recursive font files**

Download the Recursive variable font woff2 from Google Fonts. Use the CSS2 API with a Chrome user-agent to get woff2 URLs:

```bash
# Get the CSS with woff2 URLs for heading instance
curl -s -H "User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36" \
  "https://fonts.googleapis.com/css2?family=Recursive:wght,CASL@800,0.5&display=swap" > /tmp/recursive-heading.css

# Extract the woff2 URL and download it
grep -oP 'url\(\K[^)]+\.woff2' /tmp/recursive-heading.css | head -1 | xargs curl -s -o website/static/fonts/recursive-heading.woff2

# Get the CSS with woff2 URLs for body instance
curl -s -H "User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36" \
  "https://fonts.googleapis.com/css2?family=Recursive:wght,CASL,MONO@400,0,0&display=swap" > /tmp/recursive-body.css

# Extract the woff2 URL and download it
grep -oP 'url\(\K[^)]+\.woff2' /tmp/recursive-body.css | head -1 | xargs curl -s -o website/static/fonts/recursive-body.woff2
```

If the Google Fonts API doesn't serve the exact axis combination, download the full variable font from `https://github.com/arrowtype/recursive/releases` and use it as a single file. In that case, use `@font-face` with `font-variation-settings` to select axes at usage sites.

- [ ] **Step 2: Verify font files exist and have reasonable size**

```bash
ls -lh website/static/fonts/
```

Expected: Two woff2 files, each under 50KB. If a single variable font is used instead, it should be under 300KB (acceptable — subsetting can happen later).

- [ ] **Step 3: Commit**

```bash
git add website/static/fonts/
git commit -m "feat(website): add Recursive font files"
```

---

### Task 3: Data Files

**Files:**
- Create: `website/_data/nav.yaml`
- Create: `website/_data/features.yaml`
- Create: `website/_data/docs.yaml`

- [ ] **Step 1: Create nav.yaml**

Write `website/_data/nav.yaml`:

```yaml
- label: Docs
  url: /docs/
- label: GitHub
  url: https://github.com/wavefunk/eigen
  external: true
```

- [ ] **Step 2: Create features.yaml**

Write `website/_data/features.yaml`:

```yaml
- title: HTMX Fragments
  description: Every template block becomes a standalone fragment. Click a link, swap a div — no full page reload. Navigation that feels like an SPA, built with zero JavaScript frameworks.

- title: Data From Anywhere
  description: Pull data from local YAML files, REST APIs, Notion, or GraphQL. One config block per source. Caching, rate limiting, and auth headers built in.

- title: Dynamic Pages
  description: Name a template [slug].html and point it at a collection. Eigen generates one page per item — blog posts, product pages, docs — from a single template.

- title: Built-in Optimization
  description: Critical CSS inlining, image lazy loading, CSS/JS bundling with tree-shaking, content-hashed assets, HTML minification. All on by default.

- title: Incremental Builds
  description: Only re-renders pages whose templates, data, or config actually changed. Two-tier hashing keeps rebuilds fast even on large sites.

- title: Single Config
  description: One site.toml file configures everything — data sources, SEO defaults, build optimization, analytics, redirects, feeds. No plugin ecosystem to navigate.

- title: SEO Out of the Box
  description: Auto-generated sitemap, robots.txt, Open Graph tags, Twitter cards, JSON-LD structured data. Configure once at the site level, override per page.

- title: Rust Performance
  description: Async build pipeline on Tokio. Concurrent page rendering, parallel data fetching, non-blocking I/O. Builds your site as fast as your machine allows.
```

- [ ] **Step 3: Create docs.yaml**

This is the largest data file. It contains all doc content embedded as YAML multiline strings. Read each file in `docs/` and create an entry.

**CRITICAL: YAML formatting rules for embedded markdown:**
- Use the `|` (literal block scalar) indicator for the `content` field
- Indent all content lines by exactly 4 spaces relative to the `content:` key
- The `|` block scalar preserves newlines and is terminated by a line at lesser indentation
- Lines that look like YAML (starting with `-`, `#`, or containing `:`) are safe inside `|` blocks
- Horizontal rules (`---`) in markdown are safe inside `|` blocks
- **Test that the YAML parses** after creating the file: `python3 -c "import yaml; yaml.safe_load(open('website/_data/docs.yaml'))"`

Write `website/_data/docs.yaml` with the following structure. For each entry, read the corresponding file from `docs/` and embed its content.

**Category mapping** (use this exact mapping for the `category` field):

| Category | Source files |
|----------|-------------|
| Getting Started | (quickstart — added in Task 8), `github_action.md` |
| Templating | `draft_pages.md`, `clean_links.md` |
| Data | `data_cache.md`, `post_method.md`, `rate_limiting.md`, `source_asset.md` |
| Optimization | `async_build.md`, `incremental_builds.md`, `content_hashing.md`, `critical_css.md`, `css_js_bundling.md`, `lazy_loading.md`, `preload_prefetch.md` |
| SEO & Meta | `audit.md`, `json_ld.md`, `og_twitter_meta.md`, `robots_txt.md`, `rss_feed.md`, `redirects.md` |
| Frontend | `analytics.md`, `view_transitions.md` |

**Category order in the file:** Getting Started, Templating, Data, Optimization, SEO & Meta, Frontend. Within each category, maintain the order listed above.

**Entry format:**

```yaml
- title: "Content Hashing"
  slug: content-hashing
  category: Optimization
  description: "Fingerprints static assets with SHA-256 hashes for cache busting."
  content: |
    # Content Hashing

    Eigen fingerprints static assets...
    (full markdown content from docs/content_hashing.md goes here, indented 4 spaces)
```

**Slug derivation:** Take the filename, remove `.md`, replace underscores with hyphens. Examples:
- `content_hashing.md` → `content-hashing`
- `css_js_bundling.md` → `css-js-bundling`
- `og_twitter_meta.md` → `og-twitter-meta`
- `rss_feed.md` → `rss-feed`
- `github_action.md` → `github-action`

**Title derivation:** Read the first `# Heading` from each markdown file. If there's no heading, derive from filename in Title Case.

**Description derivation:** Use the first non-heading paragraph from the markdown, truncated to one sentence.

**Content:** The FULL markdown content from the source file. Do not truncate or summarize.

Read each of these files from `docs/` and create the entry:
1. `docs/github_action.md` → category: Getting Started
2. `docs/draft_pages.md` → category: Templating
3. `docs/clean_links.md` → category: Templating
4. `docs/data_cache.md` → category: Data
5. `docs/post_method.md` → category: Data
6. `docs/rate_limiting.md` → category: Data
7. `docs/source_asset.md` → category: Data
8. `docs/async_build.md` → category: Optimization
9. `docs/incremental_builds.md` → category: Optimization
10. `docs/content_hashing.md` → category: Optimization
11. `docs/critical_css.md` → category: Optimization
12. `docs/css_js_bundling.md` → category: Optimization
13. `docs/lazy_loading.md` → category: Optimization
14. `docs/preload_prefetch.md` → category: Optimization
15. `docs/audit.md` → category: SEO & Meta
16. `docs/json_ld.md` → category: SEO & Meta
17. `docs/og_twitter_meta.md` → category: SEO & Meta
18. `docs/robots_txt.md` → category: SEO & Meta
19. `docs/rss_feed.md` → category: SEO & Meta
20. `docs/redirects.md` → category: SEO & Meta
21. `docs/analytics.md` → category: Frontend
22. `docs/view_transitions.md` → category: Frontend

- [ ] **Step 4: Validate the YAML parses**

```bash
python3 -c "import yaml; data = yaml.safe_load(open('website/_data/docs.yaml')); print(f'{len(data)} docs loaded')"
```

Expected: `22 docs loaded` (quickstart added in Task 8)

- [ ] **Step 5: Commit**

```bash
git add website/_data/
git commit -m "feat(website): add static data files for nav, features, and docs"
```

---

### Task 4: Stylesheet

**Files:**
- Create: `website/static/css/style.css`

- [ ] **Step 1: Write the complete stylesheet**

Write `website/static/css/style.css`:

```css
/* ============================================
   eigen website — warm gruvbox theme
   ============================================ */

/* --- Variables --- */
:root {
  --bg-dark: #1d2021;
  --bg: #282828;
  --bg-light: #3c3836;
  --bg-lighter: #504945;
  --fg: #ebdbb2;
  --fg-dim: #a89984;
  --orange: #e78a4e;
  --yellow: #d8a657;
  --red: #ea6962;
  --aqua: #89b482;
  --blue: #7daea3;
  --purple: #d3869b;

  --font-heading: 'Recursive', system-ui, sans-serif;
  --font-body: 'Recursive', system-ui, sans-serif;
  --font-mono: 'Recursive', ui-monospace, 'SFMono-Regular', Menlo, monospace;

  --space-xs: 0.25rem;
  --space-sm: 0.5rem;
  --space-md: 1rem;
  --space-lg: 2rem;
  --space-xl: 4rem;

  --sidebar-width: 260px;
  --max-width: 1200px;
  --nav-height: 60px;
}

/* --- Font Faces --- */
@font-face {
  font-family: 'Recursive';
  src: url('/fonts/recursive-heading.woff2') format('woff2');
  font-weight: 800;
  font-style: normal;
  font-display: swap;
}

@font-face {
  font-family: 'Recursive';
  src: url('/fonts/recursive-body.woff2') format('woff2');
  font-weight: 300 500;
  font-style: normal;
  font-display: swap;
}

/* --- Reset --- */
*, *::before, *::after {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

/* --- Base --- */
html {
  font-size: 16px;
  scroll-behavior: smooth;
}

body {
  font-family: var(--font-body);
  font-weight: 400;
  line-height: 1.7;
  color: var(--fg);
  background: var(--bg-dark);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

/* --- Typography --- */
h1, h2, h3, h4 {
  font-family: var(--font-heading);
  font-weight: 800;
  line-height: 1.2;
  color: var(--fg);
}

h1 { font-size: clamp(2rem, 5vw, 3.5rem); }
h2 { font-size: clamp(1.5rem, 3vw, 2.25rem); }
h3 { font-size: clamp(1.125rem, 2vw, 1.5rem); }
h4 { font-size: 1rem; }

a {
  color: var(--orange);
  text-decoration: none;
  transition: color 0.15s;
}

a:hover {
  color: var(--yellow);
  text-decoration: underline;
}

code, pre {
  font-family: var(--font-mono);
}

code {
  background: var(--bg);
  padding: 0.15em 0.4em;
  border-radius: 4px;
  font-size: 0.9em;
  color: var(--aqua);
}

pre {
  background: var(--bg);
  padding: var(--space-md) var(--space-lg);
  border-radius: 8px;
  overflow-x: auto;
  border: 1px solid var(--bg-lighter);
  line-height: 1.5;
}

pre code {
  background: none;
  padding: 0;
  font-size: 0.875rem;
  color: var(--fg);
}

/* --- Navigation --- */
.nav {
  position: sticky;
  top: 0;
  z-index: 100;
  background: var(--bg-dark);
  border-bottom: 1px solid var(--bg-lighter);
  height: var(--nav-height);
}

.nav-inner {
  max-width: var(--max-width);
  margin: 0 auto;
  padding: 0 var(--space-lg);
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 100%;
}

.nav-logo {
  font-family: var(--font-heading);
  font-weight: 800;
  font-size: 1.5rem;
  color: var(--fg);
  text-decoration: none;
}

.nav-logo:hover {
  color: var(--fg);
  text-decoration: none;
}

.nav-links {
  display: flex;
  gap: var(--space-lg);
  list-style: none;
  align-items: center;
}

.nav-links a {
  color: var(--fg-dim);
  font-family: var(--font-mono);
  font-size: 0.875rem;
  font-weight: 500;
  letter-spacing: 0.03em;
}

.nav-links a:hover {
  color: var(--orange);
  text-decoration: none;
}

.nav-toggle {
  display: none;
  background: none;
  border: none;
  cursor: pointer;
  padding: var(--space-sm);
}

.nav-toggle span {
  display: block;
  width: 24px;
  height: 2px;
  background: var(--fg);
  margin: 5px 0;
  transition: all 0.3s;
}

/* --- Hero --- */
.hero {
  max-width: var(--max-width);
  margin: 0 auto;
  padding: var(--space-xl) var(--space-lg);
  padding-top: calc(var(--space-xl) * 2);
  text-align: center;
}

.hero-title {
  font-size: clamp(3rem, 8vw, 6rem);
  font-family: var(--font-heading);
  font-weight: 800;
  color: var(--fg);
  margin-bottom: var(--space-sm);
  letter-spacing: -0.02em;
}

.hero-tagline {
  font-size: clamp(1.125rem, 2vw, 1.5rem);
  color: var(--fg-dim);
  margin-bottom: var(--space-xl);
  max-width: 600px;
  margin-left: auto;
  margin-right: auto;
}

.hero-actions {
  display: flex;
  gap: var(--space-md);
  justify-content: center;
  flex-wrap: wrap;
  margin-bottom: var(--space-xl);
}

.btn {
  display: inline-block;
  padding: 0.75rem 1.75rem;
  border-radius: 6px;
  font-family: var(--font-mono);
  font-size: 0.875rem;
  font-weight: 500;
  text-decoration: none;
  transition: background 0.2s, color 0.2s;
  border: none;
  cursor: pointer;
}

.btn:hover {
  text-decoration: none;
}

.btn-primary {
  background: var(--orange);
  color: var(--bg-dark);
}

.btn-primary:hover {
  background: var(--yellow);
  color: var(--bg-dark);
}

.btn-secondary {
  background: var(--bg-light);
  color: var(--fg);
  border: 1px solid var(--bg-lighter);
}

.btn-secondary:hover {
  background: var(--bg-lighter);
  color: var(--fg);
}

.hero-code {
  max-width: 700px;
  margin: 0 auto;
  text-align: left;
}

.code-label {
  font-family: var(--font-mono);
  font-size: 0.75rem;
  color: var(--fg-dim);
  text-transform: uppercase;
  letter-spacing: 0.1em;
  margin-bottom: var(--space-sm);
  display: block;
}

/* --- Section shared --- */
.section {
  max-width: var(--max-width);
  margin: 0 auto;
  padding: var(--space-xl) var(--space-lg);
}

.section-title {
  text-align: center;
  margin-bottom: var(--space-xl);
}

/* --- Features --- */
.features-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: var(--space-lg);
}

.feature-card {
  background: var(--bg);
  padding: var(--space-lg);
  border-radius: 8px;
  border: 1px solid var(--bg-lighter);
  transition: border-color 0.2s;
}

.feature-card:hover {
  border-color: var(--orange);
}

.feature-card h3 {
  font-size: 1.125rem;
  margin-bottom: var(--space-sm);
  color: var(--orange);
}

.feature-card p {
  color: var(--fg-dim);
  font-size: 0.9375rem;
  line-height: 1.6;
}

/* --- How It Works --- */
.steps {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-xl);
}

.step {
  text-align: center;
}

.step-number {
  font-family: var(--font-mono);
  font-size: 0.875rem;
  color: var(--orange);
  font-weight: 700;
  margin-bottom: var(--space-sm);
  display: block;
}

.step h3 {
  margin-bottom: var(--space-md);
}

.step pre {
  text-align: left;
  font-size: 0.8125rem;
}

/* --- CTA --- */
.cta {
  text-align: center;
}

.cta-install {
  display: inline-flex;
  align-items: center;
  gap: var(--space-md);
  background: var(--bg);
  border: 1px solid var(--bg-lighter);
  border-radius: 8px;
  padding: var(--space-md) var(--space-lg);
  margin-bottom: var(--space-lg);
}

.cta-install code {
  font-size: 1.125rem;
  background: none;
  color: var(--aqua);
}

.cta-copy {
  background: none;
  border: none;
  cursor: pointer;
  color: var(--fg-dim);
  padding: var(--space-xs);
  font-size: 1rem;
  transition: color 0.15s;
}

.cta-copy:hover {
  color: var(--fg);
}

/* --- Docs Layout --- */
.docs-layout {
  max-width: var(--max-width);
  margin: 0 auto;
  display: grid;
  grid-template-columns: var(--sidebar-width) 1fr;
  min-height: calc(100vh - var(--nav-height));
}

/* --- Sidebar --- */
.sidebar {
  position: sticky;
  top: var(--nav-height);
  height: calc(100vh - var(--nav-height));
  overflow-y: auto;
  padding: var(--space-lg) var(--space-md);
  background: var(--bg);
  border-right: 1px solid var(--bg-lighter);
}

.sidebar-category {
  margin-bottom: var(--space-lg);
}

.sidebar-category > summary {
  list-style: none;
  cursor: pointer;
  user-select: none;
}

.sidebar-category > summary::-webkit-details-marker {
  display: none;
}

.sidebar-category-title {
  font-family: var(--font-mono);
  font-size: 0.6875rem;
  font-weight: 500;
  color: var(--fg-dim);
  text-transform: uppercase;
  letter-spacing: 0.1em;
  margin-bottom: var(--space-sm);
  padding: 0 var(--space-sm);
}

.sidebar-link {
  display: block;
  padding: var(--space-xs) var(--space-sm);
  color: var(--fg-dim);
  font-size: 0.875rem;
  border-radius: 4px;
  text-decoration: none;
  transition: background 0.15s, color 0.15s;
}

.sidebar-link:hover {
  background: var(--bg-light);
  color: var(--fg);
  text-decoration: none;
}

.sidebar-link.active {
  background: var(--bg-light);
  color: var(--orange);
}

/* --- Doc Content (prose) --- */
.doc-content {
  padding: var(--space-xl);
  max-width: 800px;
}

.doc-content h1 {
  margin-bottom: var(--space-lg);
  padding-bottom: var(--space-md);
  border-bottom: 1px solid var(--bg-lighter);
}

.doc-content h2 {
  margin-top: var(--space-xl);
  margin-bottom: var(--space-md);
  color: var(--yellow);
}

.doc-content h3 {
  margin-top: var(--space-lg);
  margin-bottom: var(--space-sm);
}

.doc-content p {
  margin-bottom: var(--space-md);
}

.doc-content ul, .doc-content ol {
  margin-bottom: var(--space-md);
  padding-left: var(--space-lg);
}

.doc-content li {
  margin-bottom: var(--space-xs);
}

.doc-content li > p {
  margin-bottom: var(--space-xs);
}

.doc-content pre {
  margin-bottom: var(--space-md);
}

.doc-content table {
  width: 100%;
  border-collapse: collapse;
  margin-bottom: var(--space-md);
  font-size: 0.9375rem;
}

.doc-content th,
.doc-content td {
  padding: var(--space-sm) var(--space-md);
  border: 1px solid var(--bg-lighter);
  text-align: left;
}

.doc-content th {
  background: var(--bg);
  font-weight: 600;
}

.doc-content blockquote {
  border-left: 3px solid var(--orange);
  padding-left: var(--space-md);
  color: var(--fg-dim);
  margin-bottom: var(--space-md);
  font-style: italic;
}

.doc-content hr {
  border: none;
  border-top: 1px solid var(--bg-lighter);
  margin: var(--space-xl) 0;
}

/* Code block copy button */
.doc-content pre {
  position: relative;
}

.doc-content pre .copy-btn {
  position: absolute;
  top: var(--space-sm);
  right: var(--space-sm);
  background: var(--bg-light);
  border: 1px solid var(--bg-lighter);
  color: var(--fg-dim);
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  font-family: var(--font-mono);
  font-size: 0.6875rem;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s;
}

.doc-content pre:hover .copy-btn {
  opacity: 1;
}

.doc-content pre .copy-btn:hover {
  background: var(--bg-lighter);
  color: var(--fg);
}

.doc-content img {
  max-width: 100%;
  border-radius: 8px;
}

/* --- Docs Index --- */
.docs-index {
  max-width: var(--max-width);
  margin: 0 auto;
  padding: var(--space-xl) var(--space-lg);
}

.docs-index-intro {
  max-width: 700px;
  margin-bottom: var(--space-xl);
}

.docs-index-intro p {
  color: var(--fg-dim);
  font-size: 1.125rem;
  line-height: 1.7;
}

.category-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  gap: var(--space-lg);
}

.category-card {
  background: var(--bg);
  padding: var(--space-lg);
  border-radius: 8px;
  border: 1px solid var(--bg-lighter);
}

.category-card h3 {
  color: var(--orange);
  margin-bottom: var(--space-xs);
  font-size: 1.125rem;
}

.category-card-desc {
  color: var(--fg-dim);
  font-size: 0.875rem;
  margin-bottom: var(--space-md);
}

.category-card ul {
  list-style: none;
  padding: 0;
}

.category-card li {
  margin-bottom: var(--space-xs);
}

.category-card a {
  font-size: 0.875rem;
}

/* --- Footer --- */
.footer {
  border-top: 1px solid var(--bg-lighter);
  padding: var(--space-lg);
  text-align: center;
  color: var(--fg-dim);
  font-size: 0.875rem;
}

.footer a {
  color: var(--fg-dim);
}

.footer a:hover {
  color: var(--orange);
}

/* --- 404 --- */
.not-found {
  text-align: center;
  padding: calc(var(--space-xl) * 2) var(--space-lg);
}

.not-found h1 {
  font-size: clamp(5rem, 15vw, 10rem);
  color: var(--bg-lighter);
  line-height: 1;
  margin-bottom: var(--space-md);
}

.not-found p {
  color: var(--fg-dim);
  margin-bottom: var(--space-lg);
  font-size: 1.125rem;
}

/* --- Responsive: Tablet --- */
@media (max-width: 1024px) {
  .docs-layout {
    grid-template-columns: 1fr;
  }

  .sidebar {
    position: fixed;
    left: -100%;
    top: var(--nav-height);
    width: 280px;
    height: calc(100vh - var(--nav-height));
    z-index: 90;
    transition: left 0.3s ease;
  }

  .sidebar.open {
    left: 0;
  }

  .sidebar-overlay {
    display: none;
    position: fixed;
    inset: 0;
    top: var(--nav-height);
    background: rgba(0, 0, 0, 0.5);
    z-index: 80;
  }

  .sidebar-overlay.open {
    display: block;
  }

  .nav-toggle {
    display: block;
  }

  .doc-content {
    padding: var(--space-lg);
  }

  .steps {
    grid-template-columns: 1fr;
    gap: var(--space-xl);
  }
}

/* --- Responsive: Mobile --- */
@media (max-width: 768px) {
  .hero {
    padding-top: var(--space-xl);
  }

  .hero-actions {
    flex-direction: column;
    align-items: center;
  }

  .btn {
    width: 100%;
    max-width: 280px;
    text-align: center;
  }

  .nav-links {
    display: none;
  }

  .nav-toggle {
    display: block;
  }

  .features-grid {
    grid-template-columns: 1fr;
  }

  .category-cards {
    grid-template-columns: 1fr;
  }

  .cta-install {
    flex-direction: column;
    width: 100%;
    max-width: 400px;
  }

  .section {
    padding: var(--space-lg) var(--space-md);
  }

  .docs-index {
    padding: var(--space-lg) var(--space-md);
  }
}

/* --- Scrollbar (webkit) --- */
.sidebar::-webkit-scrollbar {
  width: 6px;
}

.sidebar::-webkit-scrollbar-track {
  background: transparent;
}

.sidebar::-webkit-scrollbar-thumb {
  background: var(--bg-lighter);
  border-radius: 3px;
}
```

- [ ] **Step 2: Commit**

```bash
git add website/static/css/style.css
git commit -m "feat(website): add warm gruvbox stylesheet"
```

---

### Task 5: Base Templates & Partials

**Files:**
- Create: `website/templates/_base.html`
- Create: `website/templates/_docs.html`
- Create: `website/templates/_partials/nav.html`
- Create: `website/templates/_partials/sidebar.html`
- Create: `website/templates/_partials/footer.html`

- [ ] **Step 1: Create nav partial**

Write `website/templates/_partials/nav.html`:

```html
<header class="nav">
  <div class="nav-inner">
    <a href="/" class="nav-logo">eigen</a>
    <button class="nav-toggle" onclick="toggleSidebar()" aria-label="Toggle navigation">
      <span></span>
      <span></span>
      <span></span>
    </button>
    <ul class="nav-links">
      {% for item in nav %}
      <li>
        <a href="{{ item.url }}"{% if item.external %} target="_blank" rel="noopener"{% endif %}>
          {{ item.label }}
        </a>
      </li>
      {% endfor %}
    </ul>
  </div>
</header>
```

- [ ] **Step 2: Create sidebar partial**

Write `website/templates/_partials/sidebar.html`:

This partial expects `docs_list` (all docs) and `doc` (current doc item) in the template context. It groups docs by category and highlights the current page.

```html
<aside class="sidebar" id="sidebar">
  {% set categories = ["Getting Started", "Templating", "Data", "Optimization", "SEO & Meta", "Frontend"] %}
  {% for cat in categories %}
  <details class="sidebar-category" open>
    <summary class="sidebar-category-title">{{ cat }}</summary>
    {% for d in docs_list %}
      {% if d.category == cat %}
      <a href="/docs/{{ d.slug }}"
         hx-get="/_fragments/docs/{{ d.slug }}/doc_content.html"
         hx-target="#doc-content"
         hx-push-url="/docs/{{ d.slug }}"
         class="sidebar-link{% if d.slug == doc.slug %} active{% endif %}"
         onclick="closeSidebar()">
        {{ d.title }}
      </a>
      {% endif %}
    {% endfor %}
  </details>
  {% endfor %}
</aside>
<div class="sidebar-overlay" id="sidebar-overlay" onclick="closeSidebar()"></div>
```

Categories use `<details open>` for native collapsibility — all start open, user can collapse any section. No JS needed.

**Note:** We use explicit HTMX attributes (`hx-get`, `hx-target`, `hx-push-url`) rather than `link_to()` because we target the `doc_content` fragment block which is a non-default block. If you find that `link_to("/docs/" ~ d.slug ~ ".html", "#doc-content", "doc_content")` works correctly, you may use that instead — but verify the fragment path resolves to `/_fragments/docs/<slug>/doc_content.html`.

- [ ] **Step 3: Create footer partial**

Write `website/templates/_partials/footer.html`:

```html
<footer class="footer">
  <p>eigen &mdash; built with eigen. <a href="https://github.com/wavefunk/eigen">source on GitHub</a>.</p>
</footer>
```

- [ ] **Step 4: Create base template**

Write `website/templates/_base.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{% block title %}{{ site.name }}{% endblock %}</title>
  <link rel="stylesheet" href="{{ asset('/css/style.css') }}">
  <script src="https://unpkg.com/htmx.org@2.0.4" defer></script>
</head>
<body>
  {% include "_partials/nav.html" %}
  <main>
    {% block content %}{% endblock %}
  </main>
  {% include "_partials/footer.html" %}
  <script>
  function toggleSidebar() {
    document.getElementById('sidebar').classList.toggle('open');
    document.getElementById('sidebar-overlay').classList.toggle('open');
  }
  function closeSidebar() {
    document.getElementById('sidebar').classList.remove('open');
    document.getElementById('sidebar-overlay').classList.remove('open');
  }
  document.addEventListener('DOMContentLoaded', function() {
    document.querySelectorAll('.doc-content pre').forEach(function(pre) {
      var btn = document.createElement('button');
      btn.className = 'copy-btn';
      btn.textContent = 'copy';
      btn.addEventListener('click', function() {
        var code = pre.querySelector('code');
        navigator.clipboard.writeText(code ? code.textContent : pre.textContent);
        btn.textContent = 'copied';
        setTimeout(function() { btn.textContent = 'copy'; }, 1500);
      });
      pre.appendChild(btn);
    });
  });
  </script>
</body>
</html>
```

**Note:** The sidebar toggle/close functions are defined globally even though the sidebar only exists on doc pages. On non-doc pages, the buttons that call these functions aren't visible, so the dead functions are harmless. This avoids needing a separate base layout for docs vs non-docs.

- [ ] **Step 5: Create docs base template**

Write `website/templates/_docs.html`:

```html
{% extends "_base.html" %}

{% block content %}
<div class="docs-layout">
  {% include "_partials/sidebar.html" %}
  <article class="doc-content" id="doc-content">
    {% block doc_content %}{% endblock %}
  </article>
</div>
{% endblock %}
```

- [ ] **Step 6: Commit**

```bash
git add website/templates/
git commit -m "feat(website): add base templates and partials"
```

---

### Task 6: Landing Page

**Files:**
- Create: `website/templates/index.html`

- [ ] **Step 1: Write the landing page template**

Write `website/templates/index.html`:

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
{% extends "_base.html" %}

{% block title %}eigen — a static site generator that thinks in fragments{% endblock %}

{% block content %}
<section class="hero">
  <h1 class="hero-title">eigen</h1>
  <p class="hero-tagline">A static site generator that thinks in fragments.</p>
  <div class="hero-actions">
    <a href="/docs/quickstart" class="btn btn-primary">Get Started</a>
    <a href="https://github.com/wavefunk/eigen" class="btn btn-secondary" target="_blank" rel="noopener">View on GitHub</a>
  </div>
  <div class="hero-code">
    <span class="code-label">site.toml</span>
    <pre><code>[site]
name = "my-site"
base_url = "https://example.com"

[build]
fragments = true
clean_links = true

[sources.api]
url = "https://api.example.com"</code></pre>
  </div>
</section>

<section class="section">
  <h2 class="section-title">Features</h2>
  <div class="features-grid">
    {% for feature in features %}
    <div class="feature-card">
      <h3>{{ feature.title }}</h3>
      <p>{{ feature.description }}</p>
    </div>
    {% endfor %}
  </div>
</section>

<section class="section">
  <h2 class="section-title">How It Works</h2>
  <div class="steps">
    <div class="step">
      <span class="step-number">01</span>
      <h3>Write templates</h3>
      <pre><code>&lt;h1&gt;{{ "{{" }} post.title {{ "}}" }}&lt;/h1&gt;
{{ "{{" }} post.body | markdown {{ "}}" }}</code></pre>
    </div>
    <div class="step">
      <span class="step-number">02</span>
      <h3>Configure data</h3>
      <pre><code>data:
  posts:
    source: blog_api
    path: /posts
    limit: 10</code></pre>
    </div>
    <div class="step">
      <span class="step-number">03</span>
      <h3>Build &amp; deploy</h3>
      <pre><code>$ eigen build
  Rendered 47 pages
  Extracted 47 fragments
  Built in 0.8s</code></pre>
    </div>
  </div>
</section>

<section class="section cta">
  <div class="cta-install">
    <code>cargo install eigen</code>
    <button class="cta-copy" onclick="navigator.clipboard.writeText('cargo install eigen')" aria-label="Copy install command" title="Copy to clipboard">&#x2398;</button>
  </div>
  <p><a href="/docs/quickstart">Read the quickstart guide &rarr;</a></p>
</section>
{% endblock %}
```

**Note on Jinja escaping:** The "How It Works" section displays raw Jinja syntax as code examples. To prevent eigen from interpreting `{{ post.title }}` as a variable, we use `{{ "{{" }}` and `{{ "}}" }}` to emit literal curly braces. Verify this works in minijinja — if not, use `{% raw %}...{% endraw %}` blocks instead.

- [ ] **Step 2: Commit**

```bash
git add website/templates/index.html
git commit -m "feat(website): add landing page"
```

---

### Task 7: Docs Pages

**Files:**
- Create: `website/templates/docs/index.html`
- Create: `website/templates/docs/[doc].html`

- [ ] **Step 1: Write the docs index page**

Write `website/templates/docs/index.html`:

```html
---
data:
  nav:
    file: "nav.yaml"
  docs_list:
    file: "docs.yaml"
seo:
  title: "Documentation — eigen"
  description: "Comprehensive documentation for eigen — templates, data fetching, optimization, SEO, and more."
schema:
  type: WebSite
  breadcrumb_names:
    docs: "Documentation"
---
{% extends "_base.html" %}

{% block title %}Documentation — {{ site.name }}{% endblock %}

{% block content %}
<div class="docs-index">
  <h1>Documentation</h1>
  <div class="docs-index-intro">
    <p>Everything you need to build fast, fragment-driven static sites with eigen. Start with the quickstart guide or jump into a specific topic.</p>
  </div>

  {% set categories = [
    {"name": "Getting Started", "desc": "Install eigen and build your first site."},
    {"name": "Templating", "desc": "Draft pages, clean URLs, and template features."},
    {"name": "Data", "desc": "Fetch data from files, APIs, and remote sources."},
    {"name": "Optimization", "desc": "Build performance, asset optimization, and caching."},
    {"name": "SEO & Meta", "desc": "Search engine optimization, meta tags, and structured data."},
    {"name": "Frontend", "desc": "Analytics, transitions, and browser-side features."}
  ] %}

  <div class="category-cards">
    {% for cat in categories %}
    <div class="category-card">
      <h3>{{ cat.name }}</h3>
      <p class="category-card-desc">{{ cat.desc }}</p>
      <ul>
        {% for d in docs_list %}
          {% if d.category == cat.name %}
          <li><a href="/docs/{{ d.slug }}">{{ d.title }}</a></li>
          {% endif %}
        {% endfor %}
      </ul>
    </div>
    {% endfor %}
  </div>
</div>
{% endblock %}
```

- [ ] **Step 2: Write the dynamic doc page template**

Write `website/templates/docs/[doc].html`:

```html
---
collection:
  file: "docs.yaml"
slug_field: slug
item_as: doc
fragment_blocks:
  - doc_content
data:
  nav:
    file: "nav.yaml"
  docs_list:
    file: "docs.yaml"
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

{% block doc_content %}
<h1>{{ doc.title }}</h1>
{{ doc.content | markdown }}
{% endblock %}
```

**Key details:**
- `collection: { file: "docs.yaml" }` — generates one page per entry in docs.yaml
- `slug_field: slug` — uses the `slug` field for the URL: `/docs/<slug>.html`
- `item_as: doc` — each item is accessible as `doc` in the template
- `fragment_blocks: [doc_content]` — extracts the `doc_content` block as a fragment at `/_fragments/docs/<slug>/doc_content.html`
- `docs_list` is loaded separately for the sidebar (same file, both collection and data query)
- `nav` is loaded for the top navigation partial
- SEO fields use `{{ doc.title }}` and `{{ doc.description }}` interpolation

- [ ] **Step 3: Commit**

```bash
git add website/templates/docs/
git commit -m "feat(website): add docs index and dynamic doc page template"
```

---

### Task 8: Quickstart Tutorial

**Files:**
- Modify: `website/_data/docs.yaml` (prepend quickstart entry)

- [ ] **Step 1: Write quickstart content and add to docs.yaml**

Add a new entry at the **very beginning** of `website/_data/docs.yaml` (it must be the first entry so it appears first in the Getting Started category).

The entry to prepend:

```yaml
- title: "Quickstart"
  slug: quickstart
  category: Getting Started
  description: "Install eigen and build your first site in five minutes."
  content: |
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
```

- [ ] **Step 2: Validate YAML still parses**

```bash
python3 -c "import yaml; data = yaml.safe_load(open('website/_data/docs.yaml')); print(f'{len(data)} docs loaded'); print('First:', data[0]['title'])"
```

Expected: `23 docs loaded` and `First: Quickstart`

- [ ] **Step 3: Commit**

```bash
git add website/_data/docs.yaml
git commit -m "feat(website): add quickstart tutorial to docs"
```

---

### Task 9: 404 Page

**Files:**
- Create: `website/templates/404.html`

- [ ] **Step 1: Write the 404 template**

Write `website/templates/404.html`:

```html
---
data:
  nav:
    file: "nav.yaml"
---
{% extends "_base.html" %}

{% block title %}404 — {{ site.name }}{% endblock %}

{% block content %}
<section class="not-found">
  <h1>404</h1>
  <p>This page doesn't exist.</p>
  <a href="/" class="btn btn-primary">Back to home</a>
</section>
{% endblock %}
```

- [ ] **Step 2: Commit**

```bash
git add website/templates/404.html
git commit -m "feat(website): add 404 page"
```

---

### Task 10: Build & Verify

**Files:** None (verification only)

- [ ] **Step 1: Run eigen build**

```bash
cd /home/nambiar/projects/wavefunk/eigen
cargo run -- build -p website -v
```

Expected: Build completes without errors. Output shows:
- Pages rendered: `index.html`, `docs/index.html`, `404.html`, plus one page per doc entry (~23 doc pages)
- Fragments extracted for doc pages at `/_fragments/docs/<slug>/doc_content.html`
- Assets in `website/dist/`

- [ ] **Step 2: Check output structure**

```bash
ls website/dist/
ls website/dist/docs/ | head -20
ls website/dist/_fragments/docs/ | head -10
```

Expected:
- `website/dist/index.html` exists
- `website/dist/docs/index.html` exists
- `website/dist/docs/quickstart.html` exists (and other doc pages)
- `website/dist/_fragments/docs/quickstart/doc_content.html` exists
- `website/dist/css/` contains the bundled CSS
- `website/dist/fonts/` contains the woff2 files
- `website/dist/sitemap.xml` exists
- `website/dist/robots.txt` exists

- [ ] **Step 3: Fix any build errors**

If the build fails, read the error output and fix the issue. Common problems:
- YAML parse errors in `docs.yaml` — check indentation of `content: |` blocks
- Template syntax errors — check Jinja syntax, especially escaped braces in code examples
- Missing data variables — ensure every template's frontmatter loads all data it references (`nav`, `docs_list`, etc.)
- Fragment block misconfiguration — if `fragment_blocks: [doc_content]` isn't recognized, try without the brackets: `fragment_blocks: doc_content`

- [ ] **Step 4: Spot-check HTML output**

```bash
head -50 website/dist/index.html
head -30 website/dist/docs/quickstart.html
```

Verify:
- HTML structure is valid
- CSS link points to a content-hashed path (e.g., `/css/style.a1b2c3d4.css`)
- Font paths are correct
- Navigation links are present
- Doc content is rendered as HTML (not raw markdown)

- [ ] **Step 5: Commit any fixes**

If any fixes were needed in Steps 3-4:

```bash
git add -A website/
git commit -m "fix(website): fix build issues from initial verification"
```
