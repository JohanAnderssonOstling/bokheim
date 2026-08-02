# Plan 4: Consolidate dir events into a `dir_state` server singleton

Requires plans 2 (batch closure) and 3 (dir materialization) to be done first.
Pre-release: wipe the server's `library_events` rows as part of rollout instead of writing
legacy-event mapping code.

## Why

Dir lifecycle is currently three append event types (`dir_added`, `dir_moved`, `dir_deleted`)
applied unconditionally on clients. The server resolves nothing for appends, so concurrent dir
edits from two devices diverge permanently (each device ends with the other's version). The
server's singleton mechanism (`insert_singleton`, `Server/src/db.rs`) resolves per
`(library, entity_id, event_type)` by HLC and re-draws `server_seq` so winners resurface — but only
if all dir mutations share **one** event type carrying **full state**.

Design rule (keep forever): a singleton payload must be sufficient to build the entity from
scratch. "Create" vs "move" ceases to exist on the wire; there is only "this is D's current
state". A fresh device receiving only the latest state can construct the dir directly; the
intermediate history is intentionally compacted away.

## Changes

### SyncCommon (`SyncCommon/src/lib.rs`)

- Replace `DirAdded` / `DirMoved` / `DirDeleted` with
  `DirState { dir_id, parent_id, name, deleted: bool }`.
- Add `"dir_state"` to `SINGLETON_EVENT_TYPES` (server routing to `insert_singleton` is then
  automatic; no server code change).
- Update `event_type()`, `entity_id()`, `payload()`, `from_event_parts()`. Payload:
  `{parent_id, name, deleted}` with `deleted` as JSON 0/1 (what SQLite's `json_object` emits for
  boolean expressions) — parse with `as_i64`/`as_bool` tolerantly.
- Existing test `singleton_list_matches_is_singleton` keeps things honest.

### Triggers (`Backend/src/database/triggers.sql` + drop list in `db.rs reset_development_schema`)

Replace the four dir triggers with two, both emitting the same shape:

- AFTER INSERT ON dir (skip `__root__`, `change_origin = local`)
- AFTER UPDATE OF parent_id, name, deleted_at ON dir (skip `__root__`, only when something
  actually changed, `change_origin = local`)

Payload: `json_object('parent_id', NEW.parent_id, 'name', NEW.name, 'deleted',
NEW.deleted_at IS NOT NULL)`, event_type `'dir_state'`, entity_id `NEW.id`. Note the delete case
no longer needs a BEFORE trigger; deletes are just a state change. HLC stamping via
`trig_event_log_hlc` is unchanged (it only special-cases book_dir events).

### Client apply (`Backend/src/db.rs`)

One `SyncOp::DirState` arm replacing the three dir arms:

- Ignore events for `__root__` or with empty name/parent (defensive).
- Resolve old path before / new path after for plan 3's materialization hook.
- Upsert: needs a new query (existing `upsert_dir` forces `deleted_at = NULL`):

```sql
-- name: apply_remote_dir_state!
INSERT INTO dir (id, parent_id, name, deleted_at)
VALUES (:id, :parent_id, :name, CASE WHEN :deleted THEN 1 ELSE NULL END)
ON CONFLICT(id) DO UPDATE SET
    parent_id = excluded.parent_id,
    name = excluded.name,
    deleted_at = excluded.deleted_at;
```

(`change_origin` is `remote` during apply, so local triggers stay silent.) Unconditional apply is
correct: the server only ever ships the resolved winner, plan 1 guards the unpushed-local edge,
plan 2 guarantees in-batch dependency closure (a placement arriving before its dir in the same
batch is fine under deferred FKs; the dir_state later in the batch creates the row).

### Rollout

- Wipe `library_events` on the dev server; clients re-push state on next scan/edit.
- Local `event_log` rows with old types: `get_unpushed_events` will fail to map them
  (`from_event_parts` → None) and drop them with a warning — acceptable pre-release; mention in
  commit message.

## Tests

1. Trigger emission: create dir → `dir_state` row with full payload; rename → second `dir_state`
   with new name and higher HLC; soft-delete → `dir_state` with deleted=1.
2. Convergence: apply two concurrent remote `DirState`s for the same dir in both orders (with
   plan 1's guard inactive — both remote) → same final row both times? NOTE: clients apply
   unconditionally, so this test belongs server-side conceptually; instead test the realistic
   sequence: local rename (unpushed) + incoming older remote DirState → plan 1 guard skips it.
3. Create-then-add-book-then-rename compaction: simulate a fresh device receiving only
   [`book_dir_added`(seq n), `dir_state`{final name}(seq n+1)] in one tx (deferred FKs) →
   commit succeeds, dir exists with final name containing the placement.
