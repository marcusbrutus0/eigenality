# Dev Audit: SEO / PageSpeed / Accessibility Checker

## Overview

A built-in audit system for eigen that automatically checks rendered pages and site configuration for SEO, performance, accessibility, and best-practice issues during development. Results are surfaced via a per-page overlay badge, a full site report page, and machine-readable endpoints for AI-driven fixes.

## Goals

- Help developers catch SEO/performance/accessibility gaps **before deployment**
- Provide AI-friendly output so an AI agent can read the audit and apply fixes automatically
- Zero config to start — runs automatically in dev mode with sensible defaults
- Non-intrusive — overlay is minimal, audit never affects production builds

## Non-Goals

- Not a replacement for Lighthouse or full accessibility auditors — this is a fast, built-in first pass
- No runtime/browser-based checks (e.g., actual paint timing, real contrast measurement)
- No auto-fixing — the audit **reports** issues with actionable fix instructions, but doesn't modify files

---

## Architecture

### Approach: Build-Time Audit (Approach A)

The audit runs as the **final build phase** after all rendering, sitemap generation, robots.txt, feeds, bundling, and content hashing are complete. It inspects the fully-built `dist/` directory and the loaded `SiteConfig`.

```
... existing build phases ...
Phase 10: Content hash rewrite
Phase 11: Audit (dev mode + `eigen audit` CLI)
  1. Run site-level checks (config inspection)
  2. Walk dist/**/*.html, run page-level checks on each
  3. Filter out ignored check IDs from [audit] config
  4. Collect all findings into AuditReport
  5. Write _audit.html, _audit.json, _audit.md to dist/
  6. Inject per-page overlay badge into each HTML file (dev mode only)
```

### CLI Integration

Two entry points invoke the same audit module:

1. **`eigen dev`** — audit runs automatically after each rebuild. Serves `/_audit`, `/_audit.json`, `/_audit.md` routes. Injects overlay badge into every page.
2. **`eigen audit`** — standalone CLI command for CI/pre-deploy/AI workflows:
   - `eigen audit --project .` — prints markdown report to stdout
   - `eigen audit --project . --format json` — prints JSON to stdout
   - `eigen audit --project . --output audit-report` — writes `audit-report.json` + `audit-report.md` to disk

The audit module is shared between both entry points. The `eigen audit` command performs a full build first (or reads existing `dist/` if already built), then runs the audit phase.

---

## Module Structure

```
src/build/audit/
  mod.rs          — public API: run_audit(), AuditReport, types
  checks/
    mod.rs        — check registry, CheckFn trait
    seo.rs        — SEO checks
    performance.rs — performance checks
    accessibility.rs — accessibility checks
    best_practices.rs — best practices checks
  output/
    mod.rs        — output format dispatch
    html.rs       — _audit.html renderer
    json.rs       — _audit.json renderer
    markdown.rs   — _audit.md renderer
  overlay.rs      — per-page overlay badge HTML/CSS/JS generation
```

---

## Core Types

```rust
pub struct AuditReport {
    pub site_findings: Vec<Finding>,
    pub page_findings: BTreeMap<String, Vec<Finding>>,  // page path -> findings
    pub summary: AuditSummary,
}

pub struct Finding {
    pub id: &'static str,        // "seo/meta-description"
    pub category: Category,
    pub severity: Severity,
    pub scope: Scope,
    pub message: String,         // human-readable issue description
    pub fix: Fix,                // actionable fix info
}

pub enum Category {
    Seo,
    Performance,
    Accessibility,
    BestPractices,
}

pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

pub enum Scope {
    Site,
    Page,
}

pub struct Fix {
    pub file: String,            // "site.toml" or "templates/index.html"
    pub instruction: String,     // "Add description = \"...\" under [site.seo]"
}

pub struct AuditSummary {
    pub total: usize,
    pub by_severity: BTreeMap<Severity, usize>,
    pub by_category: BTreeMap<Category, usize>,
}
```

---

## Check Registry

Checks are implemented as functions registered in a central registry. Each check function receives context and returns `Vec<Finding>`.

### Site-Level Check Signature

```rust
fn check(config: &SiteConfig, dist_path: &Path) -> Vec<Finding>
```

Receives the full site config and path to `dist/`. Used for config-level checks and checks that scan across all pages (e.g., "no sitemap exists").

### Page-Level Check Signature

```rust
fn check(html: &str, page_path: &str, config: &SiteConfig) -> Vec<Finding>
```

Receives the rendered HTML content, the page's output path (e.g., `about.html`), and config for context. Called once per HTML file in `dist/`.

### Adding New Checks

Add a function to the appropriate category module, register it in `checks/mod.rs`. No other changes needed.

---

## Initial Check List

### SEO (`seo/`)

| ID | Severity | Scope | Description |
|----|----------|-------|-------------|
| `seo/meta-description` | high | page | Missing `<meta name="description">` |
| `seo/meta-title` | high | page | Missing `<title>` tag |
| `seo/og-tags` | medium | page | Missing Open Graph tags (title, description, image) |
| `seo/twitter-tags` | low | page | Missing Twitter Card tags |
| `seo/canonical` | high | page | Missing `<link rel="canonical">` |
| `seo/structured-data` | medium | page | Missing JSON-LD structured data |
| `seo/sitemap` | critical | site | No sitemap.xml generated |
| `seo/robots-txt` | high | site | No robots.txt configured |
| `seo/heading-hierarchy` | medium | page | Missing `<h1>` or multiple `<h1>` tags |
| `seo/feed` | low | site | No Atom feed configured |

### Performance (`perf/`)

| ID | Severity | Scope | Description |
|----|----------|-------|-------------|
| `perf/image-optimization` | high | site | Image optimization disabled |
| `perf/image-formats` | medium | site | Modern formats (webp/avif) not configured |
| `perf/minification` | high | site | HTML minification disabled |
| `perf/critical-css` | medium | site | Critical CSS inlining not enabled |
| `perf/bundling` | medium | site | CSS/JS bundling not enabled |
| `perf/content-hash` | low | site | Content hashing (cache busting) not enabled |
| `perf/preload-hints` | medium | site | Resource hints disabled |
| `perf/large-image` | high | page | Image without responsive srcset |
| `perf/render-blocking-scripts` | high | page | `<script>` in `<head>` without `defer`/`async` |
| `perf/font-preload` | medium | page | Font files referenced but not preloaded |

### Accessibility (`a11y/`)

| ID | Severity | Scope | Description |
|----|----------|-------|-------------|
| `a11y/img-alt-text` | high | page | `<img>` missing `alt` attribute |
| `a11y/html-lang` | critical | page | `<html>` missing `lang` attribute |
| `a11y/viewport-meta` | critical | page | Missing `<meta name="viewport">` |
| `a11y/link-text` | medium | page | Links with empty or generic text |
| `a11y/color-contrast-hint` | low | site | Reminder to verify color contrast |

### Best Practices (`bp/`)

| ID | Severity | Scope | Description |
|----|----------|-------|-------------|
| `bp/https-links` | medium | page | Page contains `http://` links |
| `bp/favicon` | medium | site | No favicon detected |
| `bp/mobile-viewport` | high | page | Viewport meta not configured for mobile |
| `bp/base-url` | critical | site | `site.base_url` not set |

---

## Configuration

### Ignoring Checks

Users can suppress specific check IDs in `site.toml`:

```toml
[audit]
ignore = ["seo/twitter-tags", "a11y/color-contrast-hint"]
```

The `[audit]` table is optional. When absent, all checks run. The `ignore` list uses the stable check IDs.

---

## Output Formats

### `/_audit.html` (Human-Readable Report Page)

A self-contained HTML page (no external dependencies) showing:

- Summary bar: total issues by severity (critical: N, high: N, etc.)
- Grouped by category (SEO, Performance, Accessibility, Best Practices)
- Within each category, sorted by severity (critical first)
- Each finding shows: ID, severity badge, message, fix instruction
- Page-level findings grouped by page path
- Filterable by category and severity via simple JS toggles

Styled with inline CSS, scoped to avoid conflicts. Dark/light theme based on `prefers-color-scheme`.

### `/_audit.json` (AI/Programmatic)

```json
{
  "summary": {
    "total": 12,
    "by_severity": { "critical": 1, "high": 4, "medium": 5, "low": 2 },
    "by_category": { "seo": 5, "performance": 3, "accessibility": 2, "best_practices": 2 }
  },
  "site_findings": [
    {
      "id": "seo/sitemap",
      "category": "seo",
      "severity": "critical",
      "message": "No sitemap.xml generated. Search engines use sitemaps to discover pages.",
      "fix": {
        "file": "site.toml",
        "instruction": "Sitemap is generated automatically when pages are rendered. Verify the build completes successfully."
      }
    }
  ],
  "page_findings": {
    "index.html": [
      {
        "id": "seo/meta-description",
        "category": "seo",
        "severity": "high",
        "message": "Missing <meta name=\"description\"> tag.",
        "fix": {
          "file": "site.toml",
          "instruction": "Add description under [site.seo]: description = \"Your site description\""
        }
      }
    ]
  }
}
```

### `/_audit.md` (Markdown for AI Chat / Human Reading)

```markdown
# Eigen Audit Report

## Summary
- **Critical:** 1
- **High:** 4
- **Medium:** 5
- **Low:** 2

## Site-Wide Issues

### [CRITICAL] seo/sitemap
No sitemap.xml generated. Search engines use sitemaps to discover pages.
**Fix:** In `site.toml`, verify the build completes successfully. Sitemap is generated automatically.

## Page Issues

### index.html

#### [HIGH] seo/meta-description
Missing <meta name="description"> tag.
**Fix:** In `site.toml`, add `description = "Your site description"` under `[site.seo]`.
```

---

## Dev Overlay Badge

### Injection

Injected via the same mechanism as the live-reload script (`src/dev/inject.rs`), appended before `</body>` in dev builds only. The page-specific findings are embedded as a JSON blob in the injected `<script>` tag — no async fetch needed since they're known at build time.

### Behavior

- **Badge:** Small floating element in the bottom-right corner
  - Shows issue count and color-coded by worst severity (red = critical, orange = high, yellow = medium, green = all clear)
  - Example: `"4 issues"` with orange background
- **Expanded panel:** Clicking the badge opens a panel showing page-specific findings for the current page
  - Grouped by category, sorted by severity
  - Each finding shows severity badge, message, fix instruction
  - Link to `/_audit` for the full site report
- **State:** Collapsed/expanded state remembered via `localStorage`
- **Scoping:** All CSS uses a unique prefix (`__eigen_audit_`) to avoid conflicts with site styles
- **Not in production:** The overlay is never injected during `eigen build`

### Styling

Minimal, non-intrusive. Uses `position: fixed`, high `z-index`, `prefers-color-scheme` for dark/light. Shadow DOM or prefixed classes to isolate from site CSS.

---

## Build Pipeline Integration

### In `eigen dev` (dev mode)

After the existing final build phase:
1. `audit::run_audit(config, dist_path)` → `AuditReport`
2. `audit::output::write_html(report, dist_path)` → `dist/_audit.html`
3. `audit::output::write_json(report, dist_path)` → `dist/_audit.json`
4. `audit::output::write_markdown(report, dist_path)` → `dist/_audit.md`
5. `audit::overlay::inject_badges(report, dist_path)` → modifies each HTML file in `dist/`

The overlay injection happens **after** the audit report files are written but uses the same `AuditReport` data. It must run after minification (since it modifies HTML) — this is already the case since audit is the last phase.

### In `eigen audit` (CLI command)

1. Build the site (full `eigen build` pipeline) OR read existing `dist/` if `--no-build` flag
2. `audit::run_audit(config, dist_path)` → `AuditReport`
3. Output based on flags:
   - Default: print markdown to stdout
   - `--format json`: print JSON to stdout
   - `--output <path>`: write `<path>.json` + `<path>.md` to disk

### In `eigen build` (production)

Audit does **not** run. No `_audit.*` files are written. No overlay is injected.

---

## HTML Parsing for Page-Level Checks

Page-level checks need to inspect rendered HTML. Rather than pulling in a full DOM parser, use `lol_html` (already a dependency) for streaming HTML inspection where possible. For checks that need to look at multiple elements in context (e.g., heading hierarchy), use simple regex or string matching on the already-rendered HTML — these pages are small and the checks are simple.

Checks that scan HTML:
- Tag presence: regex for `<meta name="description"`, `<title>`, `<link rel="canonical"`, etc.
- Attribute presence: regex for `<img` without `alt=`, `<html` without `lang=`
- Script position: regex for `<head>.*<script` without `defer`/`async`
- Link text: regex for `<a[^>]*>(\s*|click here|here|read more)</a>`
- Image srcset: regex for `<img` without `srcset=`

This avoids adding new dependencies and keeps the audit phase fast.

---

## Future Extensions

- **Custom checks via plugins** — allow the plugin system to register custom audit checks
- **Severity overrides** — let users change severity of specific checks in `[audit]` config
- **Baseline file** — `eigen audit --baseline` to snapshot current issues, then only report new ones
- **Watch mode for `eigen audit`** — re-run on file changes (useful for CI-like local workflows)

These are explicitly **not in scope** for the initial implementation.
