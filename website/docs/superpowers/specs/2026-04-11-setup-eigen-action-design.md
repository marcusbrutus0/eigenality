# Design: setup-eigen GitHub Action

## Summary

A composite GitHub Action that installs eigen and optionally runs `eigen build`. Leverages the existing cargo-dist shell installer for platform detection and binary download.

## Inputs

| Input | Default | Description |
|-------|---------|-------------|
| `version` | `latest` | Eigen version (e.g., `0.13.0`). `latest` resolves to the newest stable release. |
| `build` | `true` | Whether to run `eigen build` after install. |
| `source` | `.` | Working directory for `eigen build`. |
| `args` | `""` | Additional arguments passed to `eigen build`. |

## Outputs

| Output | Description |
|--------|-------------|
| `version` | Installed eigen version string (from `eigen --version`). |

## How It Works

### Step 1: Install eigen

Construct the cargo-dist installer URL based on the `version` input:

- `latest` -> `https://github.com/wavefunk/eigen/releases/latest/download/eigen-installer.sh`
- `0.13.0` -> `https://github.com/wavefunk/eigen/releases/download/v0.13.0/eigen-installer.sh`

Download and run the installer. The cargo-dist installer handles:
- Platform/architecture detection
- Binary download and extraction
- Placing the binary in a suitable location

After install, add the binary location to `$GITHUB_PATH` so subsequent steps can use `eigen`. Capture the installed version via `eigen --version` and set it as an output.

### Step 2: Build (optional)

If `build` is `true` (default), run `eigen build` in the `source` directory with any extra `args`.

## Action Type

Composite action (shell steps only). No Node.js runtime, no dependencies.

## File Location

`github-action/action.yml` — to be moved to a `wavefunk/setup-eigen` repo later.

## Usage Examples

```yaml
# Minimal: latest version, build in current dir
- uses: wavefunk/setup-eigen@v1

# Pinned version, custom source
- uses: wavefunk/setup-eigen@v1
  with:
    version: '0.13.0'
    source: './my-site'

# Install only
- uses: wavefunk/setup-eigen@v1
  with:
    build: 'false'

# Extra build args
- uses: wavefunk/setup-eigen@v1
  with:
    args: '--minify --verbose'
```

## Platform Support

Inherits from cargo-dist targets:
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

GitHub Actions runners are Linux x86_64 by default, so the primary path is well-covered. macOS and Windows runners are also supported via the installer's platform detection.

## Error Handling

- If the requested version does not exist, the curl to the installer URL returns 404. The action fails with a clear message.
- If `eigen build` fails, the action step fails with eigen's exit code.

## Scope

This is a thin wrapper (~40 lines). No caching, no artifact upload, no deploy steps. Users compose those from standard actions.
