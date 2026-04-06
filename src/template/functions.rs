//! Step 4.4: Custom minijinja functions.
//!
//! Registers the following global functions:
//!
//! - `link_to(path, target?, block?)` — generates HTMX-compatible link attributes
//! - `current_year()` — returns the current year as a string
//! - `asset(path)` — returns the path to a static asset (for future cache-busting)

use std::sync::Arc;

use minijinja::Environment;
use minijinja::Value;

use crate::build::clean_link::to_clean_link;
use crate::build::content_hash::AssetManifest;
use crate::build::source_asset::{SourceAssetCollector, SOURCE_ASSET_PROXY_PREFIX, resolve_url};
use crate::config::SiteConfig;

/// Register all custom functions on the given environment.
pub fn register_functions(
    env: &mut Environment<'_>,
    config: &SiteConfig,
    manifest: Option<Arc<AssetManifest>>,
    dev_mode: bool,
    source_asset_collector: Option<SourceAssetCollector>,
) {
    let fragment_dir = config.build.fragment_dir.clone();
    let fragments_enabled = config.build.fragments;
    let content_block = config.build.content_block.clone();
    let clean_links = config.build.clean_links;

    // link_to(path, target?, block?)
    env.add_function(
        "link_to",
        move |path: &str,
              target: Option<&str>,
              block: Option<&str>|
              -> String {
            let target = target.unwrap_or("#content");

            let display_path = if clean_links {
                to_clean_link(path)
            } else {
                std::borrow::Cow::Borrowed(path)
            };

            if !fragments_enabled {
                return format!(r#"href="{}""#, display_path);
            }

            let block_name = block.unwrap_or(&content_block);
            let fragment_path = compute_fragment_path(path, &fragment_dir, block_name);

            format!(
                r#"href="{path}" hx-get="{fragment_path}" hx-target="{target}" hx-push-url="{path}""#,
                path = display_path,
                fragment_path = fragment_path,
                target = target,
            )
        },
    );

    // current_year()
    env.add_function("current_year", || -> String {
        chrono::Local::now().format("%Y").to_string()
    });

    // asset(path)
    // Returns the content-hashed path when a manifest is available,
    // otherwise passes through unchanged.
    let manifest_clone = manifest;
    env.add_function("asset", move |path: &str| -> String {
        let normalized = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };

        match &manifest_clone {
            Some(m) => m.resolve(&normalized).to_string(),
            None => normalized,
        }
    });

    // source_asset(source_name, url_or_path)
    if !config.sources.is_empty() {
        let sources = config.sources.clone();
        let collector = source_asset_collector.unwrap_or_default();
        env.add_function(
            "source_asset",
            move |source_name: &str, url_or_path: &str| -> Result<String, minijinja::Error> {
                if url_or_path.is_empty() {
                    return Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        "source_asset: url_or_path must be a non-empty string",
                    ));
                }

                let source = sources.get(source_name).ok_or_else(|| {
                    let available: Vec<_> = sources.keys().cloned().collect();
                    minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        format!(
                            "Unknown source '{}'. Available: {}",
                            source_name,
                            available.join(", "),
                        ),
                    )
                })?;

                let resolved = resolve_url(url_or_path, &source.url);

                if dev_mode {
                    Ok(build_proxy_url(source_name, &resolved, &source.url))
                } else {
                    collector.push(source_name.to_string(), resolved.clone());
                    Ok(resolved)
                }
            },
        );
    }

    // site — expose the full [site] config as a global variable.
    env.add_global(
        "site",
        Value::from_serialize(&config.site),
    );
}

/// Compute the fragment path for a given page path and block name.
///
/// Examples:
/// - `("/about.html", "_fragments", "content")` → `"/_fragments/about.html"`
/// - `("/posts/my-post.html", "_fragments", "content")` → `"/_fragments/posts/my-post.html"`
/// - `("/about.html", "_fragments", "sidebar")` → `"/_fragments/about/sidebar.html"`
///
/// The default content block uses the page filename directly. Non-default blocks
/// get their own subdirectory.
fn compute_fragment_path(page_path: &str, fragment_dir: &str, block: &str) -> String {
    let clean_path = page_path.trim_start_matches('/');

    // Normalise directory-style URLs ("/about/" or "/about") to a stem ("about")
    // and derive the equivalent .html path ("about/index.html" or "about.html").
    let (stem, html_path) = if clean_path.ends_with('/') {
        // "/about/" → stem="about", html_path="about.html"
        let s = clean_path.trim_end_matches('/');
        (s, format!("{}.html", s))
    } else if clean_path.is_empty() {
        // "/" → root index
        ("index", "index.html".to_string())
    } else if let Some(s) = clean_path.strip_suffix(".html") {
        // "/about.html" → stem="about", html_path="about.html"
        (s, clean_path.to_string())
    } else {
        // "/about" (no trailing slash, no extension) — append .html
        let s = clean_path;
        (s, format!("{}.html", s))
    };

    // For the default content block, the fragment file mirrors the html path.
    // For additional blocks, nest under a directory named after the stem.
    if block == "content" {
        format!("/{}/{}", fragment_dir, html_path)
    } else {
        format!("/{}/{}/{}.html", fragment_dir, stem, block)
    }
}

/// Build a dev proxy URL for a source asset.
///
/// - Same-host URLs: strip the source base path and use `/_proxy/{source}/{relative}`.
///   The proxy handler prepends the source base URL, so only the relative tail is needed.
/// - Cross-host URLs: use `/_proxy/{source}/__source_asset__/{full_url}`.
fn build_proxy_url(source_name: &str, resolved_url: &str, source_base_url: &str) -> String {
    let base_host = extract_host(source_base_url);
    let url_host = extract_host(resolved_url);

    if base_host == url_host {
        // Same host — extract the path portion relative to the source base URL's path.
        // The proxy handler prepends the full source base URL, so we only emit the
        // portion after that base path to avoid doubling (e.g. `/api/uploads` appearing twice).
        let base_path = extract_path(source_base_url);
        let resolved_path = extract_path(resolved_url);
        let relative = resolved_path
            .strip_prefix(base_path)
            .unwrap_or(resolved_path);
        let relative = relative.trim_start_matches('/');
        format!("/_proxy/{}/{}", source_name, relative)
    } else {
        format!("/_proxy/{}/{}{}", source_name, SOURCE_ASSET_PROXY_PREFIX, resolved_url)
    }
}

/// Extract the path portion of a URL (everything after `scheme://host[:port]`).
fn extract_path(url: &str) -> &str {
    url.find("://")
        .and_then(|i| url[i + 3..].find('/').map(|j| &url[i + 3 + j..]))
        .unwrap_or("/")
}

/// Extract hostname from a URL (without port).
fn extract_host(url: &str) -> &str {
    url.find("://")
        .map(|i| &url[i + 3..])
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BuildConfig, SiteSchemaConfig, SiteMeta, SiteSeoConfig};
    use minijinja::context;
    use std::collections::HashMap;

    fn test_config() -> SiteConfig {
        SiteConfig {
            site: SiteMeta {
                name: "Test Site".into(),
                base_url: "https://example.com".into(),
                seo: SiteSeoConfig::default(),
                schema: SiteSchemaConfig::default(),
                extra: std::collections::HashMap::new(),
            },
            build: BuildConfig {
                fragments: true,
                fragment_dir: "_fragments".into(),
                content_block: "content".into(),
                ..Default::default()
            },
            sitemap: Default::default(),
            robots: Default::default(),
            assets: Default::default(),
            sources: HashMap::new(),
            analytics: None,
            plugins: HashMap::new(),
            feed: HashMap::new(),
            audit: None,
        }
    }

    fn test_config_no_fragments() -> SiteConfig {
        SiteConfig {
            site: SiteMeta {
                name: "Test Site".into(),
                base_url: "https://example.com".into(),
                seo: SiteSeoConfig::default(),
                schema: SiteSchemaConfig::default(),
                extra: std::collections::HashMap::new(),
            },
            build: BuildConfig {
                fragments: false,
                ..Default::default()
            },
            sitemap: Default::default(),
            robots: Default::default(),
            assets: Default::default(),
            sources: HashMap::new(),
            analytics: None,
            plugins: HashMap::new(),
            feed: HashMap::new(),
            audit: None,
        }
    }

    // --- compute_fragment_path ---

    #[test]
    fn test_fragment_path_content_block() {
        let result = compute_fragment_path("/about.html", "_fragments", "content");
        assert_eq!(result, "/_fragments/about.html");
    }

    #[test]
    fn test_fragment_path_nested() {
        let result = compute_fragment_path("/posts/my-post.html", "_fragments", "content");
        assert_eq!(result, "/_fragments/posts/my-post.html");
    }

    #[test]
    fn test_fragment_path_non_content_block() {
        let result = compute_fragment_path("/about.html", "_fragments", "sidebar");
        assert_eq!(result, "/_fragments/about/sidebar.html");
    }

    #[test]
    fn test_fragment_path_no_leading_slash() {
        let result = compute_fragment_path("about.html", "_fragments", "content");
        assert_eq!(result, "/_fragments/about.html");
    }

    #[test]
    fn test_fragment_path_directory_trailing_slash() {
        let result = compute_fragment_path("/about/", "_fragments", "content");
        assert_eq!(result, "/_fragments/about.html");
    }

    #[test]
    fn test_fragment_path_directory_no_trailing_slash() {
        let result = compute_fragment_path("/about", "_fragments", "content");
        assert_eq!(result, "/_fragments/about.html");
    }

    #[test]
    fn test_fragment_path_directory_non_content_block() {
        let result = compute_fragment_path("/about/", "_fragments", "sidebar");
        assert_eq!(result, "/_fragments/about/sidebar.html");
    }

    #[test]
    fn test_fragment_path_nested_directory() {
        let result = compute_fragment_path("/case-study/flipkart/", "_fragments", "content");
        assert_eq!(result, "/_fragments/case-study/flipkart.html");
    }

    // --- link_to function ---

    #[test]
    fn test_link_to_default() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"<a {{ link_to("/about.html") }}>About</a>"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"href="/about.html""##));
        assert!(result.contains(r##"hx-get="/_fragments/about.html""##));
        assert!(result.contains(r##"hx-target="#content""##));
        assert!(result.contains(r##"hx-push-url="/about.html""##));
    }

    #[test]
    fn test_link_to_custom_target() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"{{ link_to("/about.html", "#main") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"hx-target="#main""##));
    }

    #[test]
    fn test_link_to_custom_block() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"{{ link_to("/about.html", "#sidebar", "sidebar") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"hx-get="/_fragments/about/sidebar.html""##));
    }

    #[test]
    fn test_link_to_no_fragments() {
        let mut env = Environment::new();
        let config = test_config_no_fragments();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"{{ link_to("/about.html") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert_eq!(result.trim(), r##"href="/about.html""##);
        assert!(!result.contains("hx-get"));
    }

    // --- current_year ---

    #[test]
    fn test_current_year() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", "{{ current_year() }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        let year: u32 = result.trim().parse().expect("should be a year number");
        assert!(year >= 2024);
    }

    // --- asset ---

    #[test]
    fn test_asset_with_leading_slash() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", "{{ asset('/css/style.css') }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();
        assert_eq!(result.trim(), "/css/style.css");
    }

    #[test]
    fn test_asset_without_leading_slash() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", "{{ asset('css/style.css') }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();
        assert_eq!(result.trim(), "/css/style.css");
    }

    // --- site global ---

    #[test]
    fn test_site_global() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", "{{ site.name }} - {{ site.base_url }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();
        assert_eq!(result.trim(), "Test Site - https://example.com");
    }

    // --- asset with manifest ---

    #[test]
    fn test_asset_with_manifest() {
        let mut env = Environment::new();
        let config = test_config();
        let mut manifest = AssetManifest::new();
        manifest.insert("/css/style.css".into(), "/css/style.abc123.css".into());
        let manifest = Arc::new(manifest);

        register_functions(&mut env, &config, Some(manifest), false, None);

        env.add_template("test", "{{ asset('/css/style.css') }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();
        assert_eq!(result.trim(), "/css/style.abc123.css");
    }

    #[test]
    fn test_asset_without_manifest() {
        let mut env = Environment::new();
        let config = test_config();
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", "{{ asset('/css/style.css') }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();
        assert_eq!(result.trim(), "/css/style.css");
    }

    #[test]
    fn test_asset_unknown_path_with_manifest() {
        let mut env = Environment::new();
        let config = test_config();
        let manifest = Arc::new(AssetManifest::new());

        register_functions(&mut env, &config, Some(manifest), false, None);

        env.add_template("test", "{{ asset('/unknown.css') }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();
        assert_eq!(result.trim(), "/unknown.css");
    }

    #[test]
    fn test_asset_normalizes_then_resolves() {
        let mut env = Environment::new();
        let config = test_config();
        let mut manifest = AssetManifest::new();
        manifest.insert("/css/style.css".into(), "/css/style.abc123.css".into());
        let manifest = Arc::new(manifest);

        register_functions(&mut env, &config, Some(manifest), false, None);

        // Path without leading slash should be normalized and then resolved.
        env.add_template("test", "{{ asset('css/style.css') }}")
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();
        assert_eq!(result.trim(), "/css/style.abc123.css");
    }

    #[test]
    fn test_link_to_clean_links_enabled() {
        let mut env = Environment::new();
        let mut config = test_config();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"<a {{ link_to("/about.html") }}>About</a>"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"href="/about""##));
        assert!(result.contains(r##"hx-get="/_fragments/about.html""##));
        assert!(result.contains(r##"hx-push-url="/about""##));
    }

    #[test]
    fn test_link_to_clean_links_index() {
        let mut env = Environment::new();
        let mut config = test_config();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"{{ link_to("/index.html") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"href="/""##));
        assert!(result.contains(r##"hx-push-url="/""##));
    }

    #[test]
    fn test_link_to_clean_links_already_clean() {
        let mut env = Environment::new();
        let mut config = test_config();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"{{ link_to("/about") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert!(result.contains(r##"href="/about""##));
        assert!(result.contains(r##"hx-push-url="/about""##));
    }

    #[test]
    fn test_link_to_clean_links_no_fragments() {
        let mut env = Environment::new();
        let mut config = test_config_no_fragments();
        config.build.clean_links = true;
        register_functions(&mut env, &config, None, false, None);

        env.add_template("test", r##"{{ link_to("/about.html") }}"##)
            .unwrap();
        let tmpl = env.get_template("test").unwrap();
        let result = tmpl.render(context! {}).unwrap();

        assert_eq!(result.trim(), r##"href="/about""##);
    }

    // --- source_asset ---

    fn make_config_with_source(name: &str, url: &str, auth: &str) -> SiteConfig {
        let mut config = test_config();
        config.sources.insert(name.to_string(), crate::config::SourceConfig {
            url: url.to_string(),
            headers: std::collections::HashMap::from([
                ("Authorization".to_string(), auth.to_string()),
            ]),
        });
        config
    }

    #[test]
    fn source_asset_dev_mode_returns_proxy_url_relative_path() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true, None);
        env.add_template("test", r#"{{ source_asset("my_cms", "/uploads/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "/_proxy/my_cms/uploads/photo.jpg");
    }

    #[test]
    fn source_asset_dev_mode_returns_proxy_url_absolute_same_host() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true, None);
        env.add_template("test", r#"{{ source_asset("my_cms", "https://cms.example.com/uploads/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "/_proxy/my_cms/uploads/photo.jpg");
    }

    #[test]
    fn source_asset_dev_mode_returns_full_proxy_url_different_host() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true, None);
        env.add_template("test", r#"{{ source_asset("my_cms", "https://media.example.com/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "/_proxy/my_cms/__source_asset__/https://media.example.com/photo.jpg");
    }

    #[test]
    fn source_asset_build_mode_returns_resolved_url() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, false, None);
        env.add_template("test", r#"{{ source_asset("my_cms", "/uploads/photo.jpg") }}"#).unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "https://cms.example.com/uploads/photo.jpg");
    }

    #[test]
    fn source_asset_unknown_source_errors() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true, None);
        env.add_template("test", r#"{{ source_asset("nope", "/img.jpg") }}"#).unwrap();
        let err = env.get_template("test").unwrap().render(()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nope"), "Error should name the bad source: {}", msg);
    }

    #[test]
    fn source_asset_empty_url_errors() {
        let mut env = Environment::new();
        let config = make_config_with_source("my_cms", "https://cms.example.com", "Bearer tok");
        register_functions(&mut env, &config, None, true, None);
        env.add_template("test", r#"{{ source_asset("my_cms", "") }}"#).unwrap();
        let err = env.get_template("test").unwrap().render(()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("non-empty"), "Error should mention non-empty: {}", msg);
    }

    #[test]
    fn source_asset_dev_mode_base_url_with_path_no_doubling() {
        let mut env = Environment::new();
        let config = make_config_with_source(
            "cms_assets",
            "http://localhost:4001/apps/id8nxt/uploads/file",
            "Bearer tok",
        );
        register_functions(&mut env, &config, None, true, None);
        env.add_template("test", r#"{{ source_asset("cms_assets", "abc123/hero.png") }}"#)
            .unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        // The base path /apps/id8nxt/uploads/file must NOT appear in the proxy URL —
        // the proxy handler prepends the full source base URL.
        assert_eq!(result, "/_proxy/cms_assets/abc123/hero.png");
    }

    #[test]
    fn source_asset_dev_mode_base_url_with_path_leading_slash() {
        let mut env = Environment::new();
        let config = make_config_with_source(
            "cms_assets",
            "http://localhost:4001/apps/id8nxt/uploads/file",
            "Bearer tok",
        );
        register_functions(&mut env, &config, None, true, None);
        env.add_template("test", r#"{{ source_asset("cms_assets", "/abc123/hero.png") }}"#)
            .unwrap();
        let result = env.get_template("test").unwrap().render(()).unwrap();
        assert_eq!(result, "/_proxy/cms_assets/abc123/hero.png");
    }
}
