# Library files

Owns publication bytes and their physical placement:

- `asset_store`: native files and browser OPFS, verified staging, leases,
  publication and thumbnail access, placement and eviction.
- `scanner` (native, feature `scanner`): discovery, metadata inspection,
  hash workers, pending-work protection, and deletion grace periods.
- `watcher`: filesystem notifications that wake the scanner.
- `thumbnail_jobs`: recovery, worker dispatch, leases, retry handling, and
  publication of both thumbnail derivatives.

SQL stays in each feature's `sql/` directory. The crate uses `client-library`
for library persistence and `publication-access` for reader contracts.

## Host interfaces

The application implements `database::FileDatabase` to provide connections and
prioritized queued writes. Connections must use the library schema's connection
configuration. `events::ScanEventSink` forwards native scanner notifications
to the application's event loop.

On the browser, `hashing::HashService` provides incremental hashing sessions.
Backend implements this through its CPU worker, including session cleanup when
an interrupted operation drops its hasher.

`thumbnail_jobs::ThumbnailProcessor` supplies cover decoding and extraction;
backend preserves its native PDF worker and browser CPU-worker RPC.
`ThumbnailObserver` receives batch progress and ready notifications. Its activity
guard spans the pass and drops on cancellation. Thumbnail job queries and
state transitions belong to `client-library::thumbnail_jobs`.

Library registration, lifecycle, relocation, account state, and UI events remain
application responsibilities. This crate does not depend on backend.

## Validation

```sh
cargo test --release -p library-files --all-features --lib
cargo check --release -p bokheim-backend --target wasm32-unknown-unknown --no-default-features
python3 scripts/checks/library-sql.py
python3 scripts/checks/backend-boundaries.py
```
