# Environment Variable Substitution

Eigen replaces `${VAR_NAME}` patterns with environment variable values in
three contexts:

- **`site.toml`** — site configuration
- **`_data/` files** — global YAML and JSON data
- **Data query fields** — query paths, filters, and request bodies

## Basic usage

```toml
# site.toml
[site]
name = "My Site"
base_url = "${BASE_URL}"
```

```yaml
# _data/secrets.yaml
api_key: "${API_TOKEN}"
```

## Strict vs lenient mode

| Context | Missing variable behavior |
|---|---|
| `site.toml` | **Error** — build fails with message listing missing variables |
| `_data/` files | **Error** — same as above |
| Data query fields | **Silent** — `${MISSING}` is left as literal text |

## Escaping: literal `${...}` in content

To include a literal `${VAR_NAME}` in your output (e.g., in documentation or
code examples), double the dollar sign:

```
$${SOME_VALUE}
```

This renders as `${SOME_VALUE}` in the output. The doubled `$$` tells eigen to
skip substitution and produce the literal text.

### Examples

| Input | Output |
|---|---|
| `${HOME}` | Value of HOME env var |
| `$${HOME}` | `${HOME}` (literal text) |
| `$${MISSING}` | `${MISSING}` (no error, even in strict mode) |
| `Use $${API_KEY} at ${HOST}` | `Use ${API_KEY} at example.com` |

### When to use escaping

- Documentation pages showing environment variable examples
- Code snippets that include shell syntax
- Template examples meant to be read, not evaluated

## Variable naming rules

Variable names must match `[A-Za-z_][A-Za-z0-9_]*`:

- Start with a letter or underscore
- Followed by letters, digits, or underscores
- Examples: `HOME`, `API_TOKEN`, `_INTERNAL`, `my_var_2`
