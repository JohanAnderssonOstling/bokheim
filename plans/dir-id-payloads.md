# Dir IDs in sync payloads

## Goal
Switch the sync protocol from path-keyed dir references to UUID-keyed dir references. Eliminate path resolution from the event-apply path, collapse the `pending_remote_changes` stash to its true causal-dependency role, and make independent same-name dir creates explicit (and resolvable) instead of silently merged.

## Non-goals
- No change to `book.id` semantics (already UUID, already global).
- No change to the on-disk `.biblos_uuid` mechanism — it stays as the disk↔DB binding.
- No new transport, auth, or asset sync work.

## Background
Today dir references travel as path strings:
- `dir_added` payload: `{ path }`
- `dir_moved` payload: `{ old_path, new_path }`
- `dir_deleted` payload: `{ path }`
- `book_dir_added` payload: `{ dir_path, file_path, file_name, ... }`

On apply, `Backend/src/db.rs:516 find_dir_id_by_path` looks up the dir by path; on miss, the event is stashed (`Backend/src/db.rs:390-470`). This is the dominant source of stashing: the protocol throws away the originating client's `dir.id` and forces every receiver to re-resolve by path.

Crucially, **dir IDs are already global**. The scanner writes a `.biblos_uuid` sidecar in every directory on disk (`Backend/src/scanner.rs:157-166`) and uses that UUID as `dir.id`. The same physical directory has the same UUID across devices — *if* the protocol propagates it. Today it doesn't.

## Data model changes

### Event payload schema (SyncCommon)

| Event | Old payload | New payload |
|-------|-------------|-------------|
| `dir_added` | `{ path }` | `{ dir_id, parent_id, name }` |
| `dir_moved` | `{ old_path, new_path }` | `{ dir_id, new_parent_id, new_name }` |
| `dir_deleted` | `{ path }` | `{ dir_id }` |
| `book_dir_added` | `{ dir_path, file_path, file_name, book_id, ... }` | `{ dir_id, book_id, file_name, ... }` |
| `book_dir_removed` / `_deleted` | `{ dir_path, file_name, book_id }` | `{ dir_id, book_id, file_name }` |

`entity_id` for dir events becomes the `dir_id` directly (mirrors how `book_added` already uses `entity_id == book_id`).

### New event: `dir_merged`
Emitted when same-`(parent_id, name)` collision is detected. Payload: `{ winner_id, loser_id }`. Receivers rewrite all references from `loser_id` to `winner_id`, including:
- `dir.parent_id` of children
- `book_dir.dir_id` of contained books
- `.biblos_uuid` on disk (next scanner pass — see step 5)

The loser dir row is deleted (or tombstoned, post #4 cleanup) once references are migrated.

### No DB schema changes
`dir.id` is already a UUID column. `book_dir.dir_id` already FKs to it. The scanner already writes `.biblos_uuid`. Only the event payloads change.

## Steps

### 1. SyncCommon payload types
Replace the loose `serde_json::Value` payload accessors with typed payload structs per event type, or at minimum add helper constructors that emit the new shape. Keep `payload: serde_json::Value` on the wire for now (avoids a deeper refactor), but route all writes through helpers so no caller can forget to include `dir_id`.

### 2. Backend emit path
In `Backend/src/database/triggers.sql`, the triggers that write `dir_added`, `dir_moved`, `dir_deleted`, `book_dir_added`, `book_dir_removed` to `event_log` need to switch from emitting paths to emitting IDs. This is mostly easier than today — triggers already have `NEW.id`, `NEW.parent_id`, `NEW.name` directly available, no JOIN or path-construction needed.

Drop the path-construction logic. The `dir_paths` view (referenced at `Backend/src/db.rs:65`) is no longer needed by the event emit path; check whether it's used elsewhere before removing.

### 3. Backend remote apply path
In `Backend/src/db.rs:apply_remote_change_in_tx` (line 358), rewrite each event handler:

```rust
"dir_added" => {
    let dir_id   = payload["dir_id"].as_str()?;
    let parent   = payload["parent_id"].as_str()?;
    let name     = payload["name"].as_str()?;
    detect_and_merge_collision(tx, dir_id, parent, name)?;
    tx.upsert_dir(dir_id, parent, name)?;
}
"dir_moved" => {
    let dir_id     = payload["dir_id"].as_str()?;
    let new_parent = payload["new_parent_id"].as_str()?;
    let new_name   = payload["new_name"].as_str()?;
    // Stash only on FK miss — true causal dependency, not path mismatch.
    if !tx.dir_exists(new_parent)? { stash_pending_remote_change(tx, change)?; return Ok(()); }
    tx.upsert_dir(dir_id, new_parent, new_name)?;
}
"dir_deleted" => {
    let dir_id = payload["dir_id"].as_str()?;
    tx.mark_dir_deleted(dir_id)?;
}
"book_dir_added" | ... => {
    let dir_id = payload["dir_id"].as_str()?;
    if !tx.dir_exists(dir_id)? { stash_pending_remote_change(tx, change)?; return Ok(()); }
    // proceed
}
```

Delete `find_dir_id_by_path`, `ensure_dir_for_path`, `split_dir_path`, `split_file_path`, `normalize_dir_path` once nothing in the apply path calls them (`Backend/src/db.rs:508-560`).

### 4. Same-name collision merge

`detect_and_merge_collision(tx, incoming_id, parent_id, name)`:
1. Query `SELECT id FROM dir WHERE parent_id = ?1 AND name = ?2 AND id != ?3 AND deleted_at IS NULL`.
2. If a row exists, this is a collision. Pick the winner deterministically: lower `id` lex (or, post-HLC, the row whose creation event has the lower HLC).
3. Migrate references from loser to winner:
   - `UPDATE dir SET parent_id = winner WHERE parent_id = loser`
   - `UPDATE book_dir SET dir_id = winner WHERE dir_id = loser`
4. Delete loser dir row.
5. Emit a local `dir_merged { winner_id, loser_id }` event so other devices converge.
6. Mark the loser's on-disk `.biblos_uuid` for rewrite (see step 5).

Apply the same logic in `apply_remote_change_in_tx` for incoming `dir_merged` events: rewrite local references and queue `.biblos_uuid` rewrite.

### 5. Scanner reconciliation

In `Backend/src/scanner.rs:157-166`, extend the dir-handling logic:

```rust
let on_disk_uuid = read_or_generate_biblos_uuid(&entry);
let parent_uuid  = uuid_stack.last().unwrap();
let name         = entry.file_name().to_str().unwrap();

// Case 1: matches existing DB row by id → upsert and continue.
// Case 2: DB has a row at (parent, name) with a different id → collision.
//   This dir was created independently here while another device created
//   it elsewhere. Merge: rewrite .biblos_uuid on disk to the canonical id,
//   migrate refs, emit dir_merged.
// Case 3: no DB row matches either id or (parent, name) → fresh dir,
//   normal upsert path.
```

Maintain a small queue of `.biblos_uuid` files to rewrite outside the DB transaction (file IO under a DB lock is bad). Process it after commit.

### 6. Migration

**Backend.** Existing local DBs have dir rows already keyed by UUID — no row migration needed. The `event_log` table contains old-format events (path-based payloads); since the project is pre-users, drop and re-emit by walking the current `dir` and `book_dir` tables to synthesize fresh `dir_added` / `book_dir_added` events with the new payload shape. Bump `last_pushed_local_seq` to skip re-pushing them on already-synced clients (or accept the re-push — server dedups by `(client_id, client_seq)`).

**Server.** `library_events` rows carry old-shape payloads in their `data` JSON. Easiest path: truncate `library_events` and re-bootstrap from clients. Pre-users — cheap.

If you want to preserve history: write a one-shot migration that rewrites `data` payloads by joining against a "path → dir_id" map reconstructed from the same event log. Doable but not worth the effort at this stage.

### 7. Tests

- Two-device simulation: A creates `/Music`, syncs, B creates `/Music/Jazz`. Assert B's `Jazz` event applies without stashing.
- Independent same-name: A and B both create `/Music` offline. After sync, assert both DBs converge to one `Music` dir with the same `dir_id`, references migrated, `dir_merged` event present.
- Move under not-yet-pulled parent: B receives `dir_moved` whose `new_parent_id` doesn't yet exist locally. Assert it's stashed, then drains correctly when the parent's `dir_added` arrives.
- Scanner reconciliation: pre-create `/Music/` on B's disk with a fresh `.biblos_uuid`, then deliver A's `dir_added` for `/Music` with a different UUID. Assert merge happens at next scan, `.biblos_uuid` is rewritten to the canonical value.

### 8. Cleanup
- Delete `find_dir_id_by_path`, `ensure_dir_for_path`, `split_dir_path`, `split_file_path`, `normalize_dir_path` from `Backend/src/db.rs`.
- Drop the `dir_paths` view (`Backend/src/db.rs:65`) if no other consumer remains. (Check `library_page.rs`, `home_page.rs`, scanner — any path-based UI navigation may still want it; if so, keep it but mark as UI-only.)
- Update doc comments referring to path-keyed events.

## Risks / open questions

- **Children-before-parent sync.** Possible if `dir_moved` arrives at B before B has the new parent dir. This is now a *true* causal dependency (no path-resolution false positives), so the stash mechanism handles it cleanly. The TTL/orphan-fallback work from the stash plan still applies.
- **Loser's `.biblos_uuid` rewrite.** The disk file rewrite happens outside the DB transaction. If the process dies between commit and disk rewrite, the next scan re-detects the collision and re-merges idempotently. Make sure the merge logic is in fact idempotent (`UPDATE ... WHERE id = loser` is a no-op if loser already gone).
- **Race between scanner and sync apply.** Both can detect a collision and emit `dir_merged`. Two `dir_merged` events with reversed winner/loser would oscillate. Mitigation: the winner-pick is deterministic (lex-min UUID), so both sides emit the *same* `dir_merged`, and the second is a no-op. Test this explicitly.
- **`dir_paths` view consumers.** If UI code uses it for breadcrumbs or navigation, it stays. Just stops being on the sync hot path.
- **HLC plan interaction.** Both plans reshape event payloads. Either land them together in a single protocol break, or order them HLC-first then dir-IDs (each is a separable change). Doing them together saves one migration.

## Estimated effort
~1–2 days. The mechanics are straightforward; the collision/merge logic is the only subtle piece. Bundling with the HLC plan adds maybe half a day on top of either alone.
