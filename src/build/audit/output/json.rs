use super::super::AuditReport;
use eyre::Result;
use std::path::Path;

pub fn write_json(_report: &AuditReport, _dist_path: &Path) -> Result<()> {
    Ok(())
}
