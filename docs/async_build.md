# Async Build Pipeline

Eigen's build pipeline runs asynchronously on the tokio runtime, enabling
concurrent page rendering and overlapping HTTP requests.

## How It Works

Pages are rendered concurrently using `futures::stream::buffer_unordered`
with a concurrency limit of `available_parallelism * 2`. Shared state
(data cache, asset cache, template engine) is wrapped in
`Arc<tokio::sync::Mutex<T>>` for safe concurrent access.

## Concurrency Model

- **HTTP requests** overlap across pages — while one page waits for a
  response, others can render or fetch.
- **Template rendering** is serialized via a mutex on
  `minijinja::Environment` (it's `Send` but not `Sync`). Each render
  is fast (microseconds), so this is not a bottleneck.
- **File writes** use `tokio::fs` for non-blocking I/O.
- **CPU-bound work** (minification, CSS parsing, image optimization)
  runs synchronously within async tasks — tokio handles scheduling.

## Dev Server

The dev server runs builds natively on the tokio runtime. No
`spawn_blocking` or thread bridging is needed. Dev rebuilds are
sequential (single task) since they're fast enough for interactive use.

## Configuration

No new configuration is needed. The concurrency limit is automatic.
