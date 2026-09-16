# Backend boundaries

Frontends use `AppClient` and `LibraryClient`, with view data, import progress, and
reader capabilities exported directly from the crate root. Implementation modules
(`app`, `library`, `storage`, `platform`, and browser transport) are private.

Import state is application data: `ImportProgress` exposes `ImportState` variants
and raw counts, and per-item failures expose `ImportFailureKind`, source paths,
and optional diagnostics. The browser adapter decodes its transport protocol into
these types. UI code owns labels, percentages, failure wording, and retry controls.
Old journal failure strings remain readable as legacy diagnostics.

`AppClient` owns one target-selected transport. Shared application and library
client methods delegate requests, subscriptions, imports, and publication opening
to that transport. Native transport retains direct typed jobs and reader handles;
browser transport owns serialization, subscription leases, and reconnection.
Launch methods live beside their target transport. Browser import packaging and
publication delivery live in browser adapters, while `DirectoryImportFile` holds
an adapter-owned `ImportSource`; JavaScript values do not appear in the shared
import model. Target guards select mechanisms at module boundaries, and still
cover device features and wire-only capabilities.

`runtime/requests.rs` defines the shared application commands with one serializable
reply contract and one codec on every target. Browser publication capabilities and
OPFS staging use a typed extension in `browser_adapter/requests.rs`; extension
commands retain their existing flat wire representation. Native publication opening
returns its reader directly through the native transport and has no wire-command
counterpart. Platform selection is confined to the host-extension module boundary.

Launchers and worker entry points use the separate `host` integration API.
Native launchers pass `host::BackendLaunch` to `AppClient::start_native`; the launch object contains startup configuration and
exposes the directory needed for platform instance locks and UI session files.
Browser worker hosts use `host::BackendWorkerHost`, which only initializes the worker,
returns startup data, and dispatches protocol requests. Worker messages and codecs
are exported under `host`, browser transport under
`host::web`, and CPU execution under `host::cpu`. Runtime probes and opt-in
contracts use `host::testing`. These are integration interfaces, not view APIs.
Neither interface exposes `AppBackend`, `LibraryBackend`, or `BackendContext`.

Application workflows own account policy, metadata/catalog requests, downloads,
and synchronization. Each open-library entry owns a local `LibraryBackend` and a
separate `LibrarySync`. The local handle has no reference to application state or
a network service. Browse operations receive their account-dependent filtering
policy explicitly. Local storage operations remain usable after the application
handle is dropped.

## Workflow adapters

`app/publication.rs` owns the local-content → supported streaming → download
sequence and shared metadata lookup. Its native and browser adapters construct
readers, media URLs, and leases. Native playback-file handling stays with the
native publication adapter. `worker_cpu.rs` owns typed CPU operations; its host
adapters own local execution, worker transport, hash sessions, and PDF subprocesses.
Enrichment asks the CPU provider which inspections it supports.

Library import completion is shared. `library-files::PreparedImport` preserves
browser asynchronous object publication and native transaction-scoped file
placement without target branches in the workflow. Native file imports and
browser staged-byte adoption are separate entry adapters. Storage-aware placement
changes and eviction live in `library-files`; backend owns the transaction and
notifies subscribers after commit.

`app/library_resources.rs` constructs library resources and provides location
snapshots through a selected host. Native snapshots preserve root identity and
generation checks. Optional scanning is represented by the discovery adapter;
handle constructors and application lifecycle methods have one signature.

Sync execution adapters return the exchange result and inventory state. Shared
application code owns identity snapshots, account fencing, and checkpoints.
Inventory repair remains independent of the foreground metadata gate. Browser
transfer coordination owns cross-tab execution while shared transfer code uses
the storage provider to satisfy downloads from retained content.

Pending placement work comes from the asset store on both targets; the shared
job workflow publishes notifications from that result. Account synchronization
uses the platform database-capacity interface. Timing configuration and
publication-lease setup also belong to platform/storage services.

Completed transfer history uses the queue's `HistoryPersistence` interface.
Native hosts retain the ordered MessagePack file writer. Browser hosts store a
bounded snapshot in their library's OPFS-backed SQLite database, with failed
writes retained in memory and retried by flush. Notification sockets and
scheduler hosts are selected at module boundaries; shared application methods
contain no platform selection for these operations.

The UI owns the “Raw subjects” root entry, including its label and search matching.
It opens the same backend label query on both targets. Labels are loaded only on
navigation, and the root entry omits its count rather than scanning the library
on the browse request's critical path.

Synchronization is recovered from the SQLite outbox, durable download requests,
and local/remote asset presence. Database writes wake the scheduler after a
checked-out connection is returned; a wake is only a hint and can also follow a
rollback. Each cycle checks committed pending work before scheduling it. The
scheduler stores deadlines only: periodic local checks, remote refreshes, and
failure backoff. It does not retain or merge per-library work lists. Reading positions use the same debounce and retry policy as other state. Lost notifications and restarts
do not lose work. Local thumbnail recovery remains available while signed out. UI events
invalidate views and deliver progress; they do not decide whether to synchronize.
Explicit remote refresh requests and retries remain separate from local work
because remote changes need not have a local outbox entry.

The browser coordinator owns shared deadlines, per-library sync grouping,
transfer ownership, the bounded CPU queue, and readiness/failure propagation.
The database worker executes complete background cycles using the same typed
Rust phase driver as native. A cycle crosses the coordinator port once in each
direction; individual phases and replies stay beside storage. Rust owns worker
ports, cancellation, tab relays, CPU execution and browser HTTP streams.
Handwritten JavaScript only bootstraps WASM and buffers pre-startup messages.
The coordinator never opens storage. See [the worker runtime](web/README.md)
for ownership boundaries and test configuration.
Run `node backend/scripts/test-coordinator-policies.mjs` to build the coordinator
WASM and exercise its policies through the JavaScript adapters with real ports.
Then `node backend/scripts/test-coordinator-browser.mjs` checks real SharedWorker
startup, CPU execution, and shutdown in Firefox using a disposable storage-worker fixture.
It accepts `PLAYWRIGHT_MODULE` and `BROWSER_EXECUTABLE`, like the other browser tests.


Library mutations emit a contents invalidation; cover, download, and scan updates
retain the targeted information their consumers use. Import notifications do not
reconstruct placement paths, and transfer notifications do not query ancestor
folder counts.

Catalog acquisitions transfer their owned reader to the ordinary book importer.
The catalog client still buffers the bounded HTTP acquisition, but the backend
no longer copies that buffer before staging the publication.

Library operations and application services use `BackendError`. Storage errors
convert at the backend boundary, and client/RPC adapters format frontend errors.
Synchronization retains its typed errors for authentication and retry decisions.

Storage-level scan tests are compiled inside the backend crate, keeping internal
types private. Run them with `cargo test -p bokheim-backend --lib scan_tests`.
`tests/run_sync_e2e.sh` runs the PostgreSQL-backed synchronization checks.

Automatic synchronization owns the state-before-assets ordering and one shared
credential-refresh policy, with at most one authentication retry per operation.
Failures are rediscovered from storage after backoff; write notifications cannot
bypass that backoff. Explicit account or library changes can request a fresh
remote check. Undownloaded EPUBs and PDFs open as seekable remote files using authenticated
byte ranges; other formats use an immediate, targeted download. Explicit Download
still stores the complete book for offline reading.

EPUB and PDF range reads check the reader's memory cache (4 MiB for ordinary ranges, 16 MiB for indexed PDFs), then the device-local
`publication-cache.sqlite3`, then the network. The persistent cache stores 64 KiB
blocks by immutable publication hash and offset, together with the opening
prefix, file length, and transport checksum. Cached immutable EPUBs reopen without
an opening network request. PDFs validate the current revision with a fresh prefix
request, falling back to the cached revision on temporary network failure. Missing
or evicted blocks still need the network. Read-ahead fetches only missing blocks.

The cache is shared across libraries in the same application data directory and
uses the same SQLite storage abstraction on native platforms and browser OPFS.
It has a 256 MiB payload budget, least-recently-used block eviction, and a 288 MiB
physical database cap (temporary transaction journals are additional). Eviction
reclaims database pages. Up to eight background jobs (at most 4 MiB each) are retained; overload,
quota failures, unavailable cache storage, or interrupted writes do not prevent
network reading. Recent writes may be lost when the application exits abruptly.
There is no expiry or periodic revalidation for immutable publications.

Partial cache entries are separate from downloaded assets: they never mark a
book downloaded, and cache eviction never removes an explicit offline download.
PDFs use the same persistent range cache as EPUBs. New ingestion stores page
geometry, outline, simple embedded annotations and a bounded dependency index in
SQLite, synchronized by the exact file checksum. Existing PDFs are not backfilled.
Unchanged native files reuse metadata by their scanned fingerprint and placement
checksum; browser files use their verified immutable OPFS generation. Local edits
invalidate that match until normal ingestion records the new revision. Imported
annotations are read-only snapshots, separate from app-created annotation rows.

Indexed PDFs warm document structure and shared fonts at startup. Rendering and
the existing bounded prefetch window request page bundles through authenticated
`POST /api/libraries/:library_id/blobs/:hash/ranges`. Drawing commands select images,
forms, masks and other graphics resources; unused images in shared resource
dictionaries are omitted. Missing 64 KiB cache blocks are combined into ordered
batches of at most 4 MiB, and the response must match the pinned checksum. All
blocks from a batch share one bounded background cache job. Explicit full-text
search retains demand reads so searching does not prefetch every page image.

A page can require more than one request: a large bundle is split, and PDFium can
still request structural bytes not covered by the index. Dependencies exceeding
the 16 MiB memory cache use demand reads to avoid evicting a page before rendering.
Unsupported or oversized indexes, old metadata and older servers retain bounded
range-reading fallback. The metadata mutation also has a byte budget; unusually
large annotation lists or dependency indexes are bounded or omitted at ingestion.

Run the cache tests with
`cargo test --release -p publication-access --lib cache`.
For browser worker restart persistence and eviction, run
`STORAGE_TEST_PHASES=cache-prepare,cache-verify node client/backend/scripts/test-web-storage.mjs`
from the workspace root (with `PLAYWRIGHT_MODULE` configured if necessary).

The account page observes synchronization status. Its activity state distinguishes
status refreshes from account requests; it has no manual synchronization state.

Native application jobs run on a dedicated thread using the backend-owned Tokio
runtime. Background sync and application jobs share that runtime; application
futures that are not Send remain on their worker thread.
Settings updates execute in order directly in that worker loop, including while
all eight long-operation slots are occupied.

Imports, filesystem scans, and book downloads persist missing-cover work in
SQLite. Local maintenance generates covers asynchronously and retains retry
deadlines and no-cover outcomes across scans and restarts. The scanner has no
thumbnail workers or cover queues. Import completion means the book was committed;
its cover can appear later. Scanning status describes filesystem scans only.

An `OpenLibrary` bundles its local handle, sync service, and event receiver. The
cache stores the bundle returned by library construction. Reader and staged-file
imports validate their filename, format, and destination into one request before
source-specific staging, then share inspection and commit.

Library-list, sync-status, and transfer-status subscriptions retain at most one
unread invalidation. A slow subscriber remains subscribed when its slot is full.
Data-bearing library updates and download progress retain their event payloads.
The browser transport also coalesces unit payloads and cancels subscriptions when
the receiving view goes away.

Removing a book from the library does not automatically close an already-open reader.

Asset pending-work checks and transfer execution share one read-only planner.
The plan is rebuilt after state exchange so newly received metadata is included.
Execution verifies upload bytes, negotiates server admission, and resolves current
placements for requested downloads. Background, user-requested, and immediate
reader downloads share request preparation and completion handling.

`LibrarySync` constructs the shared state synchronization engine directly. The
backend supplies the database's `SyncStore` implementation, which owns SQLite
transactions and cursor persistence; protocol and recovery policy remain in the
shared engine.

The filesystem scanner commits each walk batch, then its coordinator drains and
persists results from the parallel hashing workers before continuing the walk.
There is no separate insertion thread or shared completion registry. A failed
batch is drained before another scan, and failed or cancelled batches do not
trigger vanished-placement cleanup. Hash-result sends check cancellation while
waiting for bounded queue space.

Transfer reconciliation runs one queue-processing loop. The queue always admits
background jobs while retaining user-request priority, thumbnail batching, retry
backoff, and authentication pauses. Immediate reader downloads still bypass the
library-wide synchronization gate.

The scanner worker owns delayed-rescan deadlines and can be woken by filesystem
changes or shutdown. Missing-file grace scans no longer spawn sleeping threads.
Pure library-handle forwarding methods have been consolidated into their
implementations; mutation notifications remain at the operation boundary.

Asset planning and recovery discovery share the database's bounded book-page
query. Ordinary cover maintenance reads only pending thumbnail jobs. Desktop
and browser opening schedule the same background maintenance pass to replay file
work and discover missing covers. The scanner waits for file maintenance before
checking for external deletions; cached books remain readable immediately. Imports, scans,
downloads, missing-cover reads, and restores arrange durable recovery work.
Completed/no-cover jobs are excluded, and retries retain their deadlines.

Purging books and emptying trash commit their cleanup request in the same
transaction as the deletion. Both desktop and browser reclaim unused files in
background maintenance, so file cleanup errors do not fail a committed purge.

Download completion also commits a maintenance request with local availability.
Verified cached bytes are readable immediately; filesystem placements are
materialized by the worker on desktop instead of delaying download completion.

The transfer scheduler owns its worker directly and executes one claimed batch
at a time. Workers have no separate semaphore or `BOKHEIM_TRANSFER_BUDGET_MB`
setting; that per-worker permit count did not govern bytes or concurrent reader
downloads. Queue ownership still prevents competing background schedulers.

Native selected-directory imports execute as one backend-owned job. The job owns
its activity guard, opens files serially, and releases activity after its current
file has settled. Dropping the client future requests cancellation before the
next file; it does not publish an early completion for an in-flight write.
Browser prepare/progress/completion messages identify their import activity.
Completion is idempotent, and late progress for a finished activity is ignored.

## Shared storage and platform mechanisms

`storage/` contains shared database coordination and the adapters connecting
`library-files` and `publication-access` to application storage. Library events
and scan-event mapping live in `library/events.rs` and `library/discovery.rs`.
These modules express application policy rather than a specific OS.

`platform/` selects implementations from `client/platforms`. Its SQLite service
supplies native versus OPFS connection behavior and diagnostics. The common
storage coordinator retains schema initialization, transactions, write
notifications, and connection retirement. Browser integration contracts live
in `contracts/`; their public `host::testing` paths are unchanged.
