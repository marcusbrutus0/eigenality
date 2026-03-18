//! Markdown audit report output.

use std::path::Path;

use eyre::Result;

use crate::build::audit::AuditReport;

/// Write the audit report as a Markdown file.
///
/// Implementation will be added in a follow-up task.
pub fn write_markdown_report(_dist_dir: &Path, _report: &AuditReport) -> Result<()> {
    Ok(())
}
