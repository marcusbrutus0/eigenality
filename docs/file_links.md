# File-Linked Data

Reference external files from `_data/` YAML and JSON files using the `_file`
key suffix. At build time, eigen reads the linked file and replaces the `_file`
key with a sibling key containing the file contents as a string.

## Usage

Add a key ending in `_file` to any object in a data file. The value is a path
relative to the project root.

```yaml
# _data/docs.yaml
- title: "Analytics"
  slug: analytics
  content_file: "docs/analytics.md"
```

At build time this resolves to:

```yaml
- title: "Analytics"
  slug: analytics
  content: "# Analytics\n\nEigen injects analytics tracking..."
```

Templates access the resolved key as normal:

```html
{{ doc.content | markdown }}
```

## Rules

- Paths are relative to the **project root**, not `_data/`.
- The `_file` suffix is stripped to produce the target key: `bio_file` → `bio`.
- If both the target key and `_file` key exist (e.g., `content` and
  `content_file`), the build fails with a conflict error.
- The linked file is read as UTF-8 text. It is not parsed as YAML or JSON.
- Resolution applies to both global data (`_data/` directory) and per-page
  data queries (`data:` in frontmatter).
- Nesting works: `_file` keys inside nested objects and arrays are resolved.
