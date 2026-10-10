# Book access

Revision-pinned range reads and bounded caches for book bytes. Applications
supply URLs and credentials for HTTP operations, or authenticated fetch callbacks
for seekable readers. This crate has no backend or account-client dependency. Timers use the shared
`client-platform-runtime` executor.

- `async_source.rs`: independent buffered async cursors for EPUB preparation
  and lazy images. Native HTTP uses the library's existing Tokio runtime;
  local files use positional reads on its blocking pool. Browser requests run
  on their owning event loop and return bytes through asynchronous channels.
  Each cursor owns its cache and read-ahead state. Dropping or seeking a cursor
  cancels its pending remote request.
- `remote_file/`: HTTP range validation, native and browser reader bridges, and
  native audiobook pause/seek/retry control. Prefix and subsequent HTTP reads
  share revision and response validation and reuse the library HTTP client.
  The audiobook worker owns retries and reports failures through channels;
  seek, pause, and cancel changes wake it without polling or a condition variable.
  Legacy synchronous native readers adapt that same futures I/O reader to Tokio
  through `tokio-util::compat` and `SyncIoBridge`, borrowing the
  application’s runtime. Constructors run in that runtime, which must remain
  alive for the readers and audiobook workers. Synchronous reads run on parser threads outside Tokio's
  async execution context; the cache and read-ahead policy is shared with the
  browser and audiobook adapters.
- `range_reader.rs`: one async cursor implementation for EPUB, native and
  browser parser adapters, and audiobook streaming. Position, pending downloads,
  cache, and read-ahead policy have no HTTP or format knowledge.
- `range_cache.rs` (internal): bounded in-memory fragments for one immutable revision,
  with shared block insertion, response length validation, and seek arithmetic.
- `pdf_bundle.rs` (`pdf` feature): shared page-warmer/parser cache. Page-aware
  batches retain their own policy and reuse the common cache operations.

Source and reader constructors return `io::Result`; invalid supplied prefixes
are rejected once at source creation. An empty async/browser prefix means that
no bytes have been prefetched.

Native HTTP/authentication callbacks retain `RemoteFileError`, including inside
I/O errors, until an application or browser transport boundary needs text.

Validate with `cargo test --release -p book-access --all-features --lib`.
