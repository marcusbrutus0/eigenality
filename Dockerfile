FROM rust:1-bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --profile dist

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/dist/eigen /usr/local/bin/eigen

WORKDIR /site

ENTRYPOINT ["eigen"]
CMD ["build"]
