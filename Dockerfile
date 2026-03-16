FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /site
ENTRYPOINT ["eigen"]
CMD ["build"]

# CI: use pre-built binary from context (docker build --target release)
FROM runtime AS release
COPY eigen /usr/local/bin/eigen

# Local: build from source (docker build .)
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --profile dist

FROM runtime
COPY --from=builder /app/target/dist/eigen /usr/local/bin/eigen
