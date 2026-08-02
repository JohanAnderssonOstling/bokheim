# Plan 22: Sync status observability

Atomic; touches `Backend/src/scanner.rs` (the `LibraryEvent` enum lives there),
`Backend/src/sync.rs`. UI consumption is out of scope (separate UI work).

## Problem

Sync outcomes are log-lines only. The UI cannot show "last synced", "offline", "syncing", or
"login expired" — failures are invisible to the user, which makes every sync bug look like
data loss.

## Change

1. New `LibraryEvent::SyncStatus { state }` with
   `state ∈ { Started, Completed { at }, Failed { kind, detail }, AuthExpired, Offline }`
   (kind: auth / network / server / local). Emitted from `sync_with_server` and the
   orchestrator (plan 20); `AuthExpired` from plan 19's 401 path.
2. Persist `last_sync_completed_at` in `sync_metadata` so the UI can show staleness across
   restarts.
3. Downgrade noisy per-item transfer logging to debug now that outcomes are surfaced.

## Tests

1. Successful `sync_once` emits Started → Completed and stamps `last_sync_completed_at`.
2. Unreachable server emits Failed{network}; 401 emits AuthExpired (with plan 19).
