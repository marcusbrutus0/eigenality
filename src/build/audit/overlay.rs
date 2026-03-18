//! Audit overlay badge injection.
//!
//! Injects a small floating badge into rendered HTML pages during dev
//! builds showing the audit score / finding count.

use crate::build::audit::AuditReport;

/// Inject an audit overlay badge into the given HTML string.
///
/// Implementation will be added in a follow-up task.
pub fn inject_overlay(_html: &str, _report: &AuditReport, _url_path: &str) -> String {
    _html.to_string()
}
