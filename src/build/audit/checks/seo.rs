//! SEO audit checks.

use crate::build::audit::Finding;

/// Run SEO checks on the rendered HTML of a single page.
///
/// Checks will be implemented in a follow-up task.
pub fn check(_html: &str, _url_path: &str) -> Vec<Finding> {
    Vec::new()
}
