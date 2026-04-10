# GitHub Action

## Overview

The `setup-eigen` GitHub Action installs eigen and optionally runs `eigen build`
in your CI pipeline. It uses the cargo-dist shell installer for platform
detection and binary download.

## Usage

Add to your workflow:

```yaml
steps:
  - uses: actions/checkout@v4
  - uses: wavefunk/setup-eigen@v1
```

This installs the latest eigen and runs `eigen build` in the repo root.

## Inputs

| Input | Default | Description |
|-------|---------|-------------|
| `version` | `latest` | Eigen version (e.g. `0.13.0`). |
| `build` | `true` | Run `eigen build` after install. |
| `source` | `.` | Working directory for `eigen build`. |
| `args` | `""` | Extra arguments for `eigen build`. |

## Outputs

| Output | Description |
|--------|-------------|
| `version` | Installed eigen version string. |

## Examples

### Pin a version

```yaml
- uses: wavefunk/setup-eigen@v1
  with:
    version: '0.13.0'
```

### Build a site in a subdirectory

```yaml
- uses: wavefunk/setup-eigen@v1
  with:
    source: './my-site'
```

### Install only (no build)

```yaml
- uses: wavefunk/setup-eigen@v1
  with:
    build: 'false'
```

### Deploy to GitHub Pages

```yaml
name: Deploy
on:
  push:
    branches: [main]
permissions:
  contents: read
  pages: write
  id-token: write
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: wavefunk/setup-eigen@v1
      - uses: actions/upload-pages-artifact@v3
        with:
          path: dist
      - uses: actions/deploy-pages@v4
```

## Platform Support

Supported runners (via cargo-dist installer):
- `ubuntu-latest` (x86_64 Linux)
- `macos-latest` (Apple Silicon)
- `macos-13` (Intel macOS)
- `windows-latest` (x86_64 Windows)
