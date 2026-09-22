# Database ownership refactor — handoff

## Goal

Finish migrating SQL/database logic out of `client/library` (the old
`client-library` crate — a grab-bag of free functions taking `&rusqlite::Connection`/
`&Transaction`) into `client/library-backend/crates/library-database` (`library_database::Database`),
which owns its SQLite connection directly (`connection: rusqlite::Connection`,
no pool, no clone, no `Arc`, no `Mutex`).

Non-negotiable constraints established this session (do not relitigate these):

- **`Database` is the single owner of its connection.** No `.get()`-style
  accessor that hands out the raw `Connection`. No `Mutex<Connection>` "to
  satisfy the compiler." No `Arc<Database>` shared across owners.
- **One method does its own database work inline.** Don't fetch a
  connection/transaction and pass it into another function (in this crate or
  another) to do the actual work. It's fine for one method to open a
  transaction and use it directly; it must not hand that transaction to a
  helper.
- **No function/struct that must cross an `.await` holds `&Database` (or
  anything containing it).** `rusqlite::Connection` is `Send` but not `Sync`
  (statement-cache `RefCell`s), so `&Database` is `!Send`. Any `async fn` or
  struct-with-a-database-reference that spans a real `.await` will fail to
  compile as `Send` if invoked in a context that requires `Send` futures.
  The fix is **not** to make `Database` `Sync` — see the threading section
  below. The fix is: read a snapshot synchronously, do the actual async
  work (network, CPU, filesystem) with **no** database reference held, then
  commit a result synchronously in a second call. Even when nothing is
  awaited *while* touching the database, if the reference is a function
  *parameter* for the whole async fn, that's enough to poison `Send` — so
  don't thread `&Database` through long functions that also do network I/O;
  split them.
- **No test-only methods.** Black-box test through the real public API. If a
  test needs a method that exists only for that test, delete the test
  instead of adding the method.
- Prefer one `-- name:` SQL entry per query in a domain's `.sql` file over ad
  hoc `include_str!` + manual `query_row`, matching the style already used
  throughout `library-database`. (Some ported code still uses `include_str!`
  for large/one-off queries copied verbatim from the old crate — acceptable
  for now, not the target end-state.)

## The `Send` problem and how it's solved

`library-backend`'s per-library actor loop (`LibraryHandle`/`LibraryActor` in
`client/library-backend/src/library/handle.rs`) holds a `LibrarySession`
(containing `Database`) as persistent state across every iteration of its
`tokio::select!` loop — an event loop's captured state is alive across every
`.await` by construction, so it needs `Database: Sync` merely to be spawned
on a `Send`-requiring executor, no matter how short individual queries are.

**Solution implemented:** `client_platform_runtime::executor::LibraryExecutor`
(`client/platforms/runtime/src/executor/native.rs`, native-only — wasm is
already single-threaded and untouched). One dedicated OS thread per **open
library**, running a single-threaded Tokio `LocalSet`. Nothing scheduled on
it needs `Send`. Key API:

- `LibraryExecutor::new() -> Result<Self, BackendExecutorError>` — spawns the
  thread.
- `.run(job: impl FnOnce() + Send + 'static)` — hands `job` to the thread;
  `job` itself must be `Send` to cross over, but once running there it may
  call `tokio::task::spawn_local`/`LibraryExecutor::spawn_local(...)` freely.
- `.run_blocking(job) -> T` — same, but blocks the calling thread for a
  result. Used once, in `LibraryHandle::open`, to build the `LibrarySession`
  (including opening `Database`) entirely on the dedicated thread — the
  session must never even transiently exist on the caller's thread, because
  it holds `Arc<LibrarySync>`, and `LibrarySync` also owns a `Database`, so
  `Arc<LibrarySync>: !Send` too.
- `LibraryExecutor::spawn_local(future) -> BackendTask<T>` / `spawn_local_detached(future)`
  — associated functions (no `&self` needed), for use by code **already
  running on the thread** (inside a `run`/`run_blocking` job). This is how
  `spawn_actor`'s main loop and `start_sync`/`start_assets`/observers are
  spawned now.
- `LibraryHandle` gained a field `_library_executor: LibraryExecutor` (native
  only) purely to keep the thread alive for as long as any handle to that
  library exists.

Granularity matters: it's **one thread per open library**, not one shared
thread for the whole app — a shared thread would let a slow query in one
library stall every other open library, since none of this is preemptive.

`ThumbnailObserver` (`client/library-backend/src/thumbnail_jobs.rs`) had a
stale `: BackendSend` supertrait bound from the pool-executor era, which
broke because its real implementor (`ThumbnailEvents` in `handle.rs`) holds
`&LibrarySession`. Removed the bound — this trait's methods only ever run on
the dedicated thread now, so `Send` was never actually required.

**This same class of problem still exists, unsolved, in `LibrarySync`/
`asset_transfer`** — see "Known remaining work" below. `LibrarySync` also
needs to run on a `LibraryExecutor` (or equivalent) for its `!Send` errors to
go away, *or* the functions that currently take `&Database` and span a
network `.await` need to stop taking `&Database` as a parameter at all
(preferred — see `asset_transfer::planning` for the pattern already applied).

## What's fully migrated and compiling clean

Run `cargo check -p library-database` — it is 100% clean (5-6 pre-existing
warnings only, no errors). Everything below lives there:

- **Audiobook playback** (`src/audiobook.rs`): `audiobook_playback_snapshot`,
  `commit_audiobook_metadata`. Reading position (`src/reading.rs`):
  `reading_position`, `pdf_reading_position`, `update_reading_position`,
  etc. Both fully wired into `library-backend`'s `library/reading.rs` and
  `browser_audiobook.rs`.
- **Placements domain** (`src/placements.rs`, ~600 lines, mirrors the old
  `client/library/src/placements/mod.rs` almost 1:1): directory
  create/move/copy, book placement transfer/remove/restore, Trash
  lifecycle (trash/restore/purge for both books and directories),
  `folder_destinations`, `library_trash`. SQL lives in
  `client/library-backend/crates/library-database/sql/placements.sql` (DSL, `placements_*`-prefixed
  names to avoid collisions — **method names generated by `include_sql!` are
  private to the module that calls `include_sql!`**, so cross-module reuse
  needs its own duplicate query with its own name; you cannot call
  `connection.some_other_modules_method()`) plus
  `client/library-backend/crates/library-database/src/sql/placements/*.sql` (many small
  `include_str!`-based files copied verbatim from the old crate, not yet
  converted to the DSL).
- **Asset planning / transfer settlement** (`src/asset_planning.rs`) — this
  one *pre-existed* this session, more complete than expected; extended
  with `settle_rejected_asset`, `complete_retained_download`,
  `is_book_downloaded`, `evict_local_book`, `local_file_work_status`.
- Wired into `library-backend`: `management.rs` (fully rewritten, all
  Database-direct one-liners), `import_native.rs`, `jobs.rs` (thumbnail +
  file-work pending/run checks), `asset_workflow.rs`'s
  `complete_retained_download`/`evict_local_book`/`placement_work` (now take
  `&Database` instead of `&rusqlite::Connection`/`&mut Connection`; the
  transfer/remove/restore methods were deleted from `AssetPlacementWorkflow`
  entirely since `Database`'s new methods supersede them for native — **wasm
  parity gap, see below**).
- `library-files`: `AssetStore::evict_book_files` changed to take
  `relative_paths: &[String]` instead of `&Database` — the caller
  (`asset_workflow.rs`) reads paths from `Database::book_paths(hash)`
  itself and passes plain data down. This is the "don't pass database into a
  function that then goes and uses it" pattern applied to a case that had
  nothing to do with `Send`/await — it's a general principle, not just an
  async-safety one.
- `asset_transfer::planning` (`client/library-backend/crates/asset-transfer/src/planning.rs`)
  restructured: `TransferPlanner::negotiate_book_uploads`/
  `negotiate_thumbnail_availability` are now pure network calls taking plain
  data (no `&Database` parameter at all); `AssetPlan::read_page`/
  `inspect_page` replace the old combined `next_page` the same way. Callers
  (`LibrarySync` in `sync/asset_sync.rs`, `sync/transfer/executor.rs`) do
  their own `database.*` calls before/after. **This is the reference pattern
  for finishing `books.rs`/`thumbnails.rs` — see below.**
- `asset_sync.rs`: rewritten to the new `read_page`/`inspect_page` split;
  its ~250-line test module (which called internal implementation details
  like `AssetPlan::next_page` directly, and had a test-only `TestAssetPlan`
  trait) was **deleted**, per the no-test-only-code rule — those tests were
  structured around the old API shape and would have needed test-only
  scaffolding to keep alive.

## Known remaining work (roughly 74 compiler errors)

Run `cargo check -p library-backend` for the live list. Rough shape, largest
first:

1. **`sync/transfer/mod.rs` (13), `sync/transfer/executor.rs` (8) —
   `books.rs`/`thumbnails.rs` need the same treatment as
   `planning.rs`.** `BookTransfers::download_blob`/
   `upload_blob_version` (`client/library-backend/crates/asset-transfer/src/books.rs`)
   and the equivalent in `thumbnails.rs` take `&Database` as a parameter for
   their *entire* (long, multi-`.await`) body, interleaving many small sync
   `database.*` calls with network/streaming work throughout — not just at
   the start/end like `negotiate_book_uploads` was. Splitting this properly
   means turning `download_blob` into a state machine the caller
   (`LibrarySync`) drives between database touches: read what's needed
   synchronously, do one chunk of pure network/IO work, commit synchronously,
   repeat. This is real surgery on a checksum-verification/atomic-publish
   path — don't rush it, and don't paper over it with `&Database` threaded
   through by reference. `validate_account` in `books.rs` was already
   converted to take `&Database` (it's a single sync check, not a
   problem) — that part's done.

2. **`sync/library_sync.rs` (10), `sync/state_sync.rs` (8).**
   `state_sync.rs`'s `AccountStore` (used by the generic `sync_engine` crate
   via the `SyncStore` trait) calls `self.sync.database.command(Priority::Background, move |conn| {...})`
   — a `Database::command(priority, closure)` API that **does not exist**.
   This looks like it was sketched assuming a priority-queued command
   interface would be built into `Database`, but never was. Needs a design
   decision: does `Database` grow a `command()` method (and if so, what
   does "priority" even mean for a single-threaded, single-connection
   owner — probably just FIFO, since there's no contention to prioritize
   against), or does `AccountStore` get restructured to not need it (likely
   cleaner: have `AccountStore`'s methods take `&Database` and do their
   `sync_engine`-required reads/writes as ordinary synchronous
   `Database` methods, since `sync_engine::SyncStore`'s trait methods return
   boxed futures — check whether those need to be `Send`, same as
   `asset_transfer::TransferHost` does; if so, same "make it pure sync +
   don't span awaits" fix applies). `ensure_sync_account` was already fixed
   to take `&Database` instead of `&Connection` — the same fix needs
   applying everywhere else in this file that still says
   `client_library::sync_state::*(conn, ...)`.

3. **`browse.rs` (11).** Entirely unstarted. Wraps `client_library::browse::*`
   (`client/library/src/browse/{mod,folders,subject_compaction,subject_incremental}.rs`,
   ~1350 lines total) — book counts, home view, author index, folder/subject
   browsing with filters/sorting/grouping. This module itself pulls in
   `client_library::books`, `crate::storage_values`, `subject_projection`.
   Biggest un-migrated domain by line count. Per the "why do we need so many
   layers" conversation earlier this session: the target shape is
   `LibrarySession::book_count()` etc. becoming one-line delegates to new
   `Database` methods — collapse the `client_library::browse` free-function
   layer away entirely for this domain, don't just fix the connection
   plumbing around it.

4. **`operations.rs` (8).** The import pipeline (`commit_import`) is one
   `rusqlite::Transaction` spanning 8+ domains at once (scanner, metadata,
   audiobooks, toc, pdf_metadata, import_inspection, thumbnail_jobs, assets)
   assembled ad hoc by the caller from a borrowed connection — the exact
   "pass a connection into unrelated functions" anti-pattern, at its most
   tangled. This needs a single cohesive `Database::commit_import(...)`
   method (or a small number of them) that does the whole transaction
   internally, rather than `LibrarySession` orchestrating it from outside.
   Not started. `validate_import_destination`/`persistent_download_state`
   in this file also need trivial `.get()` fixes once the bigger piece is
   sorted.

5. **`enrichment/application.rs` (3), `enrichment/editions.rs` (2),
   `enrichment/scheduling.rs` (1)** — not investigated this session.
   `scheduling.rs`'s error is a type mismatch (`AuthorEnrichmentCandidate`
   defined twice, in two crates — same shape as the `BookPlacement`/
   `BookUploadIntent` duplicate-type issue that was already resolved
   elsewhere by switching imports to the `library_replica`/
   `library_database` canonical version. Check for an existing canonical
   definition before porting a new one).

6. **Small/quick, not yet done:** `transfer_status.rs` (2 — composite read
   combining sync_state + assets + `client_library::operations::status`;
   needs a cohesive `Database` snapshot method), `sync/mod.rs` (3),
   `sync/transfer/coordination.rs` (1), `sync/scheduler.rs` (1),
   `library/mod.rs` (1), `handle.rs` (2 — just `download_state`'s
   `self.db.get()`, trivial once `persistent_download_state` in
   `operations.rs` is fixed since it calls into that).

7. **Known gap, not a compile error:** `management.rs::restore_book` and
   the deleted `AssetPlacementWorkflow::restore_book`/`restore_directory`
   dropped **wasm-only** OPFS-availability marking
   (`restore_opfs_availability` in the old `asset_workflow.rs` — marked a
   restored book's hash-addressed bytes as downloaded/available, requested a
   missing thumbnail). This was never re-plumbed into the new
   `Database::restore_book`/`restore_directory`. Nothing catches this at
   compile time on native. Needs attention before a wasm build would be
   correct, but wasm wasn't checked or built at all this session (no
   `cargo check --target wasm32-unknown-unknown` was run) — treat wasm
   correctness generally as unverified beyond "the type signatures allow
   both targets to compile," not as "confirmed working."

## Practical notes for continuing

- `cargo check -p library-database` and `cargo check -p library-backend` are
  the two commands to live in. `cargo check -p asset-transfer` and
  `-p library-files` for the two supporting crates touched this session.
- When adding a query, the naming convention across files is
  `<domain>_<verb>` (e.g. `transfer_*`, `native_*`, `placements_*`,
  `audiobook_*`, `owner_*`) — **names must be unique across the whole
  crate**, not just the file, because every `include_sql!` call generates
  `impl <PrivateTraitName> for rusqlite::Connection`, and two traits with
  the same method name both in scope via inherent-method-style calls
  (`connection.foo()`) would collide if a caller ever imported both — in
  practice this hasn't bitten because each trait is private to its own
  module, but duplicate names across files are still a smell and this
  crate's existing convention avoids them.
- `-- name: foo?` = query returning rows via callback, `-- name: foo!` =
  execute (returns row count), `-- name: foo ->` = scalar `query_row`. See
  any existing `.sql` file for exact syntax; `client/library-backend/crates/library-database/src/query_methods.rs`
  has the macro if the DSL's exact behavior is ever in question.
- Don't add a `Mutex`/`Arc` to get past a compile error without first
  checking whether the actual problem is "this needs `Send`" (fixable via
  `LibraryExecutor`/splitting sync-vs-async work) versus "multiple owners
  genuinely need concurrent access" (rare — so far only came up as a
  hypothesis for `LibrarySync`'s transfer runner, and even there the
  `asset_transfer::planning` split turned out to make it moot).
