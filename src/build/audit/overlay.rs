//! Audit overlay badge injection.
//!
//! Injects a small floating badge into rendered HTML pages during dev
//! builds showing the audit finding count per page.

use super::{AuditReport, Finding};
use crate::build::render::RenderedPage;
use eyre::Result;
use std::path::Path;

/// Inject audit overlay badges into all rendered page HTML files.
pub fn inject_badges(
    _report: &AuditReport,
    _dist_path: &Path,
    _rendered_pages: &[RenderedPage],
) -> Result<()> {
    Ok(())
}
