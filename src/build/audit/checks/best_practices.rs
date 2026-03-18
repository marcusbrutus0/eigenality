//! Best-practices audit checks.

use super::super::{Category, Finding, Fix, Scope, Severity};
use crate::config::SiteConfig;
use std::path::Path;

/// Site-level best practices checks.
pub fn site_checks(_config: &SiteConfig, _dist_path: &Path) -> Vec<Finding> {
    Vec::new()
}

/// Page-level best practices checks (HTML inspection).
pub fn page_checks(_html: &str, _page_path: &str, _template_path: &str) -> Vec<Finding> {
    Vec::new()
}
