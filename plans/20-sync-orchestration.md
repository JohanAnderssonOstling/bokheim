# Plan 20: Sync orchestration (when sync actually runs)

Touches `Backend/src/sync.rs` / `Backend/src/library.rs`. Plan 17 (poke) plugs into this as an
additional trigger; works without it.

## Problem

Nothing in the Backend schedules `sync_once` — it's entirely caller-driven (UI/Mobile). So:
local edits sit unpushed until the next external call; failed transfer jobs (`next_retry_at`)
never retry on their own (workers exit when the queue drains); a device left open but idle
never converges.

## Change

One background orchestrator task owned by `Library`/`SyncManager`:

1. **Triggers** for a sync request (coalesced through one debounced channel):
   - local change: new `event_log` rows — cheapest signal is a notification sent from the
     places that already emit `LibraryEvent`s, or a poll of `MAX(local_seq)` vs
     `last_pushed_local_seq` on the periodic tick;
   - periodic tick (e.g. every 5 min) — also the retry path for failed transfer jobs;
   - poke (plan 17), download request (`request_download` currently waits for the next sync);
   - app foreground/network-regained hooks exposed as a public `request_sync()` for the
     UI/Mobile layers.
2. **Single-flight**: at most one `sync_with_server` in flight; requests during a run set a
   dirty flag → one follow-up run.
3. Backoff on consecutive failures (cap ~15 min) so an offline device doesn't hammer.
4. Signed-out / expired token (plan 19): orchestrator idles instead of erroring.

## Tests

1. A local edit with the orchestrator running pushes within the debounce window without any
   external `sync_once` call.
2. N rapid edits → one (or two) sync runs, not N (single-flight + coalescing).
3. Failing server → backoff growth observed; recovery on next success resets it.
