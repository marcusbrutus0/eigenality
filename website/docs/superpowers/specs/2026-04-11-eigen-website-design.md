# Eigen Website Redesign — Design Spec

## Goal

Replace the current single-file `website/index.html` with a site built using eigen itself. The site serves two audiences equally: developers evaluating eigen, and developers already using it. All data is static (YAML/JSON) — no CMS integration.

## Pages

```
/                       Landing page
/docs/                  Docs index (category cards)
/docs/quickstart/       Getting started tutorial (new content)
/docs/<feature>/        Individual doc pages (from docs/*.md)
```

## Design Direction

**"Warm Mechanical" + programmer aesthetic.** Gruvbox-warm palette, Recursive font family. The site should feel like a well-made terminal theme became a website — warm, readable, distinctly engineer-crafted. Dark only.

## Color Palette

Gruvbox-warm derived:

| Role           | Hex       | Usage                                    |
|----------------|-----------|------------------------------------------|
| `bg-dark`      | `#1d2021` | Page background                          |
| `bg`           | `#282828` | Card/sidebar backgrounds                 |
| `bg-light`     | `#3c3836` | Hover states, active sidebar items       |
| `bg-lighter`   | `#504945` | Borders, subtle dividers                 |
| `fg`           | `#ebdbb2` | Primary text                             |
| `fg-dim`       | `#a89984` | Secondary text, labels                   |
| `orange`       | `#e78a4e` | Primary accent — links, CTAs, active     |
| `yellow`       | `#d8a657` | Secondary accent — highlights, tags      |
| `red`          | `#ea6962` | Warnings, important callouts             |
| `aqua`         | `#89b482` | Code strings, success states             |
| `blue`         | `#7daea3` | Code keywords, info callouts             |
| `purple`       | `#d3869b` | Occasional variety — used sparingly      |

## Typography

**Font family:** Recursive — self-hosted as two static woff2 instances (~30KB total).

| Role         | Instance                            | Usage                              |
|--------------|-------------------------------------|------------------------------------|
| Headings     | Recursive, casual bold (~15KB)      | `CASL 0.5, wght 800`              |
| Body         | Recursive, linear regular (~15KB)   | `CASL 0, wght 400`                |
| Code         | Same linear instance, monospace     | `MONO 1, CASL 0, wght 400`        |
| Labels/tags  | Same linear instance, mono medium   | `MONO 1, CASL 0, wght 500`        |

Fallbacks: `system-ui, sans-serif` for proportional, `ui-monospace, monospace` for code.

Scale: fluid `clamp()` for responsive sizing between mobile and desktop.

## Landing Page

**Top to bottom:**

1. **Top bar:** `eigen` wordmark (left), nav links: Docs, GitHub (right). Sticks on scroll.

2. **Hero:** Large `eigen` wordmark + one-line tagline (e.g. *"A static site generator that thinks in fragments"*). Two CTAs: `Get Started` (→ /docs/quickstart/), `View on GitHub`. Below: a static terminal-style code block showing minimal `site.toml` → rendered output.

3. **Features grid:** 6-8 cards. Title + 1-2 sentence description each. No icons. Subtle warm background variation for depth. Features: HTMX fragments, data fetching, dynamic pages, asset optimization, incremental builds, single config, built-in SEO, Rust performance.

4. **How it works:** 3-step horizontal flow: `Write templates` → `Configure data` → `Build & deploy`. Each step has a small real code snippet.

5. **Closing CTA:** `cargo install eigen` (copy-ready), link to quickstart.

No stats counters, no benefits bar, no marketing fluff.

## Docs Section

### Layout

- **Left sidebar** (~250px): Categorized doc links, collapsible sections (all start open), current page highlighted, scrolls independently.
- **Content area:** Rendered doc content. Clean heading hierarchy, generous line spacing.
- No right-side TOC initially.

### Docs Index (`/docs/`)

Brief intro paragraph + category cards. Each card: category name, one-liner description, links to docs within.

### Quickstart (`/docs/quickstart/`)

Written fresh as a walk-through tutorial:
1. Install eigen
2. Scaffold a site
3. Write a template with data
4. Build
5. Add HTMX fragments
6. "Next steps" links into reference docs

### Doc Categories

| Category            | Docs                                                                          |
|---------------------|-------------------------------------------------------------------------------|
| **Getting Started** | Quickstart, GitHub Action                                                     |
| **Templating**      | Draft Pages, Clean Links                                                      |
| **Data**            | Data Cache, POST Method, Rate Limiting, Source Assets                         |
| **Optimization**    | Async Build, Incremental Builds, Content Hashing, Critical CSS, CSS/JS Bundling, Lazy Loading, Preload/Prefetch |
| **SEO & Meta**      | Audit, JSON-LD, OG/Twitter Meta, Robots.txt, RSS Feed, Redirects             |
| **Frontend**        | Analytics, View Transitions                                                   |

### Code Blocks

- Styled to match the warm palette — syntax highlighting uses the accent colors (orange, yellow, aqua, blue, etc.)
- Language labels shown subtly
- Copy button on hover

### HTMX Navigation

Sidebar link clicks swap the content block via eigen fragments. Sidebar stays put. URL updates, back button works. View transitions for smooth swap animation.

## Responsive Design

Three breakpoints:

| Breakpoint        | Layout                                                      |
|-------------------|-------------------------------------------------------------|
| Desktop (>1024px) | Full sidebar + content                                      |
| Tablet (768-1024) | Sidebar collapses to hamburger, content full-width          |
| Mobile (<768px)   | Single column, hamburger nav, stacked cards, smaller hero   |

Mobile specifics:
- Sidebar → slide-out drawer, closes on link click
- Feature grid: 3-col → 2-col → 1-col
- Code blocks: horizontal scroll, no wrapping
- Touch-friendly tap targets
- Hamburger/drawer via small inline script (no framework)

## Eigen Features to Dogfood

The site should use these eigen features to serve as a living demo:

- **Fragments & HTMX** — doc navigation swaps content block
- **View transitions** — smooth page swaps
- **Content hashing** — all static assets fingerprinted
- **Clean links** — `/docs/quickstart` not `/docs/quickstart.html`
- **Sitemap** — auto-generated
- **Robots.txt** — auto-generated
- **SEO meta** — OG/Twitter tags on all pages
- **JSON-LD** — WebSite schema on landing, BreadcrumbList on docs
- **Critical CSS** — inline above-fold styles
- **Lazy loading** — images below fold
- **Preload/prefetch** — prefetch doc fragments for fast navigation
- **Minification** — HTML/CSS/JS minified in production
- **CSS/JS bundling** — bundled assets

## Data Architecture

All static, stored in `_data/`:

- `docs.yaml` — ordered list of docs with metadata (title, slug, category, description) driving the sidebar and docs index
- `features.yaml` — feature cards for the landing page
- `nav.yaml` — top navigation links

Doc content comes from the `docs/*.md` files, rendered via the `markdown` filter in templates.

## Template Structure

```
website/
  site.toml
  templates/
    _base.html              Base layout (head, top bar, scripts)
    _partials/
      nav.html              Top navigation
      sidebar.html          Docs sidebar
      footer.html           Footer
    index.html              Landing page
    docs/
      index.html            Docs index (category cards)
      [doc].html            Dynamic template — one page per doc entry
  _data/
    docs.yaml               Doc metadata + ordering
    features.yaml           Landing page features
    nav.yaml                Navigation links
  static/
    css/
      style.css             Main stylesheet
    fonts/
      recursive-heading.woff2
      recursive-body.woff2
    images/                 (if needed)
```

The `[doc].html` template is a dynamic page driven by `docs.yaml`. Each entry in `docs.yaml` includes a `slug` and a `content_file` field pointing to the corresponding `docs/*.md` file. The template loads the markdown content via the `file` data source in frontmatter (using `{{ item.content_file }}` interpolation), then renders it through the `markdown` filter.

## Out of Scope

- Blog / announcements
- Examples / showcase
- Light theme
- CMS / remote data
- Search (can add later)
- Right-side table of contents
