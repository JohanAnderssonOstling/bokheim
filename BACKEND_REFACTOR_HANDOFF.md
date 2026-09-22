# Backend refactor handoff

## Latest simplification: local replies and direct library clients

- `LibraryCommand` is the sole library request enum. The duplicate browser
  enum/dispatcher, `HostLibraryCommand`, and identity-only `LibraryRequest`
  trait are removed.
- Shared handlers return `library_runtime::reply::Reply`, retaining the Rust
  value and its encoder. Native clients take the value without serialization;
  only worker dispatch converts it to `WorkerPayload`. App settings retain
  receive-order execution.
- Native `LibraryClient` retains its specific `LibraryHandle`. Requests,
  subscriptions, imports, and book opening bypass the app worker. Retirement
  closes that actor's command channel; old clients cannot attach to a newly
  opened generation. The actor, not client handles, owns its executor lifetime.
- Directory import futures run beside actor commands, retaining in-flight work
  when the caller cancels, checking cancellation before the next file, and
  isolating source panics. Scans, synchronization, and transfers remain separate.
- `asset_transfer::TransferError` is the single transfer error type and retains
  its underlying source. The unused runtime duplicate and `ThumbnailJobError`
  wrapper are removed. Persisted/displayed diagnostics remain strings.
- `LibraryDirectory` directly owns its handle map and lifecycle lock. The
  single-consumer `SessionDirectory<H>` abstraction is removed.
- Opening configuration, activity state, thumbnail coordination, and worker
  ownership now live in `library-backend/src/library`, not `library-runtime`.
  Shared scheduling, account capabilities, events, transfer primitives, and
  reply/wire infrastructure remain in the runtime crate.

The browser-worker package is a workspace member so its entry points and
test-export feature can be checked with the backend on wasm32. Worker protocol
version 2 requires rebuilding the page and worker bundles together.

The older sections below describe the extraction history, not additional
compatibility layers that should be restored.

## Goal

Separate device-global application concerns from per-library concerns so the
library backend can become its own crate.

The intended ownership boundary is:

```text
app crate
  account session, device registry, application settings, startup
  └─ opens and retains library handles

library-backend crate
  LibrarySession / LibraryHandle
  database, asset store, metadata client
  scanner, enrichment, sync, transfers, book access
  library events and library commands
```

The UI owns the active-library selection. The app must not own an
active-library variable or aggregate presentation state for library work.

## Current state

`client/library-backend/crates/library-runtime` exists and now owns reusable per-library runtime
primitives, including database-pool infrastructure, events, scheduling,
workers, session-directory support, account bridge types, and transfer
history support.

The application has been reduced substantially: it primarily retains the
device registry, account session, startup/opening policy, account maintenance,
and library-list notification state. Libraries are opened eagerly on startup.

The resource-bearing library backend now lives in `client/library-backend`:

- `library/`: `LibrarySession`, `LibraryHandle`, commands, scanner,
  enrichment, imports, book access, and library event plumbing.
- `sync/`: `LibrarySync`, asset planning, transfer worker, state
  synchronization, and transfer coordination.

`library-runtime` remains the shared primitive crate. `app` owns
application registry/account/startup policy and depends on `library-backend`;
the library crate does not depend on the application backend.

## Session log: wire split, storage seams, wasm repair

Three things changed since the previous handoff.

**Host-worker wire primitives now live in `library_runtime::wire`.** `WorkerPayload`,
`encode_worker_message`/`decode_worker_message`, `encode_reply`,
`decode_worker_payload`, and `ThumbnailResolution` moved out of
`app/src/runtime/protocol.rs`. The encoded-reply shape belongs to neither the
app nor the library, and `library/` previously had to reach into `crate::runtime`
to answer a request. `runtime/protocol.rs` re-exports them, so the app-side
protocol types (`AppStartupState`, `WorkerCommand`, …) are unaffected.

**The storage compatibility seams are gone** (handoff step 5):

- `app/src/storage/database.rs` and `storage/transfer_history.rs` were pure
  re-exports of `library_runtime`; both are deleted and callers now name
  `library_runtime::database` / `library_runtime::storage_queue` directly.
- The `DbPool` alias is removed in favour of `Database`.
- `src/storage/sql/database/` held 12 orphaned SQL companions for tests that had
  already moved to library-runtime; deleted.
- `storage/` had shrunk to one real file, so the asset-store host adapter is now
  `app/src/asset_store.rs` and the `storage` module is gone.
- The browser transfer-history contract moved to
  `contracts/transfer_history_contract.rs`, where the other host contracts live.

**wasm32 was already broken and is now repaired.** The previous handoff
verified only the native workspace. `cargo check --target wasm32-unknown-unknown`
failed with **28 errors at the last commit**. Fixed here:

- `library/discovery.rs` gated the scanner on `feature = "scanner"` while
  `library-files` gates it on `all(feature = "scanner", not(wasm32))`. The gates
  now match.
- `ResolveBook` / `ImportDirectory` / `ImportBookPath` are native-only actor
  commands. Browsers open books through their page-side adapter, and the
  resources they produce (`BrowserAudiobook` holds a JS closure) are not `Send`,
  so they can never cross the actor channel. This was also the cause of the
  `observe_writes` `Send` failure in `handle.rs`.
- `sync/transfer/coordination.rs` needed the same
  `Database::get().map_err(TransferError::retryable)` treatment the previous
  session applied to `authentication.rs` and `executor.rs`.
- `runtime/web.rs::library_request` dropped the inner worker-side error.
- `browser_audiobook.rs` now uses the encapsulated `db.book_format()`.
- `browser_adapter/requests.rs` captured a session borrow in a `'static`
  storage command; it carries the library id by value instead.

**Book subscriptions now retain their browser-only lease inside the
library actor.** `BrowserAppCommand::SubscribeBook` asks the
`LibraryHandle` to subscribe instead of borrowing a session from the directory.
The actor prepares and encodes the resolved book, retains its non-`Send`
`BookLease` in an actor-local map, and returns a `Send` cancellation
guard. Dropping the worker subscription sends a release command back to the
actor. This preserves the page lifetime semantics without letting a JS-bound
reader cross an actor channel.

## Extraction result

The coherent extraction is complete. `asset_store`, `worker_cpu`, import
journaling/workflow/status/staging, library resources, and synchronization
moved with the library ownership graph. Browser book source types now
live in `library-backend`, and browser-specific library commands are direct
`LibraryCommand` variants rather than the former app-host command escape hatch.

`app` retains direct compatibility module exports for downstream
clients during the package transition, but those exports are direct exports of
`library-backend`; there is no duplicated implementation or reverse dependency.

## Historical remaining work

0. ~~Finish the wasm32 repair~~ (**complete**). Both targets now gate the crate
   extraction. Do not move `library/` and `sync/` into a new crate while they do
   not compile on one of their two targets.

1. Create a real `client/library-backend` crate.

   Move `client/app/src/library` and `client/app/src/sync` into it as
   one coherent unit. Do not continue moving isolated helper files first: the
   session, sync manager, transfer worker, scanner, and library command path
   form one ownership graph.

   The boundary survey, so it does not need re-deriving: `library/` + `sync/` is
   about 8 800 of the crate's 16 400 lines, and roughly 60 references point back
   the other way. Most of what they name as `crate::X` are re-exports of
   *external* crates, so the new crate's `lib.rs` can re-export the same names
   and the moved files need no edits: `APP_HIDDEN_DIR`/`APP_HIDDEN_LIBRARY_DB_PATH`
   (`library_files`), `ContentHash`/`DirId`/`LibraryId`/`ROOT_DIR_ID`
   (`sync_common`), `BlobKind`/`TransferJobKind`/`ScanProgress` (`library_replica`),
   `LibrarySyncState`/`LibrarySyncStatus`/`BookPlacement`/`RelativeBookPath`
   (`client_library`), `executor` (`client_platform_runtime`),
   `notification_interest` and `ThumbnailResolution` (`library_runtime`).

   Genuinely move with it: `asset_store.rs`, `worker_cpu`, `import_workflow`,
   `import_journal`, `import_status`, `staged_import`.

   `BackendError` is shared with the app; put it in `library-runtime` and
   re-export from both. Its `From` impls already only name types available there.

   Four real blockers remain, all of them host-bridge coupling rather than
   domain coupling:

   - `crate::runtime::HostLibraryCommand` in `library/commands.rs` (the
     target-selected browser/native host command).
   - `crate::runtime::{BookSource, ResolvedBookData}` in
     `library/book/web.rs`. Both types describe a resolved book and are
     library-owned; they should move rather than be re-exported.
   - `crate::browser_adapter` in `sync/scheduler/web.rs` and
     `sync/transfer/coordination.rs`, plus `crate::worker_sync` in
     `sync/library_sync/web.rs` — all wasm-only.
   - The `SubscribeBook` session-access problem described above.

   The first two are small. The wasm-only ones need the dependency inverted:
   the library crate should take a host capability, not name `browser_adapter`.

2. Define a small app-to-library interface.

   The new crate should expose a `LibraryBackend`/`LibraryHandle` constructor,
   library commands, and library event subscriptions. The app should open
   libraries from registry entries and retain handles only; it should not
   inspect a session's database, sync state, or workers.

3. Make UI library commands direct.

   `runtime/requests.rs` still routes library work through `AppBackend`.
   Replace that with a library client/handle held by the UI's per-library
   session. Retain `AppClient` only for account, registry, startup, and
   app-global preferences.

4. Finish database encapsulation.

   `Database::get()` and `DatabaseConnection` remain widely used by production
   library and sync code. Continue replacing those with concrete database or
   domain methods. Raw connections should be confined to database internals,
   transaction-heavy storage implementation, and tests.

5. Remove compatibility seams.

   Delete backend aliases/re-exports and app/library bridge code once callers
   use the new crate. In particular, remove the remaining `DbPool` alias and
   old runtime routing assumptions instead of preserving them permanently.

## Account and lifecycle policy

Account identity is global and remains app-owned. Each open library should
receive the shared account capability and react to account changes itself; it
owns its durable sync work and retry/wake lifecycle. The app should own only
which local libraries exist, their device registry records, and account-global
maintenance.

## Verification performed

Both targets must be checked. The native workspace alone is not a gate — it was
green for the whole period in which wasm32 accumulated 28 errors.

```bash
cargo check --release --workspace                                   # 0 errors
cargo check --release --target wasm32-unknown-unknown -p app  # 0 errors
```

The desktop application was also rebuilt and launched with:

```bash
cargo build --release -p desktop-gpui --bin desktop-gpui
./target/release/desktop-gpui
```

The launched process reported existing library scans pending and account
discovery retries caused by HTTP 404 responses; those are runtime/service
configuration warnings, not compilation failures.

## Important constraints

- Use release commands only (`--release`).
- Prefer the end-state ownership model over transitional bridge layers.
- Do not reintroduce app-global active-library state or aggregate library-work
  presentation state.
- Preserve durable library work across app exit and restart.
