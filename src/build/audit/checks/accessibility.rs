//! Accessibility audit checks.

use super::super::{Category, Finding, Fix, Scope, Severity};

/// Site-level accessibility checks (advisory).
pub fn site_checks() -> Vec<Finding> {
    Vec::new()
}

/// Page-level accessibility checks (HTML inspection).
pub fn page_checks(_html: &str, _page_path: &str, _template_path: &str) -> Vec<Finding> {
    Vec::new()
}
