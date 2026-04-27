# setup-eigen GitHub Action Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a composite GitHub Action that installs eigen (via the cargo-dist shell installer) and optionally runs `eigen build`.

**Architecture:** Single `action.yml` composite action with shell steps. Constructs the cargo-dist installer URL from the version input, runs it, adds the binary to PATH, and optionally runs `eigen build`.

**Tech Stack:** GitHub Actions composite action (YAML + bash)

**Spec:** `docs/superpowers/specs/2026-04-11-setup-eigen-action-design.md`

---

## File Structure

- **Create:** `github-action/action.yml` — the composite action definition
- **Create:** `docs/github_action.md` — feature docs (per project convention)

---

### Task 1: Create the composite action

**Files:**
- Create: `github-action/action.yml`

- [ ] **Step 1: Create `github-action/action.yml`**

```yaml
name: 'Setup Eigen'
description: 'Install eigen static site generator and optionally run eigen build'
branding:
  icon: 'box'
  color: 'orange'

inputs:
  version:
    description: 'Eigen version to install (e.g. "0.13.0"). Use "latest" for newest stable release.'
    required: false
    default: 'latest'
  build:
    description: 'Run eigen build after install'
    required: false
    default: 'true'
  source:
    description: 'Working directory for eigen build'
    required: false
    default: '.'
  args:
    description: 'Additional arguments passed to eigen build'
    required: false
    default: ''

outputs:
  version:
    description: 'Installed eigen version'
    value: ${{ steps.version.outputs.version }}

runs:
  using: 'composite'
  steps:
    - name: Install eigen
      shell: bash
      env:
        EIGEN_VERSION: ${{ inputs.version }}
      run: |
        if [ "$EIGEN_VERSION" = "latest" ]; then
          url="https://github.com/wavefunk/eigen/releases/latest/download/eigen-installer.sh"
        else
          url="https://github.com/wavefunk/eigen/releases/download/v${EIGEN_VERSION}/eigen-installer.sh"
        fi
        echo "Downloading eigen installer from: $url"
        curl --proto '=https' --tlsv1.2 -fsSL "$url" | sh
        echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"

    - name: Get installed version
      id: version
      shell: bash
      run: |
        version="$("$HOME/.cargo/bin/eigen" --version | awk '{print $2}')"
        echo "version=$version" >> "$GITHUB_OUTPUT"
        echo "Installed eigen $version"

    - name: Build
      if: inputs.build == 'true'
      shell: bash
      working-directory: ${{ inputs.source }}
      run: eigen build ${{ inputs.args }}
```

- [ ] **Step 2: Verify YAML is valid**

Run: `python3 -c "import yaml; yaml.safe_load(open('github-action/action.yml'))"`
Expected: No output (valid YAML)

- [ ] **Step 3: Commit**

```bash
git add github-action/action.yml
git commit -m "feat: add setup-eigen GitHub Action"
```

---

### Task 2: Write feature docs

**Files:**
- Create: `docs/github_action.md`

- [ ] **Step 1: Create `docs/github_action.md`**

```markdown
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
```

- [ ] **Step 2: Commit**

```bash
git add docs/github_action.md
git commit -m "docs: add GitHub Action usage guide"
```
