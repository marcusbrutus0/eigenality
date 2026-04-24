//! Security headers file generation.
//!
//! Writes `dist/_headers` for CDN platforms (Cloudflare Pages, Netlify).
//! When `static/_headers` exists it is copied verbatim; otherwise the file
//! is generated from `[security_headers]` config.

use eyre::{Result, WrapErr};
use std::path::Path;

use crate::config::{SecurityHeadersConfig, SiteConfig};

/// Write `dist/_headers` according to the following priority:
///
/// 1. `static/_headers` exists → copy verbatim (config is ignored).
/// 2. Generate from `config.security_headers`.
pub fn write(project_root: &Path, dist_dir: &Path, config: &SiteConfig) -> Result<()> {
    let static_file = project_root.join("static").join("_headers");
    let dest = dist_dir.join("_headers");

    if static_file.exists() {
        std::fs::copy(&static_file, &dest)
            .wrap_err_with(|| format!("Failed to copy _headers to {}", dest.display()))?;
        tracing::info!("Copying user _headers file (config ignored)... ✓");
        return Ok(());
    }

    let content = build_headers_content(&config.security_headers);
    std::fs::write(&dest, &content)
        .wrap_err_with(|| format!("Failed to write _headers to {}", dest.display()))?;
    tracing::info!("Generating _headers... ✓");
    Ok(())
}

fn build_headers_content(cfg: &SecurityHeadersConfig) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("/*\n");

    // Always emitted — no valid reason to suppress.
    out.push_str("  X-Content-Type-Options: nosniff\n");

    if !cfg.x_frame_options.is_empty() {
        out.push_str("  X-Frame-Options: ");
        out.push_str(&cfg.x_frame_options);
        out.push('\n');
    }

    if !cfg.referrer_policy.is_empty() {
        out.push_str("  Referrer-Policy: ");
        out.push_str(&cfg.referrer_policy);
        out.push('\n');
    }

    if !cfg.permissions_policy.is_empty() {
        out.push_str("  Permissions-Policy: ");
        out.push_str(&cfg.permissions_policy);
        out.push('\n');
    }

    if let Some(ref csp) = cfg.csp {
        // Defensive: validation already rejects Some(""), but guard anyway.
        if !csp.is_empty() {
            out.push_str("  Content-Security-Policy: ");
            out.push_str(csp);
            out.push('\n');
        }
    }

    if let Some(ref hsts) = cfg.hsts {
        out.push_str("  Strict-Transport-Security: max-age=");
        out.push_str(&hsts.max_age.to_string());
        if hsts.include_subdomains {
            out.push_str("; includeSubDomains");
        }
        if hsts.preload {
            out.push_str("; preload");
        }
        out.push('\n');
    }

    // Sorted for deterministic output.
    let mut custom: Vec<_> = cfg.custom.iter().collect();
    custom.sort_by_key(|(k, _)| k.as_str());
    for (key, val) in custom {
        out.push_str("  ");
        out.push_str(key);
        out.push_str(": ");
        out.push_str(val);
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HstsConfig, SecurityHeadersConfig};
    use std::collections::HashMap;

    fn default_cfg() -> SecurityHeadersConfig {
        SecurityHeadersConfig::default()
    }

    #[test]
    fn test_default_output() {
        let content = build_headers_content(&default_cfg());
        assert!(content.contains("X-Content-Type-Options: nosniff"));
        assert!(content.contains("X-Frame-Options: DENY"));
        assert!(content.contains("Referrer-Policy: strict-origin-when-cross-origin"));
        assert!(content.contains("Permissions-Policy: camera=(), microphone=(), geolocation=()"));
        assert!(!content.contains("Content-Security-Policy"));
        assert!(!content.contains("Strict-Transport-Security"));
    }

    #[test]
    fn test_csp_included() {
        let mut cfg = default_cfg();
        cfg.csp = Some("default-src 'self'".to_string());
        let content = build_headers_content(&cfg);
        assert!(content.contains("  Content-Security-Policy: default-src 'self'\n"));
    }

    #[test]
    fn test_hsts_basic() {
        let mut cfg = default_cfg();
        cfg.hsts = Some(HstsConfig {
            max_age: 31536000,
            include_subdomains: false,
            preload: false,
        });
        let content = build_headers_content(&cfg);
        assert!(content.contains("  Strict-Transport-Security: max-age=31536000\n"));
        assert!(!content.contains("includeSubDomains"));
        assert!(!content.contains("preload"));
    }

    #[test]
    fn test_hsts_full() {
        let mut cfg = default_cfg();
        cfg.hsts = Some(HstsConfig {
            max_age: 31536000,
            include_subdomains: true,
            preload: true,
        });
        let content = build_headers_content(&cfg);
        assert!(content.contains(
            "  Strict-Transport-Security: max-age=31536000; includeSubDomains; preload\n"
        ));
    }

    #[test]
    fn test_suppress_header() {
        let mut cfg = default_cfg();
        cfg.x_frame_options = String::new();
        let content = build_headers_content(&cfg);
        assert!(!content.contains("X-Frame-Options"));
    }

    #[test]
    fn test_custom_headers() {
        let mut cfg = default_cfg();
        cfg.custom.insert(
            "Cross-Origin-Opener-Policy".to_string(),
            "same-origin".to_string(),
        );
        let content = build_headers_content(&cfg);
        assert!(content.contains("  Cross-Origin-Opener-Policy: same-origin\n"));
    }

    #[test]
    fn test_custom_sorted() {
        let mut cfg = default_cfg();
        cfg.custom
            .insert("Z-Custom".to_string(), "z-value".to_string());
        cfg.custom
            .insert("A-Custom".to_string(), "a-value".to_string());
        let content = build_headers_content(&cfg);
        let pos_a = content.find("A-Custom").unwrap();
        let pos_z = content.find("Z-Custom").unwrap();
        assert!(pos_a < pos_z, "custom headers must appear sorted by key");
    }

    #[test]
    fn test_x_content_type_always_present() {
        let mut cfg = default_cfg();
        cfg.x_frame_options = String::new();
        cfg.referrer_policy = String::new();
        cfg.permissions_policy = String::new();
        cfg.csp = None;
        cfg.hsts = None;
        cfg.custom = HashMap::new();
        let content = build_headers_content(&cfg);
        assert!(content.contains("X-Content-Type-Options: nosniff"));
    }

    #[test]
    fn test_csp_some_empty_is_suppressed() {
        let mut cfg = default_cfg();
        cfg.csp = Some(String::new());
        let content = build_headers_content(&cfg);
        assert!(!content.contains("Content-Security-Policy"));
    }

    #[test]
    fn test_write_generates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path();
        let dist_dir = project_root.join("dist");
        std::fs::create_dir_all(&dist_dir).unwrap();

        let config: SiteConfig = toml::from_str(
            r#"
[site]
name = "Test"
base_url = "https://example.com"
[security_headers]
enabled = true
"#,
        )
        .unwrap();

        write(project_root, &dist_dir, &config).unwrap();

        let headers_file = dist_dir.join("_headers");
        assert!(headers_file.exists());
        let content = std::fs::read_to_string(&headers_file).unwrap();
        assert!(content.contains("X-Content-Type-Options: nosniff"));
    }

    #[test]
    fn test_static_file_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path();
        let dist_dir = project_root.join("dist");
        let static_dir = project_root.join("static");
        std::fs::create_dir_all(&dist_dir).unwrap();
        std::fs::create_dir_all(&static_dir).unwrap();

        let custom_content = "/*\n  X-Custom: my-value\n";
        std::fs::write(static_dir.join("_headers"), custom_content).unwrap();

        let config: SiteConfig = toml::from_str(
            r#"
[site]
name = "Test"
base_url = "https://example.com"
[security_headers]
enabled = true
csp = "default-src 'self'"
"#,
        )
        .unwrap();

        write(project_root, &dist_dir, &config).unwrap();

        let result = std::fs::read_to_string(dist_dir.join("_headers")).unwrap();
        assert_eq!(
            result, custom_content,
            "static file must be copied verbatim"
        );
        assert!(
            !result.contains("Content-Security-Policy"),
            "generated CSP must not appear when static file wins"
        );
    }
}
