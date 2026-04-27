# Sync design system CSS + fonts from sibling repo
sync-design:
    rm -rf website/static/css/wavefunk
    cp -r ../design/css website/static/css/wavefunk

# Build the website
website-build: sync-design
    cargo run -- build -p website -v

# Start dev server for the website
website-dev: sync-design
    cargo run -- dev -p website -v

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
