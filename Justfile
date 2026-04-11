# Build the website
website-build:
    cargo run -- build -p website -v

# Start dev server for the website
website-dev:
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
