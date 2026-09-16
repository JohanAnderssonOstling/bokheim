# Repository and crate boundaries

Bokheim organizes Rust packages by runtime ownership rather than putting every
package below a generic `crates/` directory.

```text
apps/                 UI, visual renderers, platform applications, and tools
client/               headless client backend, local persistence, and replica code
servers/sync/          account, synchronization, and blob service
servers/opds-cache/    public curated OPDS cache service
shared/                portable code used across ownership boundaries
```

The main dependency shape is:

```text
apps ───────→ client ───────→ shared
  └────────────────────────→ shared

client document services ──→ apps/reader/crates

servers/sync ──────────────→ shared
servers/opds-cache ────────→ shared
```

Server-specific crates stay inside their owning server directory. The sync
server and OPDS cache server do not depend on each other.

`apps/reader/crates` is an application-owned processing subsystem rather than a
pure outer UI layer: the client backend uses its headless EPUB/PDF engines for
metadata and thumbnail work. UI crates remain consumers of `client` and do not
move into the backend ownership directory.

## Shared crates

- `content-address` owns `ContentHash`, the content-addressed identity required
  by both the library model and asset-transfer protocol.
- `catalog-model` owns provider-neutral catalog values, language filtering, and
  the shared search text normalizer. Applications, storage, and backend code
  depend on this transport-independent model.
- `catalog-protocol` owns only the internal client/cache-server page and search
  protocol, including sequenced publication payloads.
- `catalog-core` owns only shared cover caching and cover-specific errors. It
  owns no SQLite values, pool, schema, language policy, client payload writer,
  crawl jobs, server inventory, enrichment queues, or materialized-index
  orchestration.
- `publication-model` owns portable bibliographic and contributor values,
  including author authority identifiers and their validation.
- `publication-enrichment` owns pure author, entity, and subject resolution.
  Backend schedules lookups and persists their results.
- `sync-common` owns the opaque synchronization envelope understood by clients
  and the sync server: protocol IDs, cursors, request/response DTOs, mutation
  kinds, limits, and server-visible blob declarations. It does not decode
  library payload values.
- `account-contract` owns account-service DTOs implemented by the sync server
  and consumed by its client. Session lifecycle lives in `account-client`;
  application status models stay with the backend.
- `wire` owns the common binary encoding helpers.
- `test-support` owns fixtures shared by client and server tests.

## Client crates

- `backend` (`bokheim-backend`) owns library lifecycle, session persistence,
  host adapters, and composition for application UIs.
- `library-files` owns asset storage, verified staging, leases, physical placement,
  native scanning, pending filesystem work, and thumbnail job execution. Backend
  supplies database access, progress/events, and CPU-worker callbacks.
- `account-client` owns authentication requests and session lifecycle: expiry,
  serialized refresh, retry after unauthorized responses, and login/logout race
  protection. Backend supplies `SessionPersistence` and the sync credential adapter.
- `publication-access` owns revision-pinned remote reads, bounded range caching,
  native and browser seekable readers, and audiobook stream recovery. Optional
  `pdf` and `sqlite-cache` features add PDF page warming and persistent caching.
  Backend supplies cache storage scheduling and background execution.
- `library-model` owns client library, annotation, reading-position, and view
  values. It re-exports bibliographic and author identity types from
  `publication-model` for client callers.
- `client-library` (`client/library`) owns library behavior and persistence in
  feature modules: metadata, browsing, reading, assets, file work, and replica
  state. Each feature owns its SQL files. `schema` coordinates connection setup
  and migration order for their shared SQLite database. See
  [client library ownership](../client/library/README.md).
- `device-sqlite` is the narrow database-opening boundary shared by client
  services. It prepares native parent directories and native journal policy,
  while passing browser storage locators to the installed SQLite VFS. It owns
  no application schema or queries.
- `catalog-client` consumes the catalogue server for curated registry, page,
  search, cover, and acquisition traffic, and owns the client's partial SQLite
  catalogue store, including exact language-keyed page/search layouts. Its
  active registry contains provider-neutral catalogue
  sources and opaque server locators; it does not contact or route external
  OPDS providers directly.
- `library-replica` owns the client-replica schema for synchronized library
  payloads, local mutation coalescing, conflict keys, and conversion to and from
  `sync-common::WireMutation`. The sync server never links it.
- `sync-transport` owns HTTP/WebSocket sync transport and response validation.
  `sync-engine` owns protocol orchestration; `asset-transfer` owns durable transfer
  jobs, publication upload/download execution, deadlines, and progress throttling.
  Backend supplies credentials, storage handles, and application notifications.
- `library-registry`, `book-metadata`, `thumbnail`, and
  `audiobook-library` are headless client services composed by the backend and
  applications.

Filesystem-relative paths, scanners, SQLite projections, and local placement
rules belong to client services, principally `library-files` and `client-library`.
They are not part of the client/server protocol.

## Applications and renderers

Application binaries and shared UI crates live directly under `apps/`. Visual
and document rendering is owned by the reader under `apps/reader/crates/`.
The analysis tooling is maintained as the separate sibling `DataAnalysis`
project rather than as an application in this workspace.

## Servers

- `servers/sync` is a separate Cargo workspace. It owns accounts, synchronized
  state storage, blobs, thumbnails, HTTP endpoints, and PostgreSQL adapters.
- `servers/opds-cache` serves the curated public OPDS registry and cached
  catalogue data. Its local `crates/opds` and `crates/litteraturbanken` own
  upstream protocols, while `src/store` owns crawl generations, durable worker
  queues, operational inventory, server-only indexes, its complete version-4
  SQLite schema, and its connection pool. It owns no synchronization or account
  state.

## Backend module boundaries

`client/library-files` owns native and browser asset storage, verified staging,
leases, physical placement, discovery, and pending-work protection. SQL lives
beside its owning feature, with test fixtures in the crate. Its `FileDatabase`
interface lets backend supply pooled connections and queued writes;
`ScanEventSink` forwards scanner events without importing application event types.
Browser hashing uses a host-provided service backed by the existing CPU worker.

Backend's `storage/files.rs` and `library/discovery.rs` contain host adapters.
Shared connection ownership and write notifications live in `storage/database.rs`;
cache scheduling adapters live in `storage/publication_cache.rs`. `platform/`
selects native or browser SQLite mechanisms without owning library policy. Library lifecycle and relocation remain in backend. Import and scan
both use `client_library::toc` to convert inspected navigation.

`app/enrichment/scheduling.rs` orders candidate phases and pages.
`app/enrichment/application.rs` prepares results from read snapshots and
applies short conditional write batches. Edition review, cover acquisition,
author enrichment, rich metadata, and status have their own modules beneath
`app/enrichment/`. Matching rules remain in `publication-enrichment`;
persistence remains in `client-library`.

`library/jobs.rs` adapts thumbnail progress and events to
`library-files::thumbnail_jobs`. Its processor interface preserves native worker
reuse and browser RPC; its activity guard preserves cancellation behavior.

`asset-transfer::publications` owns verified upload/download execution and
browser Blob uploads. Account and placement checks surround asynchronous work
and conditional publication. Backend retains account refresh, shared browser
request coordination, and batch notifications. Local revision queries live in
`client-library::assets`.

`scripts/checks/backend-boundaries.py` rejects dependencies or included fixtures
that point from extracted services back into backend.
`scripts/checks/library-sql.py` checks SQL-file ownership and forbids inline SQL.
Both run in CI alongside release tests for the extracted crates.

Library features import their domain types, SQL traits, and cross-feature helpers
explicitly. `client/library/src/lib.rs` declares features and public view values;
it no longer acts as an internal persistence prelude. Shared transactions remain
owned by the library schema boundary.

`asset-transfer::planning` owns bounded asset candidate paging, remote availability
negotiation, and transfer job selection. Its queries live in `client-library::assets`
as SQL files. Each page snapshots up to 32 candidates in one statement; filesystem
inspection runs after releasing the read transaction and storage command. Settlement
rechecks the account, exact work IDs, and pending file work in a write transaction.
`asset-transfer::thumbnails` owns thumbnail batch execution and local
recovery; backend supplies CPU resizing and library event callbacks. Backend retains
authentication refresh, account-scoped scheduling, and reconciliation cancellation.

Application lifecycle triggers live in `backend/src/app/sync_lifecycle.rs`.
Startup/login request independent metadata, discovery, outbox, asset, and repair
lanes. Selection refreshes the selected library without requesting discovery.
Foreground resume after 30 seconds requests reconciliation without waiting for a
notification handshake; generation counters preserve coalesced suspend/resume
events. Resume is event-driven and adds no polling timer.

Inactive file recovery runs through the outbox lane, including signed-out startup.
Each database inspection yields through the background storage queue; selected
asset planning checks only its own library. Idle inactive libraries stay unopened.

Application background work lives in `backend/src/app/background/`:

- `mod.rs` dispatches independent scheduler lanes and manages selection-scoped activities.
- `account.rs` handles discovery, queued account writes, registration, and credential retries.
- `libraries.rs` synchronizes the selected library and drains inactive outboxes.
- `assets.rs` schedules file/thumbnail work, transfers, and inventory repair.

Construction in `app/core.rs` supplies scheduler resources; background work retains
weak application references. Outbox inspection belongs to `client-library::sync_state`,
and upload-rejection retry times belong to `client-library::assets`. Production SQL
remains in those features' SQL files; background integration fixtures live with its tests.

## Transfer execution and sync integration boundaries

`asset-transfer::runner` owns queue draining, upload negotiation handoff, retry and
rejection outcomes, authentication pauses, and wake scheduling. Backend implements
`TransferHost` for account refresh, browser coordination, notification-interest
guards, and UI events. The library assets module owns rejection lookup and atomic
settlement; rejected downloads announce availability only after settlement commits.

`client-library::sync_state::exchange` owns pull-page application and inventory
checkpoint persistence. Backend's `AccountStore` retains queued storage commands,
account validation under the write transaction, and commit notifications. Protocol
exchange and recovery orchestration already belong to `sync-engine`; SQLite
transaction decisions stay with persistence and its host adapter.

Sync integration tests live under `sync/library_sync/tests/`, grouped into accounts,
metadata, recovery, assets, cancellation, protocol, and PostgreSQL integration.
Shared builders live in `tests/mod.rs`. Account-store tests live beside their adapter
in `sync/state_sync/account_tests.rs`.

The generic `SyncJobs`, `TransferJobs`, and `CpuJobs` queues live in
`client-platform-runtime::coordinator`. Backend retains browser message ports and
application teardown in `runtime/coordinator_lifecycle.rs`.

Architecture checks follow workspace and target-specific path dependencies,
prevent infrastructure from depending on feature services, prohibit cross-crate
SQL includes, and keep migrated backend adapters SQL-free. Regression tests for
the checks run in CI with `python3 -m unittest discover -s scripts/checks/tests`.
