//! Step 7.5: File watcher with debounce.
//!
//! Uses the `notify` crate to watch `templates/`, `_data/`, `static/`, and
//! `site.toml` for changes. Events are debounced at 200ms — multiple rapid
//! saves within that window produce a single rebuild.
//!
//! The watcher classifies changes into a `RebuildScope` so the rebuild
//! engine can take the most efficient path.

use eyre::Result;
use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::broadcast;

/// What kind of rebuild is needed based on which files changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildScope {
    /// `site.toml` changed — full config reload + rebuild.
    Full,
    /// Files in `_data/` changed — re-render all pages with fresh file data.
    DataOnly,
    /// One or more template files changed.
    Templates(Vec<PathBuf>),
    /// Only `static/` files changed — re-copy static assets only.
    StaticOnly,
}

/// Start the file watcher in a blocking loop.
///
/// Sends a `RebuildScope` on `rebuild_tx` whenever relevant files change.
/// This function blocks forever (or until the sender is dropped).
pub fn watch(
    project_root: &Path,
    rebuild_tx: broadcast::Sender<RebuildScope>,
) -> Result<()> {
    let templates_dir = project_root.join("templates");
    let data_dir = project_root.join("_data");
    let static_dir = project_root.join("static");
    let site_toml = project_root.join("site.toml");

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res && !event.kind.is_other() && !event.kind.is_access() {
            let _ = tx.send(event);
        }
    })?;

    // Watch directories that exist.
    if templates_dir.is_dir() {
        watcher.watch(&templates_dir, RecursiveMode::Recursive)?;
    }
    if data_dir.is_dir() {
        watcher.watch(&data_dir, RecursiveMode::Recursive)?;
    }
    if static_dir.is_dir() {
        watcher.watch(&static_dir, RecursiveMode::Recursive)?;
    }
    if site_toml.exists() {
        watcher.watch(&site_toml, RecursiveMode::NonRecursive)?;
    }

    tracing::info!("File watcher started.");

    loop {
        // Wait for the first event.
        match rx.recv() {
            Ok(first_event) => {
                let mut all_paths: Vec<PathBuf> = first_event.paths;

                // Drain any additional events that arrive within 200ms.
                loop {
                    match rx.recv_timeout(Duration::from_millis(200)) {
                        Ok(event) => {
                            all_paths.extend(event.paths);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                    }
                }


                // Classify the scope.
                let scope = classify_changes(&all_paths, project_root);

                // Send rebuild signal.
                let _ = rebuild_tx.send(scope);
            }
            Err(mpsc::RecvError) => {
                // Channel closed — watcher is done.
                return Ok(());
            }
        }
    }
}

/// Classify a set of changed file paths into a `RebuildScope`.
///
/// Priority: Full > DataOnly > Templates > StaticOnly
fn classify_changes(paths: &[PathBuf], project_root: &Path) -> RebuildScope {
    let templates_dir = project_root.join("templates");
    let data_dir = project_root.join("_data");
    let static_dir = project_root.join("static");
    let site_toml = project_root.join("site.toml");

    let mut has_config = false;
    let mut has_data = false;
    let mut has_static = false;
    let mut changed_templates: Vec<PathBuf> = Vec::new();

    for path in paths {
        if path == &site_toml || path.starts_with(&site_toml) {
            has_config = true;
        } else if path.starts_with(&data_dir) {
            has_data = true;
        } else if path.starts_with(&templates_dir) {
            // Store relative path from templates/.
            if let Ok(rel) = path.strip_prefix(&templates_dir) {
                changed_templates.push(rel.to_path_buf());
            }
        } else if path.starts_with(&static_dir) {
            has_static = true;
        }
    }

    // Priority ordering.
    if has_config {
        RebuildScope::Full
    } else if has_data && !changed_templates.is_empty() {
        // Both data and templates changed — need full data refresh + re-render.
        RebuildScope::Full
    } else if has_data {
        RebuildScope::DataOnly
    } else if !changed_templates.is_empty() {
        RebuildScope::Templates(changed_templates)
    } else if has_static {
        RebuildScope::StaticOnly
    } else {
        // Unknown change — full rebuild to be safe.
        RebuildScope::Full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_config_change() {
        let root = Path::new("/project");
        let paths = vec![PathBuf::from("/project/site.toml")];
        assert_eq!(classify_changes(&paths, root), RebuildScope::Full);
    }

    #[test]
    fn test_classify_data_change() {
        let root = Path::new("/project");
        let paths = vec![PathBuf::from("/project/_data/nav.yaml")];
        assert_eq!(classify_changes(&paths, root), RebuildScope::DataOnly);
    }

    #[test]
    fn test_classify_template_change() {
        let root = Path::new("/project");
        let paths = vec![PathBuf::from("/project/templates/index.html")];
        let scope = classify_changes(&paths, root);
        match scope {
            RebuildScope::Templates(t) => {
                assert_eq!(t.len(), 1);
                assert_eq!(t[0], PathBuf::from("index.html"));
            }
            other => panic!("Expected Templates, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_static_change() {
        let root = Path::new("/project");
        let paths = vec![PathBuf::from("/project/static/css/style.css")];
        assert_eq!(classify_changes(&paths, root), RebuildScope::StaticOnly);
    }

    #[test]
    fn test_classify_data_and_template_is_full() {
        let root = Path::new("/project");
        let paths = vec![
            PathBuf::from("/project/_data/nav.yaml"),
            PathBuf::from("/project/templates/index.html"),
        ];
        assert_eq!(classify_changes(&paths, root), RebuildScope::Full);
    }

    #[test]
    fn test_classify_config_trumps_all() {
        let root = Path::new("/project");
        let paths = vec![
            PathBuf::from("/project/site.toml"),
            PathBuf::from("/project/_data/nav.yaml"),
            PathBuf::from("/project/templates/index.html"),
            PathBuf::from("/project/static/style.css"),
        ];
        assert_eq!(classify_changes(&paths, root), RebuildScope::Full);
    }

    #[test]
    fn test_classify_unknown_path_is_full() {
        let root = Path::new("/project");
        let paths = vec![PathBuf::from("/project/unknown/file.txt")];
        assert_eq!(classify_changes(&paths, root), RebuildScope::Full);
    }
}
