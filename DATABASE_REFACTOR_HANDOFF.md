# Library database refactor handoff

## Goal

One concrete `library-database` crate owns each library's SQLite connection and
all database work. `library-files` must become physical filesystem/object-store
access only. Filesystem traversal, hashing, and network work must not retain a
database connection or transaction.

## Completed

- Added `client/library-backend/crates/library-database` to the workspace.
- Moved the concrete owner from `library-runtime` to
  `client/library-backend/crates/library-database/src/owner.rs`.
- `Database` now holds exactly one checked-out-at-a-time SQLite connection;
  the previous idle-connection pool was removed.
- `library-runtime` no longer owns a database module or depends on
  `device-sqlite`.
- Scanner directory walking and hashing do not retain a SQLite checkout. Its
  remaining database ownership is the next boundary to remove: scanner methods
  must return typed discovery snapshots/results to the session owner, which
  orchestrates database snapshots, acknowledgements, retries, and follow-up
  work.
- Added database-owned book facts used by normal local reading:
  book paths, local-storage paths, local PDF metadata, and local book
  info.
- Native normal book opening now selects its path/metadata through the
  database and asks AssetStore only to open that explicit physical path.
- Remote-library creation is a typed database decision
  (`RemoteLibraryCreation`), not caller-supplied metadata keys.
- Browser transfer history has a dedicated `local_transfer_history` table;
  it no longer uses the `local_transfer_history_v1` metadata key.
- Asset-transfer book uploads and batch uploads now receive the database
  owner's book-path snapshot and hand those paths to `AssetStore` for
  verification. They no longer use AssetStore's legacy hash-to-path SQLite
  reader.
- Native `local_file_work` replay now uses typed `library-database` snapshots
  and conditional acknowledgements. File preparation and book no longer
  retain a SQLite checkout. Directory projection preparation is still the next
  native-placement operation to split.
- Native import destination preparation now uses a typed directory snapshot,
  connection-free filesystem creation/move, and conditional database
  acknowledgement. Queued directory and book projection work still need to
  compose that primitive instead of using their legacy connection-held loop.
- Removed the transitional `library-files::database::FileDatabase` trait and
  backend adapter entirely. Thumbnail recovery now receives the concrete
  `library_database::Database` owner; scanner ownership is still being
  removed as described below.
- Asset planning now uses typed database snapshots and conditional settlement;
  its account-change regression tests that exact owner API. Requested-download
  resolution and locally-satisfied request completion are also typed database
  operations, not connection closures.
- Thumbnail worker paging, book-format lookup, and every thumbnail
  queue transition are named `library-database` operations.
- `Database` is no longer `Clone` and no longer wraps its connection in an
  `Arc`; one owner retains one checked-out-at-a-time connection.
- Removed `Database::command`, `#[track_caller]`, and all SQLite
  caller-location plumbing.
- Asset transfer helpers no longer store a database reference. Each planning,
  book, or thumbnail operation receives the database explicitly at the
  call that needs its typed operation.

## Verified

These optimized checks passed after the changes:

```text
cargo check --release -p library-database
cargo check --release -p library-runtime -p library-database
cargo check --release -p library-backend
cargo check --release -p library-files -p library-backend -p asset-transfer
cargo test --release -p library-files --lib
```

Warnings remain pre-existing/unrelated to this refactor.

## Remaining boundary to remove

The remaining boundary is direct `Database::get()` use outside
`library-database`, chiefly in asset-transfer book execution, backend
transfer coordination, library synchronization, scanner, and the legacy
queued native directory projection. Replace each with a named snapshot or
conditional commit before making connection checkout internal. `command` has
already been removed and must not be reintroduced.

The problematic implementation is native placement reconciliation:
`NativePlacement::reconcile_file_work` and `reconcile_file_job` combine SQL
selection/transactions with filesystem preparation and book. Do not move
that code wholesale into the database crate. Split each operation into:

1. database snapshot/claim (short transaction),
2. physical prepare/perform through `library-files`, with no connection held,
3. database conditional commit/acknowledgement (short transaction).

The same shape is needed for queued directory/book projection, purge requests,
and transfer uploads/downloads. Asset transfer should receive typed database
snapshots and submit typed conditional completions; it must not use `get()` or
`command()`.

## Important constraints

- Do not add arbitrary key/value wrapper methods to `Database`.
- Do not expose `rusqlite::Connection`, generic SQL closures, a database
  locator, cloning, or a shared database handle outside `library-database` in
  the final design.
- Do not remove AssetStore's current database locator until asset-transfer's
  legacy hash-to-path reader has been changed to use database-supplied paths;
  doing so breaks uploads.
- Preserve existing user worktree changes; it is intentionally very dirty.
- Use release Cargo commands only.
