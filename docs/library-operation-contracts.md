# Library operation ownership

The library actor is a lifetime and coordination boundary, not an executor of
whole sequential workflows. `handle_command` is synchronous. The exhaustive
match in `commands::prepare_library_request` either performs a short operation
in receive order or returns owned asynchronous work.

## Scheduling and ordering

| Work | Execution contract |
| --- | --- |
| Database queries, reading positions, annotations, logical folder/book mutations, import progress | Synchronous command handling; receive order is preserved. |
| Remote ranges/bundles, playback grants, reader preparation, thumbnails, storage accounting | Disposable read operations; at most eight executing. |
| Explicit downloads | Existing fast path; at most four executing, with one per content hash. A follower rechecks local availability after acquiring the key. This does not replace background transfer coordination. |
| Bytes, path, directory and staged imports | One executing import workflow per actor, independently of queries and logical mutations. Staged preparation hands its activity to the import coordinator on success. |
| Inventory repair | Managed maintenance operations; at most two executing. Existing sync gates still coordinate with background sync. |
| Scan, filesystem replay and upload preparation | One each, polled independently alongside operations and commands; repeated replay/preparation wakes coalesce. Replay completion wakes preparation, but an active enrichment batch cannot block subsequent replay. |
| Sync and background transfers | Existing library-owned workers and schedulers; not moved into the command loop. |

The request operation set admits at most 128 outstanding operations, including
permit/key waiters. Excess requests receive an error without blocking command
handling. This is a limit on accepted async operations, not on the transport
mailbox or on all background tasks combined.

Logical directory moves commit and notify immediately. Physical reconciliation
uses the existing durable directory-work queue, woken through contents events.
A move reply acknowledges the logical commit, not completion of filesystem
replay. Direct session workflows that require inline replay retain that path.
Filesystem replay has a separate owned job from inspection/network enrichment;
it still defers while scans or directory imports are active.
Replay completion refreshes views and wakes sync directly, without emitting a
new replay request; remaining durable work must not create a self-wake loop.

Imports may overlap moves, deletion, scans and sync. Destination liveness is
rechecked after inspection and inside the final database transaction. Native
publication resolves the current destination. Existing projection fingerprints,
scan generations and transfer guarded commits remain authoritative; actor
ordering is not a replacement for them. Inspection/metadata preparation stays
outside database write transactions.

## Cancellation and retirement

- A disposable read stops when its receiver closes. Its owned resources drop.
- An admitted download or single-book import is not cancelled merely because
  its caller stops waiting. Library retirement still drops local operations.
- A native directory import checks cancellation before starting and between
  files. The current file completes if only the caller leaves.
- Browser shared downloads retain coordinator ownership. Dropping this actor's
  waiter does not revoke another tab's shared download.
- Staged preparation has an activity guard: rejection, unpolled drop, error or
  panic cannot leave the activity registered. Successful preparation transfers
  responsibility to the durable import coordinator's finish command.
- Retirement closes admission and subscriptions immediately. The loop stops
  without executing its remaining backlog. An inbox guard closes and drains
  queued commands on normal exit, cancellation or panic.
- Owned pending-reply guards send an error if an operation is dropped. Queued
  command reply senders are dropped by inbox cleanup, so callers get the
  existing actor-stopped transport error instead of waiting forever.
- Operation panics are converted to typed backend errors. Scan/preparation
  panics use their existing failure paths. A panic in synchronous command
  handling retires the actor through unconditional cleanup.

Browser reader leases are installed only when successful preparation completes
and its caller still exists. Registration and reply happen synchronously on the
browser thread; only this actor-owned state needs an explicit completion value.

## Execution mechanism and limits

`PendingOperations` owns local futures in `FuturesUnordered`. They are polled
alongside the inbox, events, scan and preparation futures. There is no new
thread pool, duplicate transfer scheduler, or requirement to serialize native
replies. A future being async does not imply another thread.

This change removes waits for asynchronous I/O from command handling. It does
not make synchronous SQL or filesystem operations preemptible. The native
database's busy timeout, expensive queries, and small inline filesystem probes
remain latency risks under contention/slow storage. Bulk discovery, inspection,
and storage work continue to use their existing platform execution adapters.
Already-running blocking jobs cannot be forcibly cancelled; durable recovery
and guarded publication remain necessary across retirement/process failure.

## Verification

- `library::handle::concurrency_tests`: real HTTP held pending during explicit
  download and playback authorization; queries, mutation ordering, event
  publication, scan completion and retirement still progress.
- A held enrichment future does not prevent two successive physical folder
  renames through the actor's normal contents-event/replay path.
- `library::pending::tests`: admission/concurrency limits, same-book exclusion,
  read versus accepted-write cancellation, unpolled/active reply cleanup,
  panic containment, and inbox cleanup with retained senders.
- Existing scan and native import/parity tests cover progress, cancellation
  between files, source panic, typed replies and subscriptions.
- `import::commit_tests`: stale destination rejects a whole import batch without
  reviving the folder or committing earlier members/scan generation.
- Compile the browser worker's all-targets `web-runtime-tests` configuration;
  this is a compilation gate, not a claim of browser runtime execution.
