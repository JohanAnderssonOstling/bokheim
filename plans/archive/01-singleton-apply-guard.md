# Plan 1: Guard singleton applies against newer unpushed local edits

Independent. No prerequisites. Prerequisite for the convergence guarantees in plans 4 and 6.

## Problem

Server-side, singleton events (`SINGLETON_EVENT_TYPES` in `SyncCommon/src/lib.rs:10`, currently
`reading_position_updated` and `book_opened`) are LWW-resolved: one row per
`(library, entity_id, event_type)`, overwritten only by a strictly greater `(hlc_pt, hlc_l, client_id)`
(`Server/src/db.rs` `insert_singleton`). Pull excludes the caller's own events (`client_id != $3`).

Client-side, `apply_remote_change_in_tx` (`Backend/src/db.rs`) applies singletons unconditionally.
Hole: if a push is transiently deferred (e.g. `HlcTooFarFuture` rejection, see `push_local_events` in
`Backend/src/sync.rs`), the subsequent pull applies an *older* remote value over a *newer* unpushed
local edit. The local edit later wins on the server and every other device converges to it — but this
device never receives its own event back, so it keeps the stale value indefinitely.

## Change

In `apply_remote_change_in_tx`, before the `match` on `change.op`: if `change.op.is_singleton()`,
look up the max HLC among **unpushed** local events for the same entity and type; if it beats the
incoming change's HLC, return early without applying (still call `observe_remote_hlc` first — clock
observation must happen regardless).

New query in `Backend/src/database/library.sql`:

```sql
-- name: get_max_unpushed_hlc?
-- param: last_pushed_local_seq: i64
-- param: entity_id: &str
-- param: event_type: &str
SELECT hlc_pt, hlc_l FROM event_log
WHERE local_seq > :last_pushed_local_seq
  AND entity_id = :entity_id AND event_type = :event_type
ORDER BY hlc_pt DESC, hlc_l DESC
LIMIT 1;
```

`last_pushed_local_seq` comes from `sync_metadata` (same key `get_unpushed_events` uses).
Compare tuples `(pt, l)`: skip the apply only if local is **strictly greater**; on exact tie apply the
remote. (The server breaks ties with `client_id`, which the pulled `Change` doesn't carry — exact
HLC ties across devices are practically impossible with ms+logical clocks, so this asymmetry is
accepted. Note it in a comment.)

`SyncOp::is_singleton()` already exists in SyncCommon; the entity id and event type come from
`change.op.entity_id()` / `change.op.event_type()`.

## Tests (Backend/src/db.rs, in-memory style like existing tests)

1. Init db, insert a book, set its read_pos locally (trigger writes event_log + HLC). Do **not**
   advance `last_pushed_local_seq`. Apply a remote `ReadingPositionUpdated` with a lower HLC →
   read_pos unchanged.
2. Same setup, remote HLC higher → applied.
3. Mark the local event pushed (`mark_events_pushed`) → a lower-HLC remote now applies
   (guard only protects *unpushed* edits; once pushed, the server is authoritative).
