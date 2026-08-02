# Plan 17: Server poke — push-triggered sync (borrowed from Replicache)

Atomic; new server route + a client listener. Carries **no data** — correctness stays entirely
in the pull protocol; the poke only collapses sync latency from poll-interval to seconds.
Pairs with plan 20 (orchestration), which decides *when* to sync; this adds one more trigger.

## Change

1. Server: `GET /api/sync/events` — SSE stream, auth'd, scoped to `(user, library)`. After a
   successful push commits, broadcast a contentless `changed` event to that library's
   subscribers except the pushing `client_id` (in-process `tokio::sync::broadcast` per
   library; no persistence — a missed poke is harmless because the next periodic sync
   catches up).
2. Client (`SyncManager`): a background task holding the SSE connection; on `changed`, request
   a sync (debounced — coalesce bursts into one `sync_once`). Reconnect with backoff; treat
   the stream as best-effort (offline = no stream = periodic sync still works).
3. No protocol/schema changes; no new convergence reasoning.

## Tests

1. Server unit: push from client A → subscriber B receives one event; A does not.
2. Client: simulated poke triggers exactly one sync for a burst of N pokes within the
   debounce window.
