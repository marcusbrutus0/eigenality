# Analytics Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat `[analytics]` config with a provider-based `[analytics.google]` / `[analytics.umami]` structure and add Umami tracking support.

**Architecture:** The existing `AnalyticsConfig` struct becomes a container with optional sub-configs per provider. Each provider has its own snippet builder. The injection function collects all enabled snippets and injects them once before `</body>`. Breaking change — old `[analytics] tracking_id` format is removed.

**Tech Stack:** Rust, serde/toml for config parsing, existing `inject_analytics` pattern in `src/build/analytics.rs`.

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/config/mod.rs` | Modify | Replace `AnalyticsConfig`, add `GoogleAnalyticsConfig`, `UmamiAnalyticsConfig`, `default_umami_host`, `validate_analytics_config` |
| `src/build/analytics.rs` | Modify | Rename `build_snippet` → `build_google_snippet`, add `build_umami_snippet`, update `inject_analytics` signature |
| `src/build/render.rs` | Modify | Update `inject_analytics` call site |
| `docs/analytics.md` | Create | Feature documentation |

---

### Task 1: Update Config Structs

**Files:**
- Modify: `src/config/mod.rs:291-302` (replace `AnalyticsConfig`)

- [ ] **Step 1: Write failing config parsing tests**

Add these tests at the end of the `mod tests` block in `src/config/mod.rs`:

```rust
#[test]
fn test_parse_analytics_google_only() {
    let toml_str = r#"
[site]
name = "My Site"
base_url = "https://example.com"

[analytics.google]
tracking_id = "G-TEST123"
"#;
    let config = parse_toml(toml_str).unwrap();
    let analytics = config.analytics.unwrap();
    let google = analytics.google.unwrap();
    assert_eq!(google.tracking_id, "G-TEST123");
    assert!(analytics.umami.is_none());
}

#[test]
fn test_parse_analytics_umami_only_defaults() {
    let toml_str = r#"
[site]
name = "My Site"
base_url = "https://example.com"

[analytics.umami]
website_id = "abc-123"
"#;
    let config = parse_toml(toml_str).unwrap();
    let analytics = config.analytics.unwrap();
    assert!(analytics.google.is_none());
    let umami = analytics.umami.unwrap();
    assert_eq!(umami.website_id, "abc-123");
    assert_eq!(umami.host_url, "https://cloud.umami.is");
    assert!(umami.auto_track);
    assert!(umami.domains.is_none());
    assert!(umami.tag.is_none());
}

#[test]
fn test_parse_analytics_umami_full() {
    let toml_str = r#"
[site]
name = "My Site"
base_url = "https://example.com"

[analytics.umami]
website_id = "abc-123"
host_url = "https://analytics.example.com"
domains = "example.com,www.example.com"
auto_track = false
tag = "production"
"#;
    let config = parse_toml(toml_str).unwrap();
    let umami = config.analytics.unwrap().umami.unwrap();
    assert_eq!(umami.website_id, "abc-123");
    assert_eq!(umami.host_url, "https://analytics.example.com");
    assert_eq!(umami.domains.as_deref(), Some("example.com,www.example.com"));
    assert!(!umami.auto_track);
    assert_eq!(umami.tag.as_deref(), Some("production"));
}

#[test]
fn test_parse_analytics_both_providers() {
    let toml_str = r#"
[site]
name = "My Site"
base_url = "https://example.com"

[analytics.google]
tracking_id = "G-BOTH"

[analytics.umami]
website_id = "both-123"
"#;
    let config = parse_toml(toml_str).unwrap();
    let analytics = config.analytics.unwrap();
    assert!(analytics.google.is_some());
    assert!(analytics.umami.is_some());
}

#[test]
fn test_parse_no_analytics_section() {
    let toml_str = r#"
[site]
name = "My Site"
base_url = "https://example.com"
"#;
    let config = parse_toml(toml_str).unwrap();
    assert!(config.analytics.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_parse_analytics -- --nocapture 2>&1 | head -30`
Expected: compilation errors because `AnalyticsConfig` has no `google` or `umami` fields.

- [ ] **Step 3: Replace AnalyticsConfig and add new structs**

In `src/config/mod.rs`, replace lines 291–302:

```rust
/// Analytics provider configuration.
///
/// Located under `[analytics]` in site.toml. Each provider is an optional
/// sub-table. When present, the corresponding tracking snippet is injected
/// into every rendered full page before `</body>`.
///
/// Absent `[analytics]` section = analytics disabled.
#[derive(Debug, Clone, Deserialize)]
pub struct AnalyticsConfig {
    /// Google Analytics (gtag.js) configuration.
    pub google: Option<GoogleAnalyticsConfig>,
    /// Umami analytics configuration.
    pub umami: Option<UmamiAnalyticsConfig>,
}

/// Google Analytics (gtag.js) configuration.
///
/// Located under `[analytics.google]` in site.toml.
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleAnalyticsConfig {
    /// Google Analytics measurement ID, e.g. `"G-XXXXXXXXXX"`.
    pub tracking_id: String,
}

/// Umami analytics configuration.
///
/// Located under `[analytics.umami]` in site.toml.
#[derive(Debug, Clone, Deserialize)]
pub struct UmamiAnalyticsConfig {
    /// Umami website ID (UUID from the Umami dashboard).
    pub website_id: String,
    /// Base URL of the Umami instance. Default: `"https://cloud.umami.is"`.
    #[serde(default = "default_umami_host")]
    pub host_url: String,
    /// Comma-separated list of domains to restrict tracking to.
    pub domains: Option<String>,
    /// Whether Umami should automatically track page views. Default: true.
    #[serde(default = "default_true")]
    pub auto_track: bool,
    /// Custom event tag applied to all events from this site.
    pub tag: Option<String>,
}
```

Add the default function near the other default functions (after `default_content_block`):

```rust
fn default_umami_host() -> String {
    "https://cloud.umami.is".to_string()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests::test_parse_analytics -- --nocapture`
Expected: all 5 new tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat(config): replace flat AnalyticsConfig with provider sub-tables

BREAKING: [analytics] tracking_id is removed.
Use [analytics.google] tracking_id and/or [analytics.umami] website_id."
```

---

### Task 2: Add Config Validation

**Files:**
- Modify: `src/config/mod.rs` (add `validate_analytics_config`, wire into `validate_config`)

- [ ] **Step 1: Write failing validation tests**

Add these tests at the end of `mod tests` in `src/config/mod.rs`:

```rust
#[test]
fn test_validate_analytics_google_empty_tracking_id() {
    let toml_str = r#"
[site]
name = "My Site"
base_url = "https://example.com"

[analytics.google]
tracking_id = ""
"#;
    let err = load_config_from_str(toml_str).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("tracking_id"), "error should mention tracking_id: {msg}");
}

#[test]
fn test_validate_analytics_umami_empty_website_id() {
    let toml_str = r#"
[site]
name = "My Site"
base_url = "https://example.com"

[analytics.umami]
website_id = ""
"#;
    let err = load_config_from_str(toml_str).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("website_id"), "error should mention website_id: {msg}");
}
```

Check whether a `load_config_from_str` helper already exists; if not, add it to `mod tests`:

```rust
/// Parse + validate a TOML string as a SiteConfig.
fn load_config_from_str(input: &str) -> Result<SiteConfig> {
    let config: SiteConfig = toml::from_str(input)?;
    validate_config(&config)?;
    Ok(config)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::test_validate_analytics -- --nocapture 2>&1 | head -20`
Expected: tests pass (no validation yet), so the assertions fail — the `unwrap_err()` panics because no error is returned.

- [ ] **Step 3: Add validate_analytics_config**

Add this function in `src/config/mod.rs` near the other `validate_*` functions (before `validate_feed_configs`):

```rust
/// Validate analytics provider configurations.
fn validate_analytics_config(config: &SiteConfig) -> Result<()> {
    if let Some(ref analytics) = config.analytics {
        if let Some(ref google) = analytics.google {
            if google.tracking_id.is_empty() {
                bail!("[analytics.google] tracking_id must not be empty");
            }
        }
        if let Some(ref umami) = analytics.umami {
            if umami.website_id.is_empty() {
                bail!("[analytics.umami] website_id must not be empty");
            }
        }
    }
    Ok(())
}
```

Wire it into `validate_config` by adding `validate_analytics_config(config)?;` as the first call inside the function body (after the `base_url`/`name` checks, before `validate_feed_configs`):

```rust
fn validate_config(config: &SiteConfig) -> Result<()> {
    if config.site.base_url.is_empty() {
        bail!("site.base_url must not be empty in site.toml");
    }
    if config.site.name.is_empty() {
        bail!("site.name must not be empty in site.toml");
    }
    validate_analytics_config(config)?;
    validate_feed_configs(config)?;
    validate_robots_config(config)?;
    validate_audit_config(config)?;
    validate_security_headers_config(config)?;
    validate_redirect_rules(config)?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config::tests::test_validate_analytics -- --nocapture`
Expected: both validation tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs
git commit -m "feat(config): add analytics provider validation"
```

---

### Task 3: Update Snippet Builders

**Files:**
- Modify: `src/build/analytics.rs` (rename `build_snippet`, add `build_umami_snippet`, update `inject_analytics`)

- [ ] **Step 1: Write failing tests for umami snippet**

Replace the entire test module in `src/build/analytics.rs` with the updated tests. First, add `use crate::config::{...}` at the top of the file:

```rust
use crate::config::{AnalyticsConfig, GoogleAnalyticsConfig, UmamiAnalyticsConfig};
```

Then replace the `#[cfg(test)] mod tests` block with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AnalyticsConfig, GoogleAnalyticsConfig, UmamiAnalyticsConfig};

    fn google_config(tracking_id: &str) -> AnalyticsConfig {
        AnalyticsConfig {
            google: Some(GoogleAnalyticsConfig {
                tracking_id: tracking_id.to_string(),
            }),
            umami: None,
        }
    }

    fn umami_config_minimal(website_id: &str) -> AnalyticsConfig {
        AnalyticsConfig {
            google: None,
            umami: Some(UmamiAnalyticsConfig {
                website_id: website_id.to_string(),
                host_url: "https://cloud.umami.is".to_string(),
                domains: None,
                auto_track: true,
                tag: None,
            }),
        }
    }

    // --- build_google_snippet ---

    #[test]
    fn test_google_snippet_contains_tracking_id_twice() {
        let snippet = build_google_snippet("G-MYSITE99");
        let count = snippet.matches("G-MYSITE99").count();
        assert_eq!(count, 2, "tracking ID should appear in script src and gtag config");
    }

    #[test]
    fn test_google_snippet_structure() {
        let snippet = build_google_snippet("G-ABC123");
        assert!(snippet.contains(r#"src="https://www.googletagmanager.com/gtag/js?id=G-ABC123""#));
        assert!(snippet.contains("window.dataLayer = window.dataLayer || []"));
        assert!(snippet.contains("function gtag()"));
        assert!(snippet.contains("gtag('js', new Date())"));
        assert!(snippet.contains("gtag('config', 'G-ABC123')"));
    }

    // --- build_umami_snippet ---

    #[test]
    fn test_umami_snippet_minimal() {
        let config = UmamiAnalyticsConfig {
            website_id: "abc-123".to_string(),
            host_url: "https://cloud.umami.is".to_string(),
            domains: None,
            auto_track: true,
            tag: None,
        };
        let snippet = build_umami_snippet(&config);
        assert!(snippet.contains(r#"src="https://cloud.umami.is/script.js""#));
        assert!(snippet.contains(r#"data-website-id="abc-123""#));
        assert!(!snippet.contains("data-domains"));
        assert!(!snippet.contains("data-auto-track"));
        assert!(!snippet.contains("data-tag"));
    }

    #[test]
    fn test_umami_snippet_full() {
        let config = UmamiAnalyticsConfig {
            website_id: "abc-123".to_string(),
            host_url: "https://analytics.example.com".to_string(),
            domains: Some("example.com,www.example.com".to_string()),
            auto_track: false,
            tag: Some("production".to_string()),
        };
        let snippet = build_umami_snippet(&config);
        assert!(snippet.contains(r#"src="https://analytics.example.com/script.js""#));
        assert!(snippet.contains(r#"data-website-id="abc-123""#));
        assert!(snippet.contains(r#"data-domains="example.com,www.example.com""#));
        assert!(snippet.contains(r#"data-auto-track="false""#));
        assert!(snippet.contains(r#"data-tag="production""#));
    }

    #[test]
    fn test_umami_snippet_auto_track_true_omits_attribute() {
        let config = UmamiAnalyticsConfig {
            website_id: "abc-123".to_string(),
            host_url: "https://cloud.umami.is".to_string(),
            domains: None,
            auto_track: true,
            tag: None,
        };
        let snippet = build_umami_snippet(&config);
        assert!(!snippet.contains("data-auto-track"), "auto_track=true should not emit the attribute");
    }

    // --- inject_analytics ---

    #[test]
    fn test_inject_google_only() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = google_config("G-TEST123");
        let result = inject_analytics(html, &config);
        assert!(result.contains("googletagmanager"));
        assert!(!result.contains("umami"));
    }

    #[test]
    fn test_inject_umami_only() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = umami_config_minimal("abc-123");
        let result = inject_analytics(html, &config);
        assert!(result.contains("data-website-id"));
        assert!(!result.contains("googletagmanager"));
    }

    #[test]
    fn test_inject_both_providers() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = AnalyticsConfig {
            google: Some(GoogleAnalyticsConfig {
                tracking_id: "G-BOTH".to_string(),
            }),
            umami: Some(UmamiAnalyticsConfig {
                website_id: "both-123".to_string(),
                host_url: "https://cloud.umami.is".to_string(),
                domains: None,
                auto_track: true,
                tag: None,
            }),
        };
        let result = inject_analytics(html, &config);
        let google_pos = result.find("googletagmanager").unwrap();
        let umami_pos = result.find("data-website-id").unwrap();
        assert!(google_pos < umami_pos, "Google snippet should come before Umami");
        let body_pos = result.find("</body>").unwrap();
        assert!(umami_pos < body_pos, "both snippets should be before </body>");
    }

    #[test]
    fn test_inject_neither_provider() {
        let html = "<html><body><h1>Hi</h1></body></html>";
        let config = AnalyticsConfig {
            google: None,
            umami: None,
        };
        let result = inject_analytics(html, &config);
        assert_eq!(result, html, "no providers means no changes");
    }

    #[test]
    fn test_inject_case_insensitive_body_tag() {
        let html = "<html><body><p>test</p></BODY></html>";
        let config = umami_config_minimal("abc-123");
        let result = inject_analytics(html, &config);
        let snippet_pos = result.find("data-website-id").unwrap();
        let body_pos = result.find("</BODY>").unwrap();
        assert!(snippet_pos < body_pos);
    }

    #[test]
    fn test_inject_no_body_tag_appends() {
        let html = "<h1>Fragment</h1>";
        let config = google_config("G-FRAG");
        let result = inject_analytics(html, &config);
        assert!(result.contains("googletagmanager"));
        assert!(result.ends_with("</script>"));
    }

    #[test]
    fn test_inject_preserves_original_content() {
        let html = "<html><body><h1>Hello World</h1><p>Content</p></body></html>";
        let config = google_config("G-TEST");
        let result = inject_analytics(html, &config);
        assert!(result.contains("<h1>Hello World</h1>"));
        assert!(result.contains("<p>Content</p>"));
        assert!(result.contains("</body></html>"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib build::analytics::tests -- --nocapture 2>&1 | head -20`
Expected: compilation errors — `build_google_snippet`, `build_umami_snippet` don't exist, `inject_analytics` has wrong signature.

- [ ] **Step 3: Rewrite analytics.rs implementation**

Replace the module doc comment and both functions (lines 1–41) in `src/build/analytics.rs` with:

```rust
//! Analytics snippet injection.
//!
//! When `[analytics.google]` and/or `[analytics.umami]` are configured in
//! site.toml, the corresponding tracking snippets are injected into every
//! full rendered page before `</body>`.
//! Fragment files are not affected — analytics only belongs on full pages.

use crate::config::{AnalyticsConfig, UmamiAnalyticsConfig};

/// Build the Google Analytics gtag.js snippet.
fn build_google_snippet(tracking_id: &str) -> String {
    format!(
        r#"<script async src="https://www.googletagmanager.com/gtag/js?id={id}"></script>
<script>
  window.dataLayer = window.dataLayer || [];
  function gtag(){{dataLayer.push(arguments);}}
  gtag('js', new Date());
  gtag('config', '{id}');
</script>"#,
        id = tracking_id,
    )
}

/// Build the Umami analytics snippet.
///
/// Optional `data-*` attributes are only emitted when configured:
/// - `data-domains` when `domains` is set
/// - `data-auto-track="false"` when `auto_track` is false (true is Umami's default)
/// - `data-tag` when `tag` is set
fn build_umami_snippet(config: &UmamiAnalyticsConfig) -> String {
    let mut attrs = format!(
        r#"<script defer src="{}/script.js"
  data-website-id="{}""#,
        config.host_url, config.website_id,
    );
    if let Some(ref domains) = config.domains {
        attrs.push_str(&format!(r#"
  data-domains="{domains}""#));
    }
    if !config.auto_track {
        attrs.push_str(r#"
  data-auto-track="false""#);
    }
    if let Some(ref tag) = config.tag {
        attrs.push_str(&format!(r#"
  data-tag="{tag}""#));
    }
    attrs.push_str("></script>");
    attrs
}

/// Inject analytics snippets into rendered HTML before `</body>`.
///
/// Collects all enabled provider snippets (Google first, then Umami) and
/// inserts them before the last `</body>` tag. If no `</body>` tag is found,
/// the snippets are appended at the end.
pub fn inject_analytics(html: &str, config: &AnalyticsConfig) -> String {
    let mut snippets = Vec::new();

    if let Some(ref google) = config.google {
        snippets.push(build_google_snippet(&google.tracking_id));
    }
    if let Some(ref umami) = config.umami {
        snippets.push(build_umami_snippet(umami));
    }

    if snippets.is_empty() {
        return html.to_string();
    }

    let combined = snippets.join("\n");
    let lower = html.to_lowercase();

    if let Some(pos) = lower.rfind("</body>") {
        let mut result = String::with_capacity(html.len() + combined.len() + 2);
        result.push_str(&html[..pos]);
        result.push('\n');
        result.push_str(&combined);
        result.push('\n');
        result.push_str(&html[pos..]);
        result
    } else {
        format!("{html}\n{combined}")
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib build::analytics::tests -- --nocapture`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/build/analytics.rs
git commit -m "feat(analytics): add Umami snippet builder and multi-provider injection"
```

---

### Task 4: Update Render Call Site

**Files:**
- Modify: `src/build/render.rs:663-668`

- [ ] **Step 1: Update inject_analytics call in render.rs**

Replace lines 663–668 in `src/build/render.rs`:

```rust
    // Inject analytics snippets if configured.
    let full_html = if let Some(ref analytics) = config.analytics {
        analytics::inject_analytics(&full_html, analytics)
    } else {
        full_html
    };
```

The change: pass `analytics` (the `&AnalyticsConfig`) instead of `&analytics.tracking_id`.

- [ ] **Step 2: Run full test suite to catch any breakage**

Run: `cargo test 2>&1 | tail -20`
Expected: all tests pass. No other call sites reference the old `inject_analytics` signature.

- [ ] **Step 3: Commit**

```bash
git add src/build/render.rs
git commit -m "fix(render): update inject_analytics call for new provider config"
```

---

### Task 5: Write Feature Documentation

**Files:**
- Create: `docs/analytics.md`

- [ ] **Step 1: Write docs/analytics.md**

```markdown
# Analytics

Eigen injects analytics tracking snippets into every full rendered page before
`</body>`. Fragment files are not affected. Configure one or both providers
under `[analytics]` in `site.toml`.

## Google Analytics

```toml
[analytics.google]
tracking_id = "G-XXXXXXXXXX"
```

| Field | Required | Description |
|-------|----------|-------------|
| `tracking_id` | yes | Google Analytics measurement ID (e.g. `G-XXXXXXXXXX`) |

Injects the standard gtag.js async snippet.

## Umami

```toml
[analytics.umami]
website_id = "abc-123-def"
host_url = "https://analytics.example.com"
domains = "example.com,www.example.com"
auto_track = true
tag = "production"
```

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `website_id` | yes | — | Website ID from the Umami dashboard |
| `host_url` | no | `https://cloud.umami.is` | Base URL of your Umami instance |
| `domains` | no | — | Comma-separated domains to restrict tracking to |
| `auto_track` | no | `true` | Automatically track page views |
| `tag` | no | — | Custom event tag applied to all events |

Injects a deferred script tag loading `{host_url}/script.js` with the
configured `data-*` attributes.

## Using Both

Both providers can be active simultaneously:

```toml
[analytics.google]
tracking_id = "G-XXXXXXXXXX"

[analytics.umami]
website_id = "abc-123-def"
```

Google's snippet is injected first, followed by Umami's.

## Disabling

Omit the `[analytics]` section entirely, or remove both sub-tables, to
disable all analytics injection.
```

- [ ] **Step 2: Run the full test suite one final time**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add docs/analytics.md
git commit -m "docs: add analytics feature documentation"
```
