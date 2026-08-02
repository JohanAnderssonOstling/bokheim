# Plan 24: Server append-log compaction (deferred; design when bootstrap hurts)

Borrowed from Kafka log compaction. Not urgent: singletons already compact; only placement
appends (`book_dir_added`/`book_dir_removed`) grow with churn. Trigger to build: fresh-device
bootstrap time becomes noticeable.

## Idea

A placement is itself an LWW register keyed `(dir_id, book_id, file_name)` — the client gate
proves it (only the highest-HLC event matters; losers are never needed). So the server can
periodically delete placement events that are **superseded**: for each placement key, keep
only the latest-HLC event (and keep everything newer than every device's pull cursor, so
nobody loses un-pulled history — same liveness bookkeeping as plan 23, build together).

## Cautions

- The dedup index `(library, client_id, client_seq)` means a re-pushed deleted event re-inserts;
  compaction must keep per-client max `client_seq` knowledge or tolerate re-insert (harmless:
  it would be superseded again).
- `changed_at`/HLC ties across clients: keep the same winner the client gate would pick
  (remove-wins on tie) — reuse `SyncCommon` rules (plan 12 extracts them).
- Alternative if this gets hairy: Replicache-style snapshot bootstrap instead (server
  materializes state for fresh devices); compare costs when the trigger fires.

## Acceptance (for now)

Tracked; revisit alongside plan 23 when bootstrap latency or log size shows up in practice.
