# File-Linked Data in `_data` Files

## Problem

Data files in `_data/` (YAML/JSON) sometimes need to include large text content — for example, `website/_data/docs.yaml` has 23 entries with full markdown content copied inline. The same content exists in `docs/*.md` files, creating duplication and a maintenance burden.

## Solution

Support a `_file` key suffix convention in `_data` files. Any object key ending in `_file` is resolved at load time: the value (a project-root-relative file path) is read, and a sibling key with the suffix stripped is inserted containing the file contents as a string.

### Example

```yaml
# Before resolution
- title: "Analytics"
  slug: analytics
  content_file: "docs/analytics.md"

# After resolution
- title: "Analytics"
  slug: analytics
  content: "# Analytics\n\nEigen injects analytics tracking..."
```

## Design

### Core function

`resolve_file_links(value: Value, project_root: &Path) -> Result<Value>`

Recursively walks a `serde_json::Value` tree. For each object:
1. Find keys ending in `_file`
2. Validate the value is a string
3. Read the referenced file (path relative to project root)
4. Insert a sibling key with `_file` stripped, containing file contents as `Value::String`
5. Remove the `_file` key
6. Continue recursing into nested objects and arrays (but not into resolved string content)

### File location

`src/data/file_links.rs`, registered in the `data` module.

### Integration points

1. `load_global_data()` in `src/data/global.rs` — resolve after parsing each file, before inserting into the HashMap
2. `fetch_file()` in `src/data/fetcher.rs` — resolve after parsing, before caching

### Path resolution

File paths are relative to the project root, not `_data/`. This supports linking to files outside `_data/` (e.g., `docs/analytics.md`).

### Error handling

| Case | Behavior |
|------|----------|
| `_file` value is not a string | Error naming the key and parent context |
| Referenced file doesn't exist | Error with the attempted path |
| Referenced file isn't valid UTF-8 | Error |
| Target key already exists (e.g., both `content` and `content_file`) | Error naming the conflict |

### Not in scope

- Glob patterns or directory references
- Parsing linked files as YAML/JSON (resolved value is always raw text)
- Watching linked files for incremental rebuild invalidation
- Recursive resolution inside resolved content

## Testing

- Unit tests in `src/data/file_links.rs` covering: basic resolution, nested objects, arrays of objects, missing file error, non-string value error, conflict error, no `_file` keys (passthrough), deeply nested structures
- Integration test: build a site with `_file` references and verify rendered output matches
- Update `website/_data/docs.yaml` to use `content_file` pointing to `docs/*.md`
