//! Post-build HTML auditor.
//!
//! Runs SEO, performance, accessibility, and best-practices checks on
//! rendered pages and produces reports in multiple output formats.

pub mod checks;
pub mod output;
pub mod overlay;

use std::path::Path;

use eyre::Result;
use serde::Serialize;

use crate::build::render::RenderedPage;
use crate::config::AuditConfig;

/// Severity level for an audit finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A single audit finding attached to one page.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Machine-readable check identifier, e.g. `"seo-title"`.
    pub check_id: String,
    /// Human-readable one-line description of the issue.
    pub message: String,
    /// Severity level.
    pub severity: Severity,
    /// The category this check belongs to.
    pub category: Category,
}

/// Audit category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Seo,
    Performance,
    Accessibility,
    BestPractices,
}

/// Aggregated audit results for one page.
#[derive(Debug, Clone, Serialize)]
pub struct PageAudit {
    /// URL path of the page, e.g. `/about.html`.
    pub url_path: String,
    /// Source template path (if available).
    pub template_path: Option<String>,
    /// Findings for this page.
    pub findings: Vec<Finding>,
}

/// Full audit report across all pages.
#[derive(Debug, Clone, Serialize)]
pub struct AuditReport {
    /// Per-page results.
    pub pages: Vec<PageAudit>,
    /// Total number of findings.
    pub total_findings: usize,
    /// Counts by severity.
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

/// Run all audit checks on the rendered pages.
///
/// Returns an `AuditReport` with findings filtered by the ignore list
/// in `audit_config`.
pub fn run_audit(
    dist_dir: &Path,
    rendered_pages: &[RenderedPage],
    audit_config: &AuditConfig,
) -> Result<AuditReport> {
    let mut pages = Vec::with_capacity(rendered_pages.len());

    for rp in rendered_pages {
        let html_path = dist_dir.join(rp.url_path.trim_start_matches('/'));
        let html = match std::fs::read_to_string(&html_path) {
            Ok(h) => h,
            Err(_) => continue, // skip pages we can't read
        };

        let mut findings = Vec::new();
        findings.extend(checks::seo::check(&html, &rp.url_path));
        findings.extend(checks::performance::check(&html, &rp.url_path));
        findings.extend(checks::accessibility::check(&html, &rp.url_path));
        findings.extend(checks::best_practices::check(&html, &rp.url_path));

        // Filter out ignored checks.
        if !audit_config.ignore.is_empty() {
            findings.retain(|f| !audit_config.ignore.contains(&f.check_id));
        }

        pages.push(PageAudit {
            url_path: rp.url_path.clone(),
            template_path: rp.template_path.clone(),
            findings,
        });
    }

    let total_findings: usize = pages.iter().map(|p| p.findings.len()).sum();
    let errors = pages
        .iter()
        .flat_map(|p| &p.findings)
        .filter(|f| f.severity == Severity::Error)
        .count();
    let warnings = pages
        .iter()
        .flat_map(|p| &p.findings)
        .filter(|f| f.severity == Severity::Warning)
        .count();
    let infos = total_findings - errors - warnings;

    Ok(AuditReport {
        pages,
        total_findings,
        errors,
        warnings,
        infos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_audit_empty_pages() {
        let tmp = tempfile::tempdir().unwrap();
        let config = AuditConfig::default();
        let report = run_audit(tmp.path(), &[], &config).unwrap();
        assert_eq!(report.total_findings, 0);
        assert!(report.pages.is_empty());
    }

    #[test]
    fn test_run_audit_filters_ignored_checks() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = tmp.path();

        // Write a minimal HTML file (no <title> — should trigger seo-title).
        std::fs::write(dist.join("index.html"), "<html><body>hi</body></html>").unwrap();

        let pages = vec![RenderedPage {
            url_path: "/index.html".into(),
            is_index: true,
            is_dynamic: false,
            template_path: None,
        }];

        // Without ignore — should have findings.
        let config = AuditConfig::default();
        let report = run_audit(dist, &pages, &config).unwrap();
        let has_findings = report.total_findings > 0;

        // With ignore list covering all findings — should filter them out.
        let all_ids: Vec<String> = report.pages[0]
            .findings
            .iter()
            .map(|f| f.check_id.clone())
            .collect();
        let ignore_config = AuditConfig { ignore: all_ids };
        let filtered = run_audit(dist, &pages, &ignore_config).unwrap();

        // If there were findings before, they should now be gone.
        if has_findings {
            assert_eq!(filtered.total_findings, 0);
        }
    }
}
