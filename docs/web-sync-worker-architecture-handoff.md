# Web sync/storage worker architecture handoff

Date: 2026-09-05

## User-visible failure

After signing in on Firefox, libraries remain sync-pending for a noticeable time. Opening a publication remains on `Sending request to backend…` and eventually reaches the outer 65-second worker-response timeout. Earlier variants remained on `Preparing book; downloading it if needed…` indefinitely because the error path issued another backend request before rendering the original timeout.

## What is confirmed

The page-side reader task starts and sends `LibraryCommand::ResolveBook`. The page stays responsive. The database worker does not emit the direct `request_received` diagnostic placed at the beginning of its message handler. Therefore the observed failure precedes book resolution, HTTP transfer, hashing, OPFS commit, and response serialization. The worker is not servicing new messages.

The absence of a propagated `error` or `unhandledrejection` after adding worker-failure forwarding makes a blocked worker event loop more likely than an ordinary JavaScript/WASM crash. It does not prove the exact blocking call.

The earlier `database is locked` error is relevant supporting evidence that browser database concurrency/ownership is unhealthy, but it is not by itself proof of the exact current stall.

## Current topology

`WebWorkerClient` creates `web_backend_worker_loader.js` as a SharedWorker. That broker creates `web_backend_database_worker_loader.js` as a dedicated Worker and transfers each tab's `MessagePort` to it. The dedicated worker initializes one `BackendWorkerHost` and runs all of the following on one worker event loop:

- application RPC dispatch;
- SQLite and OPFS access;
- background synchronization orchestration;
- HTTP synchronization and asset transfers;
- hashing, publication verification, and previously thumbnail extraction;
- interactive book resolution.

The relevant files are:

- `apps/web-backend-worker/web_backend_worker_loader.js`
- `apps/web-backend-worker/web_backend_database_worker_loader.js`
- `apps/web-backend-worker/src/lib.rs`
- `client/backend/src/runtime/web.rs`
- `client/backend/src/storage/database.rs`
- `client/backend/src/executor/wasm.rs`
- `client/backend/src/app/background/`
- `client/backend/src/sync/scheduler.rs`

On WASM, `executor::run_blocking` directly invokes the closure. It does not offload work. Any synchronous SQLite, OPFS, hashing, parsing, or thumbnail operation can monopolize the database worker.

`storage::database::Database` is also not a single-connection owner. `get()` pops an idle connection, but when none is idle it synchronously opens another SQLite connection to the same path. This is risky with OPFS-backed SQLite and async task interleaving. A checked-out connection or transaction spanning an `await`, followed by another task calling `get()`, can produce lock contention or a same-thread liveness failure.

## Fundamental diagnosis

The core design combines four roles in one execution context:

1. cross-tab coordination;
2. exclusive persistent-storage ownership;
3. slow network execution;
4. unbounded CPU/background work.

This makes an interactive storage request dependent on completion/yielding of unrelated background work. In a single-threaded worker, a synchronous lock wait or CPU operation also prevents timers and diagnostic handlers from running. Adding more timeouts inside that worker cannot solve such stalls.

## Recommended architecture

Keep global coordination across tabs, but separate coordination, network/CPU execution, and storage ownership:

```text
Tabs
  -> Shared coordinator
       - deduplicates sync runs and downloads
       - owns job leases and subscribers
       - broadcasts progress/results
       - never performs blocking SQLite work
       -> network/CPU workers
            - HTTP, streaming, hashing, parsing
       -> storage worker
            - one serialized SQLite/OPFS owner
            - short commands and transactions only
```

The storage worker should expose operations such as:

- snapshot pending mutations and cursor;
- snapshot pending asset jobs;
- atomically apply a sync response;
- test/read/store a content-addressed blob;
- atomically finalize transfer metadata.

Network waits must occur after releasing storage ownership. A sync cycle should snapshot work, perform HTTP elsewhere, then submit a short apply command. Book opening should check storage, download/hash elsewhere if absent, then commit through the storage owner.

The coordinator should coalesce periodic sync requests and key asset jobs by `(library_id, content_hash)`. Multiple tabs subscribe to one logical job, so architectural separation does not imply duplicate network traffic. A request arriving during an active sync should set one `run_again` flag instead of starting another complete run.

## Recommended implementation sequence

1. Make the web storage layer an actual single owner with a serialized async command queue. Do not synchronously open a second connection when the first is checked out.
2. Audit and eliminate every SQLite connection/transaction lifetime that crosses an `await`.
3. Split state sync into `snapshot -> HTTP -> apply`, ensuring the database owner is free during HTTP.
4. Move asset download/upload and hashing outside the storage owner; retain cross-tab job leases in the coordinator.
5. Add interactive priority or bounded fairness so background reconciliation cannot indefinitely delay reader commands.
6. Retain phase/progress messages as typed protocol events, but remove diagnostic-only timeouts once cancellation and health reporting are established.

## Changes already made during diagnosis

- Added phase diagnostics for resolve dispatch, targeted download, HTTP headers/body, hash verification, OPFS/storage work, response encoding, and response posting.
- Added a typed `Diagnostic` worker message and reader-visible phase updates.
- Added direct `request_received`, `library_lookup`, and `library_found` boundaries.
- Made web reader errors render immediately instead of issuing a second backend request first.
- Added propagation for worker `error` and `unhandledrejection` to pending page requests.
- Embedded the four GPUI icons previously fetched from a nonexistent `/assets` endpoint.
- Disabled eager background thumbnail generation on WASM; it used synchronous `run_blocking` on the database worker. This did not resolve the confirmed pre-dispatch stall, so it was not the sole cause.

These changes are diagnostic/mitigating work, not the fundamental storage-owner refactor.

## Important uncertainty

The exact synchronous call currently blocking the worker has not been identified. Do not claim that thumbnail generation, HTTP, OPFS, or a specific mutex is proven as the culprit. The evidence establishes the architectural liveness failure at the worker-message boundary. The database connection model and synchronous WASM execution are the strongest code-level risks to investigate first.

## Reproduction/acceptance criteria

Reproduction:

1. Start the release web app in Firefox.
2. Sign in and allow background synchronization to start.
3. Open a publication.
4. Observe that the reader remains at `Sending request to backend…`; no `Backend: request received…` phase appears.

The redesign is successful when:

- an interactive worker request is acknowledged promptly while sync/download jobs are active;
- multiple tabs trigger only one logical sync/download per key;
- network stalls do not block browsing or opening already-local books;
- no SQLite connection or transaction spans a network await;
- storage writes remain serialized and crash-safe;
- closing one tab does not cancel work still leased/subscribed by another tab.
