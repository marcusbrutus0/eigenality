//! Performance audit checks.

use super::super::{Category, Finding, Fix, Scope, Severity};
use crate::config::SiteConfig;

/// Site-level performance checks (config inspection).
pub fn site_checks(_config: &SiteConfig) -> Vec<Finding> {
    Vec::new()
}

/// Page-level performance checks (HTML inspection).
pub fn page_checks(
    _html: &str,
    _page_path: &str,
    _template_path: &str,
    _config: &SiteConfig,
) -> Vec<Finding> {
    Vec::new()
}
