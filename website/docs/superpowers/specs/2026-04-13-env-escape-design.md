# Design: `$${...}` Escape for Environment Variable Substitution

**Date:** 2026-04-13
**Status:** Approved

## Problem

Eigen replaces `${VAR_NAME}` with environment variable values in config (`site.toml`), global data (`_data/` files), and query interpolation. There is no way to include a literal `${SOME_VALUE}` string in content — it is always interpreted as a substitution and either replaced or causes an error (in strict mode) when the variable is missing.

## Solution

Introduce `$${VAR_NAME}` as an escape sequence that renders to the literal text `${VAR_NAME}` in output. The double-dollar prefix signals "do not substitute."

## Mechanism

A two-phase approach using a sentinel:

1. **Pre-pass:** Before the env var regex runs, find all `$${...}` patterns and replace them with a sentinel string that cannot appear in normal content (e.g., `\x00EIGEN_ESC{VAR_NAME}\x00`).
2. **Substitution:** Run the normal `${VAR}` regex replacement as today. The sentinels are invisible to the regex.
3. **Post-pass:** Replace all sentinels with the literal `${VAR_NAME}` text.

This avoids the regex matching the inner `${...}` after stripping the leading `$`.

## Scope

The escape applies uniformly in all three substitution sites:

- `config/mod.rs::interpolate_env_vars` — strict mode (used by `site.toml` and `_data/` files via `global.rs`)
- `data/query.rs::interpolate_env_in_string` — lenient mode (used by query bodies, filters, paths)

No changes to `global.rs` needed since it delegates to `interpolate_env_vars`.

## Behavior

| Input | Output | Notes |
|---|---|---|
| `${HOME}` | `/home/user` | Normal substitution (unchanged) |
| `$${SOME_VALUE}` | `${SOME_VALUE}` | Escaped — literal output |
| `$${HOME}` | `${HOME}` | Escaped — even if HOME is set |
| `${MISSING}` in strict mode | Error | Unchanged behavior |
| `$${MISSING}` in strict mode | `${MISSING}` | No error — it's escaped |
| `${MISSING}` in lenient mode | `${MISSING}` | Unchanged behavior |
| `Use $${API_KEY} for auth and ${HOST}` | `Use ${API_KEY} for auth and example.com` | Mixed escaped + real |

## Changes

### Code
- **`src/config/mod.rs::interpolate_env_vars`** — add sentinel pre-pass/post-pass around existing regex logic
- **`src/data/query.rs::interpolate_env_in_string`** — same sentinel pre-pass/post-pass

### Tests
- Escape in strict mode (config): `$${FOO}` produces `${FOO}`, no error
- Escape in lenient mode (query): same behavior
- Mixed escaped + real vars in one string
- Escaped var that would be missing: no error in strict mode
- Existing tests continue to pass (no regression)

### Documentation
- `docs/env_vars.md` — document both the `${VAR}` substitution feature and the `$${VAR}` escape convention
