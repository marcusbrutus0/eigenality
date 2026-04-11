# Env vars that appear in doc code examples — set to their own ${NAME} so
# eigen's interpolation is a no-op and the examples render correctly.
_doc_env := "NOTION_DB_ID='${NOTION_DB_ID}' NOTION_API_KEY='${NOTION_API_KEY}' ENV_VAR='${ENV_VAR}' API_TOKEN='${API_TOKEN}' WORKSPACE_ID='${WORKSPACE_ID}' CMS_TOKEN='${CMS_TOKEN}' STRAPI_TOKEN='${STRAPI_TOKEN}'"

# Build the website
website-build:
    {{_doc_env}} cargo run -- build -p website -v

# Start dev server for the website
website-dev:
    {{_doc_env}} cargo run -- dev -p website -v

# Run audit on the website
website-audit:
    cargo run -- audit -p website

# Run all tests
test:
    cargo test

# Run clippy
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt
