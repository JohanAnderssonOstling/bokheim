# Set-membership conflict resolution

## Goal
Make conflict resolution correct for entities that represent **set membership** — currently only `book_dir` (a book's directory placements), and any future tags / collections / bookmarks / highlights. Today these events apply unconditionally in arrival order, which can desync replicas when events are reordered, retried, or replayed after long offline periods.

## Non-goals
- No field-level LWW for record-shaped entities (e.g. `book_added`). Out of scope.
- No server-side HLC clamping for clock-skew defence. Out of scope.
- No payload schema versioning. Out of scope.
- No changes to singleton handling (`reading_position_updated`, `book_opened`).
- Not adding tags/collections/highlights/bookmarks themselves — only making the conflict layer ready for them.

## Background

### What exists today
- `Change` carries an `Hlc` tuple and a `client_id`-based tie-break (`SyncCommon/src/lib.rs:48-54`).
- HLC LWW is **only** consulted for singleton entities (`is_singleton_event`, `should_replace_singleton`). Non-singleton changes apply unconditionally in `apply_remote_change_in_tx` (`Backend/src/db.rs:381-493`).
- `book_dir_added` / `book_dir_removed` / `book_dir_deleted` are emitted per-element by triggers (`Backend/src/database/triggers.sql:183-256`), so set semantics are *operationally* correct when events arrive in causal order.
- However, `entity_id` for these events is `NEW.book_id` (`triggers.sql:191`), **not** the element tuple `(book_id, dir_id, file_name)`. The element key only lives in the payload (`dir_path`, `file_name`, `file_path`).
- The remote apply path runs `insert_book_dir` or `mark_book_dir_deleted` keyed on the tuple resolved from payload, with no HLC compare.

### Concrete failure modes today
1. **Reordered Add/Remove on the same element.** A → `Add(b, d, f)` at HLC=1, then `Remove(b, d, f)` at HLC=2. If B replays in HLC-reverse order (server pagination boundary, network retry, post-offline catch-up), the row ends up *present* on B. The local "reactivate" trigger may then re-fire and propagate the wrong state.
2. **Stale resurrection after long offline.** B observed `Add(b, d, f)` weeks ago, queued locally. Meanwhile A removed it; B has applied that Remove. A retried push from B then arrives at the server and propagates back; B re-applies the old Add on top of its own tombstone, because nothing compares HLCs.
3. **Same-tuple delete-then-readd.** Works today by accident of operational ordering, but is fragile — there is no rule, only "last arrival wins".

### Latent risk
When tags / collections / highlights / bookmarks are added, two patterns exist in the codebase:
- `book_added`-style: one event per parent, payload carries an array → whole-row LWW, concurrent edits silently lose data.
- `book_dir_added`-style: one event per element → correct *shape*, still vulnerable to the reorder bug above.

We want every future set-typed feature to land on a single, documented rule that already works.

## Design

### Element key in payload, not `entity_id`
`entity_id` stays as `book_id` for compatibility with existing consumers. Add an explicit `element_key` string to the payload of any set-membership event. For `book_dir_*` it is `"<book_id>|<dir_id>|<file_name>"`. The element key is opaque to the conflict layer — it just needs to be a stable, deterministic string for one logical set element.

### Per-element HLC table
A small new table records the highest HLC observed (or emitted) for each element of each set-typed entity:

```sql
CREATE TABLE set_element_hlc (
    entity_type TEXT NOT NULL,    -- e.g. 'book_dir'
    element_key TEXT NOT NULL,    -- opaque, set-defined
    last_op    TEXT NOT NULL,     -- 'upsert' | 'delete'
    hlc_pt     INTEGER NOT NULL,
    hlc_l      INTEGER NOT NULL,
    client_id  TEXT NOT NULL,
    PRIMARY KEY (entity_type, element_key)
);
```

`entity_type` is a logical group (`book_dir`), distinct from the wire `entity_type` strings (`book_dir_added`, `book_dir_removed`). Map those to the same group at the apply site.

Tombstones live in this table indefinitely for now. Storage cost is trivial relative to assets; revisit only if it ever becomes measurable.

### HLC-gated apply for set-typed entities
On apply, before mutating `book_dir`:
1. Resolve `(group, element_key)` from the change.
2. Look up the row in `set_element_hlc`.
3. If missing → apply, then upsert the row with the change's `(hlc, client_id, op)`.
4. If present → compare `(incoming.hlc, incoming.client_id)` against `(stored.hlc, stored.client_id)` with `incoming_wins`. If incoming wins, apply and update the row. Otherwise drop the change.

This reuses `incoming_wins` from `SyncCommon` so the tie-break rule is identical across singletons and sets.

### Local emit path
Local writes go through triggers today. The triggers run *before* HLC is assigned to the event (HLC is assigned by `trig_event_log_hlc`). Local writes do not need to consult `set_element_hlc` because the local DB is the source of truth for its own state — the row exists or it doesn't. We only need to populate `set_element_hlc` when an event is emitted, so that future *remote* changes can be compared against it.

Approach: piggy-back on the existing event-log HLC trigger. After HLC is assigned, upsert into `set_element_hlc` for any event whose `entity_type` belongs to a set group, deriving `element_key` from the JSON payload. Keep this in SQL so it stays atomic with the event insert.

### Remote apply path
`apply_remote_change_in_tx` already centralises remote handling (`Backend/src/db.rs:381`). Add an early gate for set-typed entity types: compute `element_key` from payload, consult `set_element_hlc`, decide apply-or-drop, and update the table on apply. Existing match arms keep their effect logic; only the gate is new.

### What about `entity_id` collisions across element_keys?
None. `set_element_hlc` is keyed on `(entity_type, element_key)`. Two `book_dir_*` events for the same `book_id` but different `(dir_id, file_name)` are independent rows in `set_element_hlc`, so they do not interfere — which is exactly the union semantic we want.

## Steps

### 1. Schema
Add `set_element_hlc` table per Design, in `Backend/src/database/create_library.sql`. Migration: empty initial state is fine — existing rows in `book_dir` will get `set_element_hlc` rows the first time any event touches them. No backfill needed.

### 2. SyncCommon helpers
Add to `SyncCommon/src/lib.rs`:
- `set_group_for(entity_type: &str) -> Option<&'static str>` — maps `"book_dir_added" | "book_dir_removed" | "book_dir_deleted"` to `"book_dir"`. Returns `None` for non-set entities. The single source of truth for "is this a set?".
- `set_element_key(change: &Change) -> Option<String>` — for a set-typed change, derives the element key from `entity_id` + payload. Centralised so emit and apply agree.

This keeps the rule next to `is_singleton_event` / `incoming_wins` rather than scattered.

### 3. Local emit: populate set_element_hlc
Extend the existing event-log HLC trigger (`trig_event_log_hlc`) to also write a `set_element_hlc` row when the event's `entity_type` is in a set group. Compute `element_key` from the event payload using `json_extract` (the trigger already operates on the inserted row's `payload`). Upsert with `ON CONFLICT(entity_type, element_key) DO UPDATE` only if `(NEW.hlc_pt, NEW.hlc_l, client_id)` strictly wins.

If the SQL gets unwieldy, do it in Rust at the emit boundary instead — but keep it inside the same transaction as the event insert.

### 4. Remote apply gate
In `Backend/src/db.rs::apply_remote_change_in_tx`, before the `match change.entity_type.as_str()` block:
- If `set_group_for(&change.entity_type).is_some()`:
  - Compute `element_key` via `set_element_key(change)`. If `None`, log and drop (malformed event).
  - Look up `set_element_hlc(group, element_key)`.
  - Apply `incoming_wins` against the stored HLC + client_id (use the change's own client_id from… *see open question below*).
  - If incoming does not win, return `Ok(())` — drop the change. Do **not** stash it as pending; it is permanently superseded.
  - If incoming wins, fall through to the existing match arms; on success, upsert `set_element_hlc`.

Singletons keep their existing path; the new gate runs only for set groups.

### 5. Wire `client_id` through to apply
`incoming_wins` needs the change's originating `client_id`. `Change` does not currently carry it (only `PushChangesRequest` does, at the envelope level). Two options:
- **A. Add `origin_client_id: String` to `Change`.** Server stamps it on push; pull echoes it back. Cleanest. Requires migration of any persisted server-side change records.
- **B. Compute tie-break from a stable fallback.** E.g. include `client_id` inside the HLC's `l` or pass it via a dedicated wire field. Less clean.

Recommend **A**. Update `SyncCommon::Change`, `Server` storage, and `pull` response. Backwards compat: default to empty string on deserialize; treat empty string as a tie-break loser deterministically (so old data does not spuriously win).

### 6. Documentation
In `SyncCommon/src/lib.rs`, add a short doc comment above `Change`:

> Non-singleton entities representing **set membership** (see `set_group_for`) must carry a stable `element_key` in their payload. Apply is gated by HLC compare against the last-seen element HLC in `set_element_hlc`. New set-typed features must register their group in `set_group_for` and their key derivation in `set_element_key`.

This is the contract every future entity type follows.

### 7. Tests
- **Reorder kills resurrection.** Emit `Add(b,d,f)@HLC=1`, then `Remove(b,d,f)@HLC=2`. Apply remotely in reversed order. Assert final state is removed and `set_element_hlc` carries HLC=2 with op=delete.
- **Stale Add does not resurrect.** Apply `Remove@HLC=5`. Then apply `Add@HLC=3`. Final state still removed; `set_element_hlc` unchanged.
- **Distinct elements do not interfere.** `Add(b, d1, f)@HLC=1` and `Add(b, d2, f)@HLC=1` both apply; both rows present in `book_dir`; two rows in `set_element_hlc`.
- **Tie-break by client_id.** Same HLC, different client_id. Higher client_id wins, deterministically, on both replicas.
- **Idempotent apply.** Apply the same Add change twice. Second call is a no-op (HLC equal, not strictly winning).
- **Local emit populates table.** A local `book_dir` insert produces both an `event_log` row and a `set_element_hlc` row with matching HLC.

### 8. Cleanup
Once landed and stable, audit any future use of `entity_type` for membership-shaped features (tags, collections, highlights) and require they go through `set_group_for` / `set_element_key`. Add a debug-build assertion in `apply_remote_change_in_tx` that any non-singleton, non-set entity is record-shaped (one row per `entity_id`), so the three categories stay explicit.

## Decisions

- **`entity_id` stays as `book_id` for `book_dir_*` events.** Existing readers may depend on it. The element key is added as an extra payload field, not by reshaping `entity_id`.
- **One HLC table for all set groups, not one table per group.** Cheaper migrations and one query path. The `entity_type` column does the partitioning.
- **Tombstones retained indefinitely.** Storage cost is negligible; correctness wins. A future GC pass can prune tombstones older than the longest expected offline window once that window is defined.
- **Drop superseded changes silently.** Do not stash them as pending. They are not waiting on a missing parent — they are permanently outranked.

## Risks

- **Trigger complexity.** Doing the `set_element_hlc` upsert inside a SQL trigger may be awkward when payload extraction is non-trivial. Fallback: do it in Rust at the emit boundary, in the same transaction. Not a correctness risk, only a layering one.
- **`client_id` propagation churn.** Adding `origin_client_id` to `Change` touches the wire format, the server schema, and the pull/push paths. Minor but real. Mitigation: ship behind defaulted-empty deserialization so old records keep parsing.
- **Interaction with the dir-ID payload plan.** When `book_dir_added` switches from `dir_path`+`file_name` to `dir_id`+`file_name`, the `element_key` derivation changes (`<book_id>|<dir_id>|<file_name>` becomes natural and stable). Land this plan after, or at the same time as, the dir-ID change to avoid rewriting the key derivation twice.
- **Interaction with asset-sync.** None. Asset sync depends on `book_id` and content hash; it does not read `set_element_hlc`.

## Estimated effort
~1.5–2 days. Schema + helpers (~half day), emit-path trigger or hook (~half day), apply-path gate (~few hours), `client_id` wire-through (~half day), tests (~half day).

Independent of the asset-sync plan. Dependent on (or at least best landed after) the dir-ID payload change to avoid reworking the element-key derivation.
