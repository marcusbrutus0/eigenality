use eyre::{Result, WrapErr, bail};
use serde_json::Value;
use std::path::Path;

/// Resolve `_file` key suffixes in a JSON value tree.
///
/// Recursively walks `value`. For each object key ending in `_file`, reads the
/// file at the given project-root-relative path and replaces the key with a
/// sibling whose name has the suffix stripped, containing the file contents.
///
/// Errors if a `_file` value is not a string, the referenced file cannot be
/// read, or both the target key and the `_file` key exist on the same object.
pub fn resolve_file_links(value: Value, project_root: &Path) -> Result<Value> {
    match value {
        Value::Object(map) => resolve_object(map, project_root),
        Value::Array(arr) => {
            let resolved: Result<Vec<Value>> = arr
                .into_iter()
                .map(|v| resolve_file_links(v, project_root))
                .collect();
            Ok(Value::Array(resolved?))
        }
        other => Ok(other),
    }
}

fn resolve_object(
    map: serde_json::Map<String, Value>,
    project_root: &Path,
) -> Result<Value> {
    let mut result = serde_json::Map::with_capacity(map.len());

    for (key, _) in &map {
        if let Some(target_key) = key.strip_suffix("_file") {
            if map.contains_key(target_key) {
                bail!(
                    "Conflict: both '{}' and '{}' exist in the same object",
                    target_key,
                    key
                );
            }
        }
    }

    for (key, value) in map {
        if let Some(target_key) = key.strip_suffix("_file") {
            let path_str = value
                .as_str()
                .ok_or_else(|| eyre::eyre!("'{}' must be a string file path", key))?;

            let full_path = project_root.join(path_str);
            let contents = std::fs::read_to_string(&full_path)
                .wrap_err_with(|| format!("Failed to read file linked by '{}': {}", key, full_path.display()))?;

            result.insert(target_key.to_string(), Value::String(contents));
        } else {
            result.insert(key, resolve_file_links(value, project_root)?);
        }
    }

    Ok(Value::Object(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn test_basic_resolution() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "docs/hello.md", "# Hello World");

        let input = serde_json::json!({
            "title": "Hello",
            "content_file": "docs/hello.md"
        });

        let result = resolve_file_links(input, root).unwrap();
        assert_eq!(result["title"], "Hello");
        assert_eq!(result["content"], "# Hello World");
        assert!(result.get("content_file").is_none());
    }

    #[test]
    fn test_array_of_objects() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "docs/a.md", "# A");
        write(root, "docs/b.md", "# B");

        let input = serde_json::json!([
            { "title": "A", "content_file": "docs/a.md" },
            { "title": "B", "content_file": "docs/b.md" }
        ]);

        let result = resolve_file_links(input, root).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr[0]["content"], "# A");
        assert_eq!(arr[1]["content"], "# B");
        assert!(arr[0].get("content_file").is_none());
    }

    #[test]
    fn test_nested_objects() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "data/bio.txt", "A short bio.");

        let input = serde_json::json!({
            "author": {
                "name": "Alice",
                "bio_file": "data/bio.txt"
            }
        });

        let result = resolve_file_links(input, root).unwrap();
        assert_eq!(result["author"]["bio"], "A short bio.");
        assert!(result["author"].get("bio_file").is_none());
    }

    #[test]
    fn test_no_file_keys_passthrough() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!({
            "title": "Hello",
            "count": 42
        });

        let result = resolve_file_links(input.clone(), root).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_scalar_passthrough() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!("just a string");
        let result = resolve_file_links(input.clone(), root).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn test_missing_file_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!({
            "content_file": "nonexistent.md"
        });

        let err = resolve_file_links(input, root).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("content_file"), "error should name the key: {}", msg);
    }

    #[test]
    fn test_non_string_value_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let input = serde_json::json!({
            "content_file": 42
        });

        let err = resolve_file_links(input, root).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("content_file"), "error should name the key: {}", msg);
        assert!(msg.contains("string"), "error should mention string: {}", msg);
    }

    #[test]
    fn test_conflict_error() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "docs/hello.md", "# Hello");

        let input = serde_json::json!({
            "content": "inline",
            "content_file": "docs/hello.md"
        });

        let err = resolve_file_links(input, root).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("Conflict"), "error should mention conflict: {}", msg);
    }

    #[test]
    fn test_deeply_nested_in_array() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "x.txt", "deep content");

        let input = serde_json::json!([
            [{ "data_file": "x.txt" }]
        ]);

        let result = resolve_file_links(input, root).unwrap();
        assert_eq!(result[0][0]["data"], "deep content");
    }
}
