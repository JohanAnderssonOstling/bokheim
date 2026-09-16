# Worker split completion audit

2026-09-06. Implemented and verified against the worker architecture handoff in
`docs/web-sync-worker-architecture-handoff.md` (workspace root), together with the
user's Settings/startup follow-ups. Final local release bundle:
`c268ce8403794e6bcaaa13ee`.

## Architecture and acceptance evidence

| Requirement | Implementation | Verification |
| --- | --- | --- |
| UI sees one SharedWorker | `web/coordinator.js`, `client_relay.js`, `runtime/web.rs`; no per-tab remote startup reconciliation | Actual release Firefox contract: two tabs, one scheduler, completed background phases, first-tab disconnect |
| Sole browser SQLite/OPFS owner; serialized writes | Dedicated storage worker, `storage/database.rs` rejects a second concurrent checkout, synchronous owned command closures | SQLite ownership regressions; real OPFS restart/recovery contract; 133 SQLite tests passed |
| No SQLite connection or transaction crosses an await | Short snapshot/apply scopes and queued storage commands; lexical guard scopes | WASM and native Clippy lifetime audits; a removed negative probe proved the custom DatabaseConnection type is recognized. The SQLite, registry and device-SQLite crates contain no async/await paths |
| State sync uses snapshot → HTTP → apply | Rust sync engine runs in the existing SharedWorker and issues typed storage commands; HTTP runs in the network worker | Real browser sync/outbox concurrent-edit, exact-acknowledgment, transient-failure/retry and multi-page recovery contract passed |
| Network and CPU work do not block local reading | Dedicated concurrent network worker; separate chunked hash/inspection/thumbnail CPU worker | Final release: local book opened in 6 ms under held CPU and stalled HTTP; cancellation/backpressure and worker failure JS tests passed |
| Background work is bounded and yields | Interactive burst 8, queue capacity 256; file work pages 16, maintenance/index pages 32, acknowledgment batches 256; metadata pages 32 with indexed keyset cursors and yields | Final real-browser recovery 972 ms, slowest interactive probe 173 ms; 70-book enrichment failure/progression test and cover/rich/author paging tests passed |
| One logical sync/download per key across tabs | SharedWorker scheduling, sync run-again coalescing and transfer ownership | JS service tests and real browser shared-download test: one download, owner tab closes, follower completes, local reuse |
| Durable pending work survives interruptions | Existing persistent outbox and recovery markers retained; exact acknowledgments preserve concurrent edits | Real browser storage restart, cursor recovery, transient retry, account-bound acknowledgment and replacement-account download-intent tests passed |
| Failures complete UI requests; retain useful diagnostics | Rust client drains pending requests and fails future calls on worker failure/unreadable response; subscription/request/startup drop guards; typed phase diagnostics retained | Actual Rust client in Firefox passed failure, malformed-response and worker-crash cases. Startup readiness and HTTP health deadlines remain; old diagnostic-only reader/account-snapshot timeouts removed |

Normal tab closure and shared-download ownership are tested. Abrupt renderer loss
cannot reliably send a MessagePort close notification; durable work and shared
jobs do not depend on a UI request future remaining alive. The tests make no
claim about browser-specific notification delivery after an OS-level renderer kill.

Retained large-file coverage from the storage phase: Firefox and Chromium passed
a publication larger than 128 MiB and indexed maintenance across worker restarts.
These were not repeated after UI/account/metadata changes; their storage and CPU
streaming implementations were not changed by those follow-ups.

## Settings and startup follow-ups

| Problem | Final behavior | Evidence |
| --- | --- | --- |
| Settings awaited HTTP and full storage scans on sync notifications | Immediate local/cached snapshot; remote details through existing scheduler; coalesced background totals | Final release Settings response 3 ms under CPU/network work; background statuses/totals subsequently arrived |
| Every tab performed remote startup reconciliation | Subscribe to library changes, then reread the local list to close the startup subscription race | No app-side `reconcile_libraries()` call or Settings-triggered remote refresh remains; startup local summary loader calls `libraries()`; release UI build passed |
| Cold Settings status opened every database synchronously | Cached statuses; one library per background command; Checking state while unknown; active/pending and account guards | Native first-poll test proves Settings does not open cold library databases; final browser completion check passed |
| Sign-out waited for the server | Local credentials cleared immediately; captured-session revocation runs separately with an eight-second deadline | Native held-notification and subsequent-login regression passed |
| Transfer panel allowed overlapping/out-of-order refreshes | One active request and one pending follow-up; account snapshots no longer independently overwrite transfer groups | Inspected sole refresh call path and active/pending transitions; release UI compilation passed |
| Automatic metadata enrichment selected unbounded candidate sets | 32-row indexed keyset pages for identity, covers, rich metadata and authors; advance past failed pages and yield | 70-book failed-first-request pipeline; 70 candidates per cover/rich/author type in 17-row test pages; full SQLite suite |
| Native metadata writes could fail during maintenance | Metadata write transactions acquire the writer lock before reading (IMMEDIATE) | Reproduced `database is locked` under the app suite, then all 55 app tests passed with the fix |

## Final verification record

- Native app regressions: 55 passed, session 61975.
- SQLite suite: 133 passed, session 58339; final indexed-cursor paging retest passed, session 67804.
- JavaScript workers: all 11 test files passed (`.test-tmp/five-fixes-js-tests.log`).
- WASM connection-lifetime check passed, session 32601; native lifetime audit passed in the preceding phase, session 50100. Subsequent shared async changes were covered by the WASM check and native compilation/tests.
- Full release build 71308 completed; final storage module rebuild 20078 and packaging 60493 completed. Only the storage module needed updating for the indexed SQL predicate and final Settings scheduling changes. Final storage rebuild 77868, packaging 20253 and native Settings retest 19092 (3 tests) passed.
- Actual final release Firefox worker contract 82528 passed (`.test-tmp/five-fixes-browser-final.log`).
- Real browser sync/restart/shared-download contract 70447 passed (`.test-tmp/five-fixes-browser-sync.log`). This passed before the final removal of the Settings remote-refresh trigger, which does not participate in this fixture. The fixture uses the production SharedWorker sync engine/network/CPU services with an instrumented real SQLite/OPFS adapter and a disposable local server.

All task-owned build/test sessions are terminal. The local web server serves the
final bundle. Native source is tested; native executable artifacts were not
rebuilt as part of the user's request to launch the browser release.
