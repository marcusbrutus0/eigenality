//! Robots.txt generation.
//!
//! When `[robots] enabled = true`, the following priority applies:
//! 1. `static/robots.txt` — copied as-is if present (owner's file wins)
//! 2. `rules` in `site.toml` — generated from config if non-empty
//! 3. hardcoded default — `User-agent: *\nAllow: /`

use eyre::{Result, WrapErr};
use std::path::Path;

use crate::config::{RobotsConfig, RobotsRule, SiteConfig};

const DEFAULT_ROBOTS_TXT: &str = "User-agent: *\nAllow: /\n";

/// Write `dist/robots.txt` according to the priority rules.
pub fn write(project_root: &Path, dist_dir: &Path, config: &SiteConfig) -> Result<()> {
    let custom = project_root.join("static").join("robots.txt");
    let dest = dist_dir.join("robots.txt");

    if custom.exists() {
        std::fs::copy(&custom, &dest)
            .wrap_err_with(|| format!("Failed to copy robots.txt to {}", dest.display()))?;
        tracing::info!("Copying robots.txt... ✓");
    } else if !config.robots.rules.is_empty() {
        let content = build_content(&config.robots, &config.site.base_url);
        std::fs::write(&dest, &content)
            .wrap_err_with(|| format!("Failed to write robots.txt to {}", dest.display()))?;
        tracing::info!("Generating robots.txt from config... ✓");
    } else {
        std::fs::write(&dest, DEFAULT_ROBOTS_TXT)
            .wrap_err_with(|| format!("Failed to write robots.txt to {}", dest.display()))?;
        tracing::info!("Generating default robots.txt... ✓");
    }

    Ok(())
}

/// Build robots.txt content from `RobotsConfig` rules.
fn build_content(robots: &RobotsConfig, base_url: &str) -> String {
    let mut out = String::new();

    for rule in &robots.rules {
        out.push_str(&format_rule(rule));
        out.push('\n');
    }

    if robots.sitemap {
        let base = base_url.trim_end_matches('/');
        out.push_str(&format!("Sitemap: {}/sitemap.xml\n", base));
    }

    for url in &robots.extra_sitemaps {
        out.push_str(&format!("Sitemap: {}\n", url));
    }

    out
}

fn format_rule(rule: &RobotsRule) -> String {
    let mut out = format!("User-agent: {}\n", rule.user_agent);
    for path in &rule.allow {
        out.push_str(&format!("Allow: {}\n", path));
    }
    for path in &rule.disallow {
        out.push_str(&format!("Disallow: {}\n", path));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BuildConfig, SitemapConfig, SiteMeta};
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn make_config(robots: RobotsConfig) -> SiteConfig {
        SiteConfig {
            site: SiteMeta {
                name: "Test".into(),
                base_url: "https://example.com".into(),
            },
            build: BuildConfig::default(),
            sitemap: SitemapConfig::default(),
            robots,
            assets: Default::default(),
            sources: HashMap::new(),
            analytics: None,
            plugins: HashMap::new(),
        }
    }

    fn setup(custom_robots: Option<&str>) -> TempDir {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("dist")).unwrap();
        if let Some(content) = custom_robots {
            fs::create_dir_all(tmp.path().join("static")).unwrap();
            fs::write(tmp.path().join("static/robots.txt"), content).unwrap();
        }
        tmp
    }

    #[test]
    fn test_static_file_wins_over_rules() {
        let tmp = setup(Some("User-agent: *\nDisallow: /secret/\n"));
        let mut config = make_config(RobotsConfig {
            enabled: true,
            rules: vec![RobotsRule {
                user_agent: "*".into(),
                allow: vec!["/".into()],
                disallow: vec![],
            }],
            ..Default::default()
        });
        config.site.base_url = "https://example.com".into();
        write(tmp.path(), &tmp.path().join("dist"), &config).unwrap();
        let content = fs::read_to_string(tmp.path().join("dist/robots.txt")).unwrap();
        // static file wins — contains Disallow: /secret/, not Allow: /
        assert!(content.contains("Disallow: /secret/"));
        assert!(!content.contains("Allow: /"));
    }

    #[test]
    fn test_rules_used_when_no_static_file() {
        let tmp = setup(None);
        let config = make_config(RobotsConfig {
            enabled: true,
            sitemap: false,
            rules: vec![RobotsRule {
                user_agent: "*".into(),
                allow: vec![],
                disallow: vec!["/admin/".into()],
            }],
            ..Default::default()
        });
        write(tmp.path(), &tmp.path().join("dist"), &config).unwrap();
        let content = fs::read_to_string(tmp.path().join("dist/robots.txt")).unwrap();
        assert!(content.contains("User-agent: *"));
        assert!(content.contains("Disallow: /admin/"));
    }

    #[test]
    fn test_default_when_no_static_and_no_rules() {
        let tmp = setup(None);
        let config = make_config(RobotsConfig::default());
        write(tmp.path(), &tmp.path().join("dist"), &config).unwrap();
        let content = fs::read_to_string(tmp.path().join("dist/robots.txt")).unwrap();
        assert!(content.contains("User-agent: *"));
        assert!(content.contains("Allow: /"));
    }

    #[test]
    fn test_sitemap_directive_included() {
        let tmp = setup(None);
        let config = make_config(RobotsConfig {
            enabled: true,
            sitemap: true,
            rules: vec![RobotsRule {
                user_agent: "*".into(),
                allow: vec!["/".into()],
                disallow: vec![],
            }],
            ..Default::default()
        });
        write(tmp.path(), &tmp.path().join("dist"), &config).unwrap();
        let content = fs::read_to_string(tmp.path().join("dist/robots.txt")).unwrap();
        assert!(content.contains("Sitemap: https://example.com/sitemap.xml"));
    }

    #[test]
    fn test_sitemap_directive_excluded() {
        let tmp = setup(None);
        let config = make_config(RobotsConfig {
            enabled: true,
            sitemap: false,
            rules: vec![RobotsRule {
                user_agent: "*".into(),
                allow: vec!["/".into()],
                disallow: vec![],
            }],
            ..Default::default()
        });
        write(tmp.path(), &tmp.path().join("dist"), &config).unwrap();
        let content = fs::read_to_string(tmp.path().join("dist/robots.txt")).unwrap();
        assert!(!content.contains("Sitemap:"));
    }

    #[test]
    fn test_extra_sitemaps() {
        let tmp = setup(None);
        let config = make_config(RobotsConfig {
            enabled: true,
            sitemap: false,
            extra_sitemaps: vec!["https://other.example.com/sitemap.xml".into()],
            rules: vec![RobotsRule {
                user_agent: "*".into(),
                allow: vec!["/".into()],
                disallow: vec![],
            }],
        });
        write(tmp.path(), &tmp.path().join("dist"), &config).unwrap();
        let content = fs::read_to_string(tmp.path().join("dist/robots.txt")).unwrap();
        assert!(content.contains("Sitemap: https://other.example.com/sitemap.xml"));
    }

    #[test]
    fn test_multiple_rules() {
        let tmp = setup(None);
        let config = make_config(RobotsConfig {
            enabled: true,
            sitemap: false,
            rules: vec![
                RobotsRule {
                    user_agent: "*".into(),
                    allow: vec!["/".into()],
                    disallow: vec!["/admin/".into()],
                },
                RobotsRule {
                    user_agent: "Googlebot".into(),
                    allow: vec![],
                    disallow: vec!["/".into()],
                },
            ],
            ..Default::default()
        });
        write(tmp.path(), &tmp.path().join("dist"), &config).unwrap();
        let content = fs::read_to_string(tmp.path().join("dist/robots.txt")).unwrap();
        assert!(content.contains("User-agent: *"));
        assert!(content.contains("User-agent: Googlebot"));
        assert!(content.contains("Disallow: /admin/"));
    }
}
