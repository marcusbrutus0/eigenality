# Incremental Builds

Eigen supports incremental builds that skip re-rendering pages whose inputs are unchanged. This makes repeated `eigen build` invocations fast when only a few pages have changed.

## How It Works

The incremental build system uses a two-tier invalidation model and a persisted manifest.

### Manifest

After each successful build, Eigen writes `.eigen_cache/build_manifest.json`. This file stores:

- Tier 1 global hashes: eigen version, config, layout templates, global data, content asset manifest.
- Per-page records: template path, template body hash, frontmatter hash, data hashes, output file paths.
- Per-dynamic-template slug lists (used for orphan detection).

The manifest is only written on successful completion. A failed build leaves the previous manifest intact so the next run starts from a clean known state.

### Tier 1: Global Invalidation

Before rendering any pages, Eigen computes:

| Hash | Source |
|------|--------|
| `eigen_version` | `CARGO_PKG_VERSION` |
| `config_hash` | SHA-256 of `site.toml` |
| `layout_hash` | SHA-256 of all `_`-prefixed templates under `templates/`, sorted by path |
| `global_data_hash` | SHA-256 of all files under `_data/`, sorted by path |
| `content_manifest_hash` | SHA-256 of the content-hashed asset manifest |

If **any** Tier 1 hash differs from the previous manifest, a full rebuild is triggered (all pages are re-rendered).

### Tier 2: Per-Page Invalidation

When Tier 1 is clean, each page is checked individually:

- **Template body hash**: SHA-256 of the template file's contents.
- **Frontmatter hash**: SHA-256 of the raw frontmatter YAML string.
- **Data hashes**: SHA-256 of each resolved data query result.
- **Output file existence**: all expected output files must exist on disk.

If all hashes match and all output files exist, the page is skipped — its previous manifest record is carried forward unchanged. Otherwise the page is re-rendered and its record is updated.

### Two-Stage `dist/` Preparation

To support incremental builds:

1. `dist/` is created (not wiped) and static assets are copied.
2. Tier 1 hashes are computed and compared to the previous manifest.
3. If a full rebuild is needed, `dist/` is wiped and static assets are re-copied.
4. If incremental, `dist/` is left intact and only changed pages are re-written.

### Orphan Detection

During incremental builds, pages present in the previous manifest but absent from the current build are treated as orphans — their output files are deleted from `dist/`. This keeps the output directory clean when templates are deleted or renamed.

## CLI Flags

```
eigen build             # incremental build (default)
eigen build --full      # force full rebuild, ignoring the manifest
eigen build --fresh     # clear data cache AND force full rebuild
```

`--fresh` implies `--full`.

Dev mode (`eigen dev`) always does a full rebuild and does not update or consult the manifest.

## Implementation

| File | Role |
|------|------|
| `src/build/incremental.rs` | Structs, hash functions, manifest load/save, invalidation checks, orphan deletion |
| `src/build/render.rs` | Two-stage dist prep, manifest threading in `BuildContext`, per-page skip in render functions, manifest save and orphan cleanup at build end |

Key types:

- `BuildManifest` — top-level manifest; serialized as JSON.
- `PageRecord` — per-page entry with all hashes and output file paths.
- `DynamicTemplateRecord` — per-dynamic-template list of rendered slugs.

Hash functions use SHA-256 via the `sha2` crate. All directory walks are sorted before hashing to ensure deterministic results regardless of filesystem ordering.
