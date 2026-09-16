# Backend duplication audit

Implementation completed: 2026-09-15.

- Audiobook metadata reads and narrator decoding now use `client-library::audiobooks`.
- Live destination validation and occupied filename queries now live in `client-library::placements`.
- Folder view assembly now lives in `client-library::browse::folders`.
- Purge-work queries and database write notifications now live in `client-library::file_work`.
- Transfer retry orchestration is shared in `src/sync/transfer/authentication.rs`, preserving planned/queued credential behavior.
- Native and browser recovery use `src/sync/recovery.rs`; the transactional recovery initializer is shared by `client-library`.
- Removed 15 SQL copies after verifying their include references. One test now calls the shared filename query.
- Fixed a missing `rusqlite::Connection` import in an existing author-cache test that blocked library test compilation.

## Follow-up consolidation

The follow-up findings are also implemented:

- `client-library::placements::live_file_name` serves backend placement operations, library placement operations, and the scanner. Missing rows retain the existing `QueryReturnedNoRows` behavior; callers that allow absence still use `optional()`.
- `client-library::file_work::cancel_book_restore` serves both local eviction and permanent purge, within their existing transactions.
- Grouped and ungrouped folder views share the internal `folder_contents` function. Grouping remains inside its existing read transaction.
- Browser cached playback reuses title, author, chapters, duration, and narrator. Title/author fallback is queried only without cached metadata; narrator is refreshed after uncached inspection commits.
- Four additional SQL copies were removed after verifying include references.

Follow-up verification: all seven edited Rust files pass parsing, all literal includes resolve, and shared queries match the removed SQL exactly. Release backend/library/scanner tests and the WASM check were attempted but blocked before compilation by an unrelated `subject-projection` build failure: duplicate authoritative taxonomy paths under `Literature / Germanic & Nordic` for Denmark (3869, 4174), Netherlands (3899, 4209), Norway (4211, 4394), and Sweden (4230, 4473). Taxonomy data was not changed. The passing test results below belong to the previous consolidation, not this follow-up.

## Verification of the implementation

All compilation and tests used optimized release profiles.

- Native backend test compilation and WebAssembly backend check passed (including `web-runtime-tests` and `storage-contract-tests`).
- 104 selected backend tests passed: import/placement/file-job behavior, storage ownership/write notifications, account-safe recovery, and transfer execution. One manual benchmark was ignored.
- 14 selected library tests passed: playback cache metadata, downloaded folder grouping, placement collisions/copies, file work, and recovery-related behavior. Nine manual benchmarks were ignored.
- The browser sync regression passed with freshly built release storage and coordinator workers: multi-page recovery with responsive storage, concurrent edits/exact acknowledgements, transient retry, storage restart, owner-tab closure, shared downloads, and stale-account response rejection. Recovery completed in 1133 ms; the slowest interactive storage probe was 309 ms in this run.
- Every literal SQL/include target in the three edited source trees resolves.

The initial sandboxed transfer run could not bind localhost; all 38 transfer tests passed when rerun with localhost access. The new playback fixture was corrected to include the live placement required by `book_cards`; the corrected regression passes. The browser test reused the previously verified release network/CPU assets and rebuilt the storage/coordinator workers affected by this change.

The findings below record the pre-change source review; line numbers refer to that snapshot.

Source review: 2026-09-15. Scope: backend sync, storage, and library code against
`sync-engine`, `asset-transfer`, `client-library`, `library-files`,
`publication-access`, and platform crates. The review findings below motivated the completed changes above.

## Conclusion

The large subsystems mostly have a single implementation already. Backend owns
application coordination and supplies adapters. The remaining work is smaller:
consolidate repeated metadata queries, share recovery orchestration, and finish
moving domain decisions out of browser-specific paths. Platform-independent
application logic can stay in backend; it does not need to move into a platform crate.

## Confirmed active duplication

### 1. Audiobook metadata decoding — cross-crate, recommended first

- `src/browser_audiobook.rs:55` reads title, author, duration and chapters directly;
  line 77 reads recording evidence and extracts the first narrator.
- `../library/src/audiobooks/mod.rs:140` implements `cached_playback_metadata`,
  including chapter JSON decoding and the same recording-evidence query and narrator extraction.
- The browser's `sql/browser_audiobook/prepare_select_3.sql` and the library's
  `audiobooks/sql/cached_recording_evidence.sql` contain the same query.

Owner: `client-library::audiobooks`. Share metadata reads/decoding there, extending
its API if necessary to support a title/author when audio metadata is absent.
Keep file leases, Blob URLs and remote-source selection in the browser adapter.
Do not blindly replace the whole browser preparation function: it also inspects
uncached audio and requires a positive cached duration before skipping inspection.

Validation for a change: cached and uncached audio, malformed chapter JSON,
missing recording evidence, zero duration, and remote playback without local bytes.

### 2. Occupied destination filenames — cross-crate

The same query and collection of live placement filenames appears in:

- `src/library/operations.rs:204`, `import_book_reader_direct`.
- `../library/src/placements/mod.rs:70` and the directory-copy path near line 425.
- `../library-files/src/asset_store/mod.rs:509` and
  `../library-files/src/asset_store/native_placement.rs:68`.

Owner: `client-library::placements`. Expose one function for live names in a
destination and reuse it. Filesystem collision checks and physical copying still
belong to `library-files`; database occupancy alone cannot replace those checks.

Validation: collisions, deleted placements, folder copies, and import/reconciliation
races. Preserve each caller's transaction boundary.

### 3. Account refresh and retry — repetition within backend

- `src/sync/transfer/executor.rs:7`, `:31`, and `:74` repeat the sequence:
  capture credentials/account, attempt work, refresh on authentication failure,
  verify the same user, and retry once.
- `src/sync/transfer/mod.rs:93` has the same policy around batch execution.

Owner: backend account/session integration. Consolidate a narrowly scoped helper
for transfer authentication retries. `asset-transfer` should continue receiving
its host capabilities rather than depending on the application's account session.
Preserve differences in missing credentials, empty input, and error conversion.

Validation: expired tokens, concurrent refresh, sign-out/account switch while
awaiting refresh, and no second refresh/retry after another unauthorized response.

### 4. Cursor recovery loop — repetition across native/browser adapters

- `src/sync/state_sync.rs:88`, `AccountStore::complete_cursor_recovery`.
- `src/worker_sync.rs:73`, `StorageCommand::CompleteCursorRecovery`.

Both iterate recovery pages, accumulate inserted cells, and advance the cursor.
Native explicitly yields between pages; browser schedules each page as a storage command. Native also spells out beginning recovery where browser
calls `client_library::sync_state::begin_cursor_recovery`.

Owner: shared backend orchestration for the asynchronous loop; `client-library`
for the transactional state changes. Supply the page operation through the
existing storage adapters. Keep account validation inside the appropriate write
transaction and preserve browser command serialization. This is a targeted
consolidation, not a reason to rewrite the already-shared sync engine.

Validation: interruption/resume, account rebinding, atomic acknowledgement and
pull-page commit, and both native and browser recovery contracts.

## Boundary issues rather than duplicate implementations

- `src/library/browse.rs:46` and `:58` assemble folder results and apply
  `scope_browse_children` (`:117`). Most other browsing delegates to
  `client-library::browse`. Move the complete folder query/filter/sort operation
  there so domain browsing behavior has one owner. The source review does not
  establish an identical second implementation of these complete operations.
- `src/library/operations.rs:25` validates import destinations with backend-owned
  SQL. That validation is library-domain policy and belongs behind a
  `client-library` function. Keep validation before staging and again at commit.
- `src/storage/database.rs:37` classifies writes by library table names. Connection
  pooling is separate from this domain knowledge. Consider moving classification
  beside the durable queues if it changes again; no second classifier was
  established in the reviewed platform SQLite modules.

## Existing separation to preserve

| Area | Evidence | Assessment |
| --- | --- | --- |
| Metadata sync protocol | `src/sync/library_sync.rs:285` constructs `sync_engine::SyncEngine`; `src/worker_sync.rs` implements its storage interface | Shared protocol engine, platform-specific storage delivery |
| Transfer scheduling | `src/sync/transfer/mod.rs:15`, `:140` re-export shared jobs/queue and invoke the runner; `:144` implements `TransferHost` | Backend supplies account and application events; runner/queue remain in `asset-transfer` |
| Transfer planning | `src/sync/asset_sync.rs:17` uses `asset_transfer::planning::AssetPlan` | Application reconciliation around a shared planner |
| Asset storage | `src/storage/files.rs` re-exports `library_files::asset_store` and implements `FileDatabase`/worker hashing adapters | No second asset-store implementation here |
| Publication cache | `src/storage/publication_cache.rs:10` aliases the shared cache and implements `CacheStorage` | Adapter, not a duplicate cache |
| Scanning | `src/library/discovery.rs:4` aliases the shared scanner and maps events | Adapter, not a duplicate scanner |
| Thumbnail jobs | `src/library/jobs.rs:58` invokes `library_files::thumbnail_jobs::ThumbnailJobs` | Shared jobs with application activity/events |
| Storage execution | `src/storage/mod.rs:7` re-exports runtime storage queue; platform SQLite modules supply target mechanisms | Shared database ownership need not move into the web platform |

`src/sync/transfer/web_upload.rs` is a feature-gated integration contract test,
not a competing uploader. Its implementation calls `asset-transfer`.

## Stale SQL copies

`src/storage/database.rs:176` onwards already delegates existing-schema file-work,
outbox, and status queries to `client-library`. The old corresponding SQL files
remain under `src/storage/sql/database/`:

- `has_pending_file_jobs_select.sql`
- `pending_local_{file,directory,book,purge}_work.sql`
- `has_pending_outbox_select.sql`, `has_pending_outbox_select_2.sql`
- `stored_sync_status_select.sql`

These match library-owned queries but are not referenced by the backend Rust
source reviewed. Treat them as cleanup candidates, not active duplicate behavior.
Separately, `src/library/jobs.rs:24` and `:40` still actively query purge-work
existence, which could use a small `client-library::file_work` helper.

## Proposed order

1. Share audiobook metadata reads and decoding; this directly removes domain
   logic from a browser-specific module using an existing domain API.
2. Centralize destination-name and purge-work queries; remove verified stale SQL.
3. Move folder browsing assembly into `client-library`.
4. Consolidate transfer authentication retries and native/browser recovery loops
   separately, retaining their account-switch and cancellation tests.

Method: inspected implementations and call sites; compared normalized SQL across
the scoped crates; distinguished production callers from tests and stale assets.
This is not an exhaustive semantic clone analysis. Similar filenames and matching
test-fixture SQL were not counted as duplicate production services.
