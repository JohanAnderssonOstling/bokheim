# Browser worker split — historical implementation notes

For current crate ownership and build entry points, see
[the web application README](../../apps/web-backend-worker/README.md) and
[the worker runtime README](web/README.md).

The requested end state is a SharedWorker coordinator as the sole UI entry
point, with dedicated storage, network, and CPU workers. Local book opening must
remain responsive during synchronization, transfers, and thumbnail generation.
The existing durable SQLite outbox is retained.

## Implemented so far (2026-09-05)

- Browser `Database` rejects a second simultaneous connection checkout instead
  of opening another OPFS SQLite connection. Native connection pooling remains.
  This guard complements the queued sync storage commands described below.
- SharedWorker retains tab ports and relays separate storage sessions. A tab's
  disconnect closes its session without terminating shared workers.
- SharedWorker lazily creates one CPU worker with a bounded queue and one active
  transformation. Failures reject active/queued jobs; later jobs can restart it.
- CPU WASM example does not initialize the backend or persistent storage.
  Thumbnail extraction/resizing, import inspection, streamed download hashes,
  staged import hashes, and verification of existing publication bytes use it.
- Hash sessions consume bounded chunks and fail explicitly if the CPU worker
  loses their state. Cancellation attempts to release the session.
- Backend fetch is bridged through a dedicated network worker created by the
  coordinator. Response bodies are pulled through a MessagePort with
  backpressure. Cancellation aborts HTTP; worker failure rejects pending reads.
- Release build bundles all worker loaders/modules and CPU WASM.
- Browser storage-contract harness now uses the real CPU worker.

Sources: `src/worker_cpu.rs`, `examples/cpu_worker.rs`, `web/*.js`,
`src/storage/database.rs`, `src/storage/files.rs`,
`src/library/maintenance.rs`, `src/library/operations.rs`, and the app worker
loaders/build script under `apps/`.

## Sync boundary update (2026-09-05)

- The existing Rust `SyncEngine` now awaits typed `SyncStore` operations. Native
  sync uses the same engine and database implementation.
- Browser sync invokes a separate WASM module inside the existing SharedWorker;
  this does not create another worker. The module accesses storage through
  explicit sync commands and routes HTTP through the dedicated network worker.
- `sync_service.js` serializes passes by server/account/library. Triggers during
  an active pass coalesce into one follow-up using the latest request input.
  Each pass reports its own outcome to its waiters. Queue overload and worker
  failure reject callers explicitly.
- `sync_bridge.js` retains the storage callback for the pass lifetime. Rust
  transfers callback ownership to JavaScript so dropping the Rust waiter cannot
  invalidate a callback that the coordinator still needs.
- Database sync commands use a bounded async queue. Commands execute synchronously
  and return owned results after releasing their connection. A macrotask yield
  between commands admits UI messages; eight interactive admissions yield to
  queued background work. UI dispatch currently queues its initial admission,
  not every storage phase of every operation.
- App build/loader wiring includes the coordinator WASM and sync bridge/service.
- Browser outbox/inventory reads now cross the worker boundary in pages of at
  most 256 rows. Outbox pages target 256 KiB of stored bodies, allowing a single
  oversized row so later mutations remain reachable. A captured sequence
  watermark excludes later edits from the current scan.
- Browser cursor recovery commits bounded pages while retaining its durable
  recovery marker until an empty final page. An interrupted run restarts
  idempotently. Missing inventory cells are also enqueued in bounded commands.
  Native transaction semantics remain unchanged.
- Four SQLite paging regressions pass: concurrent insertion/acknowledgment,
  deleting earlier inventory rows, byte limits/oversized rows, and idempotent
  cursor recovery with a retained marker. Coordinator WASM check passes.
  Release rebuild and Firefox multi-page sync contract passed.
- Other maintenance/reconciliation commands and large publication copies still
  require the remaining storage-duration audit.

## Verification recorded

Latest boundary checks: coordinator WASM example check passed; six sync-service
and three sync-bridge tests passed; native sync suite passed 62 tests (four
PostgreSQL tests ignored); two storage queue fairness/overload tests passed.
The first release rebuild and Firefox topology retest passed (3.0 ms local
open during busy CPU/stalled HTTP). A subsequent real sync contract exposed
a Firefox POST-body bug in the network bridge: Request.body was absent even
though arrayBuffer() returned bytes. The bridge now reads non-GET/HEAD bodies
with arrayBuffer(); six network tests pass, including this regression. The fixed release rebuild and Firefox sync contract retest both passed. Earlier browser
results below apply to the previous CPU/network split checkpoint.

- CPU service JavaScript tests: 3 passed.
- Network bridge JavaScript tests: 5 passed, covering body backpressure,
  concurrent stalled HTTP, cancellation, worker failure, and HTTP error status.
- Client relay JavaScript test: passed, including separate tab sessions and
  keeping another tab alive after disconnect.
- Native exclusive-storage tests: 2 passed.
- Native CPU protocol/transformation tests: 3 passed.
- Backend sync/transfer tests: 62 passed, 4 PostgreSQL integration tests ignored.
- WASM CPU example check passed before the latest streamed staging changes.
- Release web build completed successfully with the streamed staging changes.
- Real browser storage contracts passed `prepare`, `restart`, and `verify` in
  Chromium and Firefox with the CPU worker, including >128 MiB publication and
  cover/trash recovery contracts.
- Full topology test initially reached book opening and exposed a pre-existing
  diagnostic panic: `std::time::Instant::now()` in runtime/dispatch.rs on WASM.
  Changed it to `web_time::Instant::now()`; release rebuild and Firefox retest passed.
- `scripts/test-web-workers.mjs` and `examples/worker_probe.rs` exercise the
  production release SharedWorker bundle using the production binary protocol.
  The test server redirects external HTTP to a local stalled endpoint (no
  account request/email leaves the test origin), and makes the actual CPU worker
  busy while a second tab opens a local book. This test passed: local book opened
  in 4.0 ms while CPU was held for 1500 ms and HTTP was stalled. The second tab
  remained functional after the first tab disconnected. This proves that tested
  topology/load scenario, not full sync/outbox/coordinator completion.

## Remaining requirements — do not mark the goal complete yet

1. Extend queued storage ownership beyond state sync and initial UI admission;
   verify every interactive operation and background storage phase.
2. Audit every connection/transaction lifetime across awaits, including browser
   registry/account storage and long maintenance operations.
3. State-sync passes now run in the SharedWorker. Background policy, asset
   reconciliation/transfer scheduling, and related leases still need migration
   from BackendWorkerHost as appropriate to the agreed coordinator ownership.
4. Bound reconciliation/maintenance storage command duration. Whole publication
   thumbnail/inspection inputs are still materialized and MessagePack encoded
   in storage before transfer; remove large storage-side copies/work where needed.
5. Establish cancellation/health semantics across workers, subscriber leases,
   and coalescing of repeated sync triggers; verify deduplication across tabs.
6. Exercise Firefox with actual storage/network/CPU workers: open an already-local
   book while sync, thumbnails, and stalled transfers are active, including a
   second tab and closing the first. Verify outbox edits/retries/crash recovery.
7. Review diagnostics/timeouts once health and cancellation are established.

There are concurrent edits to sync/application files from another process.
An asynchronous question about session/file ownership was sent to the user.
Re-read current files before editing; preserve that work. Its AssetPlan snapshot
refactor superseded the initial connection-lifetime edit in asset_sync.rs.
At latest inspection, state_sync.rs implements SyncStore directly for Database,
and LibrarySync constructs sync_engine::SyncEngine directly; the original
BackendSyncStore/StateSyncEngine wrappers were removed by that other process.

## Environment notes

The worktree has extensive pre-existing changes and untracked refactor files.
Do not reset it. Writable workspace is `client/`; changes to app worker loaders
and build script were applied through approved patches outside that root.

Both disks were nearly full during validation. `cargo clean -p bokheim-backend
--profile dev` freed about 3.8 GiB of generated build outputs. A Firefox download
under `/tmp/bokheim-worker-browsers` failed to extract due to tmpfs space; retry
uses `client/.test-tmp/worker-browsers` and succeeded. Set TMPDIR to
`client/.test-tmp` for browser tests because `/tmp` remains nearly full.
Playwright is available at
`/tmp/backend-contract-browser/node_modules/playwright-core`. Chromium is at
`/home/johan/.cache/ms-playwright/chromium-1234/chrome-linux64/chrome`.

Native backend tests require default/scanner features: a no-default-features
`--lib` test build fails in existing scanner-dependent tests. WASM builds use
`--no-default-features`. The release build uses `RUSTC_BOOTSTRAP=1` for GPUI.

The new `scripts/test-web-sync.mjs` contract uses real Rust sync WASM in a
SharedWorker, production bridge/service/network modules, and a feature-gated
SQLite/OPFS fixture in a dedicated worker. It checks a concurrent replacement
of an outbox mutation during stalled HTTP, exact-ID acknowledgment, transient
failure/retry, persistence across storage-worker restart, and first-tab close.
The Firefox contract passed after fixing the POST-body bug described above.
This fixture exercises the real engine/bridges/storage boundary, but does not
replace an end-to-end signed-in application/account test or the remaining
large-command/scheduler migration audit.

Latest environment checkpoint: disk exhaustion interrupted writes to
`network_bridge.js` and its regression test. Both were restored/repaired and
verified by six Node network tests and the passing Firefox sync contract.
`cargo clean -p bokheim-backend --profile dev` and
`cargo clean -p subject-projection --profile dev` removed generated artifacts.
Another process also freed disk space; roughly 7 GiB became available.
The last release build and browser sync test are terminal/successful.

Latest paging checkpoint: the expanded Firefox contract now uses real
reading-position updates on an established book, rather than synthetic outbox
rows. It seeds another 300 books, recovers and drains multiple outbox pages,
and exercises paged inventory reads. This passed with exact acknowledgment,
transient retry, storage restart, and first-tab close. The first expanded run
correctly rejected an orphan synthetic reading-position register; fixing the
fixture to use real canonical state resolved it without weakening recovery.
The failed download directory `/tmp/bokheim-worker-browsers` was removed with
approval after /tmp exhaustion temporarily prevented sandbox commands starting.
The current release bundle includes paged worker sync commands. Remaining
scheduler/maintenance/connection-lifetime work above is still incomplete.

## Background timer migration checkpoint

Browser `SyncScheduler::run` now forwards wake/refresh signals over a dedicated
MessagePort and awaits cycle requests from `background_scheduler.js` in the
SharedWorker. The SharedWorker owns debounce, periodic reconciliation, retry
backoff, and the one-active-cycle rule. The existing backend cycle executor
still reconstructs and executes maintenance/account/asset work; that remaining
orchestration and its large storage commands have not all moved.

Four deterministic scheduler tests and two real-port bridge tests pass, as does
the coordinator WASM check. A real timer test caught fractional timeout rounding
that could misclassify an early refresh as a local cycle; the timer now rechecks
its deadline. Refreshes arriving during an active pass remain scheduled for a
follow-up, and ordinary writes do not bypass failure backoff.

The app build now bundles `background_scheduler.js`. The topology browser test
has been extended to assert one scheduler across both tabs and at least one
actual background cycle. The first packaging attempt was killed during wasm-bindgen under severe memory
and swap pressure. The retry with RAYON_NUM_THREADS=1 and BINARYEN_CORES=1
succeeded. The Firefox topology test passed with one actual scheduler across
both tabs and a 3.0 ms local book open during busy CPU/stalled HTTP.

Inspection found full publication materialization still in WASM
`recover_thumbnail` and import inspection (`read_to_end` before CPU submission).
These and whole-library maintenance/reconciliation loops remain concrete work.

The scheduler startup test exposed a release packaging bug: separate bindgen
invocations wrote different `inline0.js`/`inline1.js` files into the same crate
snippet directory. The build script now places each WASM module and its snippets
in its own directory, with a root JavaScript re-export for existing loaders.
The current release was repaired using freshly regenerated glue and retaining
optimized WASM only after byte-identical glue comparisons. This repaired bundle
passed the real two-tab topology test. The sync/outbox contract retest also passed, including concurrent real reading-position edits, exact acknowledgment, transient retry, storage restart, and multi-page recovery/inventory.

Fatal coordinator shutdown now explicitly terminates the database worker to
release SQLite/OPFS ownership; a focused test verifies shutdown and idempotence.
Current scheduler checks: four timing tests, two bridge tests, three existing
sync bridge tests, and one coordinator shutdown test pass; WASM check passes.

Latest release/test processes have completed successfully. The current bundle uses isolated module directories. No goal-completion claim: publication copying, maintenance/reconciliation bounds, remaining job orchestration, and the broader storage lifetime audit remain.

## Bounded publication transfer checkpoint

WASM import inspection and thumbnail recovery now use `submit_publication`:
storage reads at most 256 KiB, awaits the CPU acknowledgment, then reads the
next chunk. Full publication assembly and parsing happen in the CPU worker.
The protocol caps active inputs at four, rejects oversized chunks, and rejects
updates/finish after session loss. Cancellation sends an abort; finish removes
the session before parsing so parser errors also release the input.
Native reader-based processing is unchanged.

The WASM check passed. CPU protocol tests cover chunk ordering/preservation,
capacity, cancellation, lost sessions and parser failure cleanup. A real EPUB
fixture additionally compares streamed inspection with direct inspection.
The topology browser test now explicitly arms the next CPU job after the first
import instead of assuming an implementation-specific message count.
The release bundle has NOT yet been rebuilt for this transfer change, so the
previous browser results do not verify it. Maintenance/reconciliation batching,
remaining orchestration and the broader storage lifetime audit remain open.

## Maintenance paging checkpoint

`AssetStore::maintain_files` now uses queued browser storage commands: file-work
pages of 16 through a captured maximum ID, then one unreferenced-file cleanup
transaction per queue turn. Both local maintenance and sync asset reconciliation
use it. The native path retains synchronous maintenance. A regression test proves
pages advance past leased jobs, preserve jobs above the captured watermark, and
retry skipped work on the next pass. Existing 11 asset-store tests passed.
Thumbnail scanning now reads book rows in pages of 32 rather than constructing
all book paths first; before generating missing covers it checks the existing
live-placement query. The WASM check passes for these changes.

Five management operations now drop their committed connection before replaying
file work or looking up the materialized directory name. These nested checkouts
were incompatible with browser single-owner storage.

Still open: synchronous maintenance calls in startup, management/purge and
transfer completion; the OPFS index still returns a full file list; AssetPlan
still builds a full snapshot; remaining account/background orchestration and
connection lifetime audit; rebuilt browser tests for these changes. The release
bundle still predates publication chunking and this maintenance work.

## Deferred startup and paged asset planner checkpoint

Browser library opening now records local_asset_maintenance_pending and wakes
the existing scheduler; it no longer scans paths, replays file work or deletes
unreferenced bytes before returning the handle. Browser organization/purge and
transfer completion also leave durable maintenance requests. Native filesystem
projection still executes synchronously at those call sites.
Local maintenance consumes an existing request before awaiting and restores it
on failure/cancellation with a Drop guard. Clean passes do not unconditionally
write the marker, avoiding a perpetual scheduler wake loop.

AssetPlan production loading now uses queued pages of 32 for requests and book
rows (both targets, to exercise the browser algorithm in native tests). Remote
presence/rejection checks are per hash rather than full-table snapshots. Pending
work collection is asynchronous so it can await these pages without retaining
SQLite. The old synchronous planner exists only as a test reference. Empty
publication hashes are excluded, matching the former path query.

WASM check passed after startup deferral; durable-work recovery tests passed.
The new 95-book planner test initially rejected an incomplete fixture missing
book_dir.local_hash; its fixture was corrected and the rerun passed (95 jobs and requests across multiple pages, no requests consumed).
Still needed: comparison tests for planner behavior, current browser rebuild and
contracts, large-library responsiveness evidence, OPFS index list bounds,
remaining account/job orchestration and completion/lifetime audit.

## Full regression checkpoint

All eight JavaScript test files passed. Full backend suite initially failed from
/tmp exhaustion; rerun with TMPDIR=client/.test-tmp and two test threads passed:
212 passed, zero failed, four PostgreSQL integration tests ignored.
The low-thread browser release build is still live in exec session 20371, past
both Rust compile phases and generating/optimizing the bundle. Do not restart it
without polling that handle. No current Rust implementation edits were made
during the build. WORKER_SPLIT_AUDIT.md now tracks the remaining completion gates,
including a newly identified diagnostic forwarding disconnect cleanup gap.

## Rebuilt Firefox topology checkpoint

Release build session 20371 completed successfully. The new bundle includes
publication chunking, maintenance/asset-plan paging and deferred startup.
Firefox two-tab topology contract passed: 5.0 ms local book open while CPU was
held for 1500 ms and HTTP stalled; one actual SharedWorker background scheduler;
second tab remains usable after first-tab disconnect.

The diagnostic forwarding task now selects on an explicit disconnect signal,
and the request handler gives its closure lifetime to JS instead of forgetting
it permanently. Patch applied to apps/web-backend-worker/src/lib.rs; its WASM
check passed. This fix was made after bundle compilation. Module-only build
session 43743 is still live. Once terminal, /tmp/bokheim-refresh-database-module.sh
can generate/optimize glue under client/.test-tmp/refreshed-database-module;
then replace the bundle's web_backend_worker subdirectory and rehash the bundle
with an approved parent-workspace mutation. Do not restart live build 43743.

Diagnostic-cleanup worker build 43743 and module packaging 84431 completed.
Installed generated module with client/.test-tmp/install-database-module.py
(the attempted /tmp installer was truncated by disk exhaustion; do not use it).
Current bundle is 17cb5b6b720daac26ec0ad62. Firefox retest passed: 4.0 ms local
book open under held CPU/stalled HTTP, one scheduler, first-tab disconnect.
Sync contract session 69567 passed all recovery cases. All known build/test
sessions are terminal; no live process needs resuming from this checkpoint.
/tmp exhaustion sometimes prevents sandbox startup; workspace TMPDIR works for
contracts. Remaining work is tracked in WORKER_SPLIT_AUDIT.md.

## OPFS index paging checkpoint

Maintenance now enumerates indexed logical file names in pages of 32, yielding
through the background queue between pages and cleanup transactions. Browser
storage-usage scans use the same paged index and yield while summing file sizes.
VFS startup adopts existing legacy assets into the index without overwriting
newer physical mappings or deletion tombstones. Native indexing regression
passed (70 entries, tombstone preservation, newer mapping preservation, deletion
between pages, namespace bounds). WASM checks passed, including paged totals.

The storage browser harness now isolates bindgen output directories per module.
Its contract checks 70 files/69 live entries and paged totals across all three
worker-restart phases, plus the existing >128 MiB publication, cover and trash
recovery checks. Firefox test is live in session 69323; poll it rather than
restart. Current app bundle 17cb5b6b720daac26ec0ad62 predates index paging and
will need an updated database-worker module after this phase is verified.

The first Firefox index-paging run passed prepare/restart and its index/byte
assertions, but failed the final old trash-contract assertion that restore work
was already drained synchronously. The contract now explicitly checks that the
book is readable while durable restore work survives restart, runs the background
maintenance pass, then asserts the queue is empty and bytes remain intact.
Rerun is live in session 31734 (69323 is terminal/failed). Poll the new handle.
Normal tab closure is already covered by runtime/web.rs pagehide; persisted
back/forward-cache pages intentionally retain the connection.

Firefox rerun 31734 passed prepare, restart and verify, including >128 MiB
publication streaming, 70-file index paging/deletion persistence, paged totals,
cover recovery, and deferred restore work surviving restart then draining.
All sessions from this checkpoint are terminal. App bundle still predates the
index-paging changes. Next: update the worker bundle after remaining source
changes, Chromium storage regression, and final orchestration/lifecycle audit.

## Shared download ownership checkpoint

Chromium storage contract passed prepare/restart/verify using a short TMPDIR
symlink /tmp/bokheim-browser-tmp -> client/.test-tmp. The original long workspace
TMPDIR exceeded Chromium's Unix-socket path limit; that was a launch failure,
not an OPFS regression. Production asset planner tests now use load_queued and
all three pass (the old test-only planner was removed by concurrent work).
Remote presence writes and explicit download preparation now take background
queue turns rather than looping synchronously over a whole library.

Re-read original requirements in docs/web-sync-worker-architecture-handoff.md:
cross-tab download deduplication was still missing for direct reader downloads.
New transfer_service.js owns one publication download per server/bound-user/
library/hash. TransferWorker::handle_job routes browser DownloadBlob jobs through
bokheimTransferRequest in sync_bridge.js, so direct and background paths share
the coordinator. Only the first executor callback runs; all followers receive
the same serialized typed TransferError/result. JS owns callback lifetime via
Closure::once(...).into_js_value(), including uncalled follower callbacks.
Coordinator fatal failure rejects transfer waiters. Build script now includes
transfer_service.js. Service tests cover dedup, capacity, owner subscriber
cancellation, retries and failure; real MessagePort bridge tests cover one
executor/shared result, thrown executor failure and storage-worker shutdown.
WASM worker-host check passed.

Known remaining work in this new path: real-browser simultaneous downloads and
stalled network acceptance test; verify stale placement handling for followers
and account changes; audit unexpected tab loss and overall job orchestration.
The currently installed bundle still predates index paging/transfer ownership.
The module-only build 14899 completed before transfer ownership was added, so
its artifact is also stale. All sessions in this checkpoint are terminal.
Use original architecture handoff + WORKER_SPLIT_AUDIT.md for final acceptance.

## Real shared-download checkpoint

Browser download wrapper now rechecks local storage after admission (avoids a
late duplicate HTTP request), checks the bound account before execution and on
return, and allows one follower retry when the original executor's placement
became stale. Results carry an explicit abandoned-placement flag so an ordinary
HTTP 404 does not become a needless retry. WASM worker-host checks passed.

sync_contract.rs and test-web-sync.mjs now exercise production LibrarySync
and TransferWorker with real SQLite/OPFS, network and CPU workers plus the actual
SharedWorker TransferService. The first expanded run passed: two tabs issue one
HTTP request, storage responds during its stall, first-tab close preserves the
follower's completion, and a later local request does not fetch again. All prior
outbox/restart/multi-page checks also passed (session 23826 terminal success).
The script serves current sync_bridge/transfer_service/cpu_bridge source over
the disposable origin while retaining release CPU and sync WASM; this is a real
worker boundary contract, not yet the updated production app bundle.

A further run adds explicit missing-publication handling (404 is one request,
returns missing and does not take an abandoned-placement retry). It is live in
session 74825; poll rather than restart. Current bundle still predates indexed
maintenance and transfer ownership. Final account-change-during-network audit,
asset scheduling ownership and current full-bundle acceptance remain open.

## Transfer account-response checkpoint

The missing-publication contract rerun (30977, after fixing the fixture's
connection dereference) is terminal with exit 0. Transfer metadata writes now
validate the bound user and current server after HTTP and before applying
results. Successful and rejected upload responses, missing/corrupt downloads,
thumbnail results and publication lease waits use this check; a token refresh
for the same user/server remains valid. Old-account errors become retryable
before the caller can resolve or suppress the new account's durable work.

Native transfer suite passed: 26 tests, including held-HTTP account switches
for successful/missing/corrupt/forbidden downloads and successful/forbidden
uploads (92050 terminal success). All ten web JavaScript test files passed.
The real-browser sync fixture now also changes the account during a held
download and checks that the new durable request survives; this extension
still needs its browser run.

Full release build is live in session 73392. Poll that handle, do not restart
on quiet output. This build includes indexed maintenance, SharedWorker transfer
ownership, and the transfer account checks. Run actual-bundle topology and the
expanded sync contract after it completes. Home disk was down to 735 MiB free;
client/.test-tmp/client-review-target belongs to other ongoing work and should
not be removed. Use the short /tmp/bokheim-browser-tmp TMPDIR symlink.

Further audit findings to resolve: state-sync storage callbacks still need
account-binding validation inside queued commands; upload-manifest remote
facts need the same apply-time protection; shared download admission should
recheck its captured binding after waiting for a transfer semaphore. Account/
asset phase orchestration and unexpected tab loss remain open against the
original architecture document. Concurrent source edits continue, including
transfer scheduler changes; re-read current files before modifying them.

Release build 73392 completed successfully. Installed bundle is
1093a7d0355e90f5a9601520; coordinator.js, sync_bridge.js and transfer_service.js
match source byte-for-byte. Actual-bundle Firefox topology test 99484 passed
with a 3.0 ms local open during held CPU/stalled HTTP, one background scheduler,
and first-tab disconnect. No full release rebuild is currently running.

Expanded sync contract 83021 failed at the 15-second multi-page recovery
deadline (test-web-sync.mjs line 176), before the new account-change case,
while the topology probe was compiling. Its isolated rerun is live in 97355;
poll it before diagnosing/restarting. Native and JS results above remain valid
for their tested source snapshots. Concurrent cleanup removed the transfer
semaphore, so the semaphore-admission follow-up is no longer applicable;
verify current source rather than restoring the removed machinery.

Isolated sync rerun 97355 passed and is terminal (exit 0): concurrent edit and
exact outbox acknowledgment, transient failure/retry, storage restart, first-tab
close, multi-page cursor recovery/inventory, shared publication download, 404
without duplicate retry, and account change during held HTTP preserving the
replacement account's durable download request. The earlier recovery timeout
did not recur without the simultaneous topology-probe build. All sessions from
this checkpoint are terminal. Remaining architecture/account-update work is
still open; this is not a completion claim.

## Account-bound queued sync checkpoint

worker_sync.rs now captures the database account binding for each sync and
checks it inside every queued command, including each recovery page. The final
inventory result and last-successful-sync marker also reject account changes.
state_sync.rs exposes a shared synchronous pull-page apply helper; its SQL and
atomic page/cursor transaction are unchanged. Manifest negotiation validates
binding around HTTP, inside queued remote-presence writes, and before preparing
or starting transfer jobs. Native asset suite passed four tests, including
normal versus account-switched manifest responses (40439 terminal success).
WASM worker-host check 6058 passed.

The browser sync contract now includes old-account acknowledgment rejection and
interactive SELECT probes during the 300-book recovery. Initial runs hit the
old 15-second recovery deadline, then one initialization deadline. Host process
inspection showed another backend test process using 523% CPU, but a subsequent
instrumented run (6665, terminal failure) proved a real storage latency issue:
one interactive probe took 13,947 ms during recovery. Do not dismiss this as
only host load. Outbox acknowledgment was issuing one durable commit per ID.
Both native SyncStore and browser AccountStorage now acknowledge in chunks of
256 inside one transaction per chunk, yielding between chunks on WASM.

The contract grants the full recovery up to 60 seconds but independently
requires every interactive storage probe to finish under 1 second. Startup and
other commands retain their existing deadlines. OPFS/WASM startup diagnostics
now identify initialization failures. Rerun after transactional acknowledgment
is live in 28821 (currently waiting for another artifact-directory lock). Poll
that handle; no other task-owned process is live. Bundle 1093a7d0355e90f5a9601520
predates these latest account/acknowledgment changes and needs refresh after
verification. SharedWorker phase scheduling and lifecycle audit remain open.

Transactional acknowledgment rerun 28821 passed and is terminal (exit 0).
Recovery finished in 1,799 ms; both interactive probes completed promptly,
slowest 184 ms. All prior sync/restart/outbox/download checks passed, along with
old-account sync acknowledgment rejection and preservation of replacement-
account download intent. This materially closes the measured recovery stall.
Native sync regression suite is now running in 1192. Release database-worker
module rebuild is running in 42204. Poll both handles before packaging the
module with the existing staged bindgen/wasm-opt helper and refreshing the
bundle. The JS protocol is unchanged in this phase, so only the database module
needs replacing. Native manifest suite (4 tests) was already green.

Native sync regression 1192 passed: 32 tests, four PostgreSQL integration tests
ignored. Database worker build 42204 and staged bindgen/optimization 40399 both
completed successfully. Installed module in bundle 59e1d5854393abf5420f8e6c.
Actual-bundle Firefox topology 23686 passed: 6.0 ms local open under held CPU /
stalled HTTP, one SharedWorker background scheduler, and first-tab disconnect.
All task-owned sessions are terminal at this checkpoint. The installed database
module includes account-bound queued commands and transactional acknowledgment.

Next remaining scope: move account/library/asset phase sequencing to the
existing SharedWorker JS scheduler while preserving native behavior and the
current durable work reconstruction. Inspect app/background/, app/core.rs,
sync/scheduler.rs and web/background_scheduler.js + sync_bridge.js. No extra
WASM coordinator or worker is needed: extend the existing scheduler port with
typed phase commands/results and keep database callbacks short. Complete the
tab/failure lifecycle and diagnostic-timeout audit against the original handoff,
then rerun relevant actual-bundle acceptance. Do not mark the whole goal done
based on the completed account/acknowledgment phase alone.

2026-09-06: SharedWorker phase sequencing and UI failure handling are implemented.
The scheduler sends typed snapshot/maintenance/account/state/assets phases;
state checkpoints finish across libraries before asset transfers. Eleven JS
worker test files passed. Earlier actual-bundle Firefox verified phase completion,
13 ms local opening under held CPU/stalled HTTP, tab disconnect, and the real
Rust client's pending/future rejection on failure, malformed input and crash.
Full release build 91974 subsequently completed, installing 12bd758092abf91bd908ce9e;
the user opened that build on localhost:4173.

The user reported Settings lingering on Refreshing during startup on both native
and WASM. The former account snapshot cache and its background refresh loop were
subsequently removed: a foreground settings view now owns its presentation cache
and explicitly requests a fresh account-and-library snapshot when it needs one.
Persistent browser and reader preferences remain registry-backed and are not part
of that presentation state. Verification in progress: release rebuild 5576;
native Settings regressions 43715. The first native test attempt omitted scanner
features; a subsequent attempt encountered a concurrent registry API edit
(current source again accepts LibraryId). No product workaround was added for
those build-only failures. Browser test now explicitly measures Settings
responsiveness alongside the existing local-open load check.

Compiler-assisted connection audit: backend/clippy.toml rejects DatabaseConnection,
rusqlite Connection/Transaction/Savepoint across await. A temporary negative probe
correctly failed, proving the custom wrapper is recognized. Two production findings
were explicit-drop scopes, now lexical blocks in library_sync/transfer executor.
The negative probe was removed. WASM Clippy 67017 passed with the disallowed-type
lint denied (log .test-tmp/worker-clippy/current.log). JS tests also pass (11 files).
Whole-goal completion still requires final updated-bundle browser/sync acceptance
and updating WORKER_SPLIT_AUDIT.md; do not infer completion from this checkpoint.

Settings follow-up verification completed: native tests 43715 passed (2 tests);
WASM lifetime check 67017 and native lifetime check 50100 passed; release build
5576 completed and installed 48b6d7ee9bb5b2bbac36a460. Firefox actual-bundle test
80505 passed: Settings snapshot 2 ms under CPU/network work, local book open
6 ms, one SharedWorker scheduler with completed phases, first-tab disconnect,
and Rust UI failure/malformed/crash handling. The first browser attempt found a
test-only JSON adapter limitation for binary UUIDs; worker_probe now decodes
AccountSnapshot through its production type before JSON conversion. The backend
returned success on that initial attempt. No live task-owned build/test process
remains. The local release server continues serving the new dist. Native app
binaries were not rebuilt; the shared source fix is validated by native tests.
Remaining whole-goal gate: rerun real sync/outbox/restart/shared-download contract
against the final bundle, then close the requirement-by-requirement audit.

2026-09-06 follow-up: user explicitly requested fixing five further latency
patterns. Implemented source changes: per-tab startup reconciliation removed
(UI now subscribes then rereads local summaries); logout clears local session
and detaches a bounded eight-second server notification using captured credentials;
BatchJobs permits one active refresh and one pending follow-up, and no longer
accepts a second competing snapshot from AccountSnapshot; Settings status is
cached and refreshed one cold library per background command, with Checking
placeholder status and account/pending guards; automatic metadata enrichment
uses 32-row keyset pages for identity, covers, rich metadata and authors, advances
past failures, and yields between pages/books. Query helpers live in
library/src/external_metadata/mod.rs. Existing unpaged APIs remain
for explicit/manual callers. WASM lifetime check 10660 passed before final small
follow-ups. Native app suite 2037: 52 passed, three failures in new regressions.
Two were fixture errors (no default library and session for wrong configured
server), now corrected. The 70-book enrichment test sent each book exactly once
but recorded only 64 attempts; debugging that outstanding finding with warnings
enabled in live test 88074. Do not mark these five fixes complete yet. Parent UI
patches applied through approved patch; final whitespace cleanup still needed
in view.rs library_rows. Release/browsers must be rebuilt/retested once remaining
tests pass. Latest served release is still 48b6d7ee9bb5b2bbac36a460 (earlier fix).
Removed only task-owned debug/incremental cache with approval to reclaim 2.4 GB;
use CARGO_INCREMENTAL=0 for subsequent native builds. Goal remains active.

Five-fix regression follow-up: the missing enrichment outcomes were a real native
SQLite writer-upgrade race, reproduced in the full app group (database is locked).
External metadata write transactions now start IMMEDIATE, acquiring the write
lock before reading. Native app rerun 61975 passed all 55 tests, including the
70-book failed-first-request pipeline and immediate local logout with a held
notification. SQLite suite 58339 passed all 133 tests; separate keyset test covers
70 cover/rich/author candidates in 17-item pages. WASM lifetime check 32601 passed;
all 11 JS worker test files pass. Release rebuild 71308 is live (log
.test-tmp/five-fixes-release.log), still compiling/packaging; poll its handle.
Browser contract now also waits for non-Checking statuses and local totals after
the immediate Settings response. Once release completes, run actual-bundle
worker contract and real sync contract using isolated target/env from previous
entries. Latest installed/served completed bundle still precedes these five fixes.

Completion checkpoint: all five responsiveness fixes and original worker-split
acceptance are verified; see the rewritten WORKER_SPLIT_AUDIT.md for the scoped
requirement/evidence matrix. Final bundle 25323eb539b0c65f1ba16ba9 includes the full
UI release plus the storage-module indexed-cursor refinement. Browser worker
contract 86646 passed: Settings 2 ms, local opening 5 ms under CPU/network load,
background status/totals arrival, one scheduler, tab closure and real client
failure handling. Real sync contract 70447 passed: recovery 972 ms, slowest
interactive probe 173 ms, durable outbox/concurrent edit/exact acknowledgment,
restart/retry, shared download/owner closure/local reuse, and account guards.
All build/test handles are terminal; local release server remains available.

Final review removed Settings' own remote refresh trigger entirely; startup,
sign-in and the existing scheduler already own remote refreshes. This eliminates
a possible follow-up pass requested by a late-starting Settings page. Targeted
native test 19092 passed all 3 tests; storage build 77868 and packaging 20253
completed. Final installed bundle c268ce8403794e6bcaaa13ee passed Firefox contract
82528: Settings 3 ms, local book opening 6 ms, background status/totals completion,
one scheduler and tab/client failure handling. Sync contract 70447 remains the
applicable passing gate for the unchanged sync service/storage adapter; the last
change only removes a Settings-triggered scheduler request. All task-owned
build/test handles are terminal. Completion audit updated with final scope and
evidence.
