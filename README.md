<p align="center">
  <img src="assets/logo.svg" width="80" alt="eigen">
</p>

<h1 align="center">eigen</h1>

<p align="center">
  A static site generator that thinks in fragments.
</p>

<p align="center">
  <a href="https://github.com/wavefunk/eigen/actions/workflows/test.yml"><img src="https://github.com/wavefunk/eigen/actions/workflows/test.yml/badge.svg" alt="Tests"></a>
  <a href="https://github.com/wavefunk/eigen/actions/workflows/website.yml"><img src="https://github.com/wavefunk/eigen/actions/workflows/website.yml/badge.svg" alt="Website"></a>
  <img src="https://img.shields.io/badge/version-0.13.1-e78a4e" alt="Version 0.13.1">
</p>

---

Eigen is a fast, opinionated static site generator with first-class HTMX support. It renders every page as both a full HTML document and a standalone HTML fragment, enabling partial page loads with zero client-side framework code.

Built in Rust with [minijinja](https://github.com/mitsuhiko/minijinja) templates, YAML/JSON data, and [Axum](https://github.com/tokio-rs/axum) for the dev server.

## Features

- **HTMX Fragments** — Every template block becomes a standalone fragment. Click a link, swap a div — no full page reload. Navigation that feels like an SPA, built with zero JavaScript frameworks.

- **Data From Anywhere** — Pull data from local YAML files, REST APIs, Notion, or GraphQL. One config block per source. Caching, rate limiting, and auth headers built in.

- **Dynamic Pages** — Name a template `[slug].html` and point it at a collection. Eigen generates one page per item — blog posts, product pages, docs — from a single template.

- **Built-in Optimization** — Critical CSS inlining, image lazy loading, CSS/JS bundling with tree-shaking, content-hashed assets, HTML minification. All on by default.

- **Incremental Builds** — Only re-renders pages whose templates, data, or config actually changed. Two-tier hashing keeps rebuilds fast even on large sites.

- **Single Config** — One `site.toml` file configures everything — data sources, SEO defaults, build optimization, analytics, redirects, feeds. No plugin ecosystem to navigate.

- **SEO Out of the Box** — Auto-generated sitemap, robots.txt, Open Graph tags, Twitter cards, JSON-LD structured data. Configure once at the site level, override per page.

- **Rust Performance** — Async build pipeline on Tokio. Concurrent page rendering, parallel data fetching, non-blocking I/O. Builds your site as fast as your machine allows.

## Quick Start

```bash
# Create a new project
eigen init my-site

# Build the site
cd my-site
eigen build

# Or start the dev server with live reload
eigen dev
```

## Installation

### cargo install

```bash
cargo install eigen
```

### GitHub Action

```yaml
steps:
  - uses: actions/checkout@v4
  - uses: wavefunk/setup-eigen@v1
```

See the [GitHub Action docs](https://wavefunk.github.io/eigen/docs/github-action) for inputs and options.

### Nix

A dev shell is provided via `flake.nix`:

```bash
nix develop
```

### Build from source

```bash
git clone https://github.com/wavefunk/eigen.git
cd eigen
cargo build --release
# Binary at target/release/eigen
```

## Project Structure

```
my-site/
├── site.toml              # Site configuration
├── templates/             # Jinja2 templates (minijinja)
│   ├── _base.html         # Layout (underscore = not a page)
│   ├── _partials/         # Reusable partials
│   ├── index.html         # Static page → dist/index.html
│   └── posts/
│       └── [post].html    # Dynamic page → dist/posts/{slug}.html
├── _data/                 # Global data (YAML/JSON)
├── static/                # Static assets (copied to dist/)
└── dist/                  # Build output
```

| Convention | Meaning |
|---|---|
| `_` prefix in `templates/` | Layout or partial — not rendered as a page |
| `[name].html` filename | Dynamic template — one page per collection item |
| `_data/` directory | Global data available to all templates |
| `static/` directory | Copied verbatim to `dist/` |

## Documentation

Full documentation is available at **[wavefunk.github.io/eigen](https://wavefunk.github.io/eigen/docs/)**.

| Category | Topics |
|---|---|
| **Getting Started** | Installation, quickstart, project structure, CLI commands |
| **Templating** | Layouts, partials, dynamic pages, draft pages, clean URLs, view transitions |
| **Data** | Data sources, queries, transforms, POST method, ETag caching, rate limiting |
| **Optimization** | Async builds, incremental builds, critical CSS, bundling, content hashing, lazy loading |
| **SEO & Meta** | Open Graph, Twitter cards, JSON-LD, sitemap, robots.txt, canonical URLs, Atom feeds, redirects |
| **Frontend** | Analytics (Google Analytics, Umami), view transitions, resource hints |

## License

See [LICENSE](LICENSE) for details.
