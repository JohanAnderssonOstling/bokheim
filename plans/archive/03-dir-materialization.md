# Plan 3: Materialize remote dir changes to disk + write `.biblos_uuid` markers

Independent of plans 1–2. Hard prerequisite for plan 4 (dir sync cannot work without it under
either event model). Fixes two existing bugs.

> **STATUS**: partially done. `db::materialize_dir_with_markers` exists and is called at the
> blob-download site (`transfer.rs` `download_blob`), and post-download fingerprint stamping
> exists (`db::stamp_book_dir_local_hash_by_rel_path`, called in `download_blob`; the later
> typed-transfer work fixed its state-driven book_id-vs-hash mismatch).
> Still missing: markers at the copy-materialization and relocation sites (`sync.rs` pass 2 and
> `apply_local_removals` still use bare `create_dir_all`), remote dir move/delete
> materialization (change 2 and 3 below), and stamping at the copy/relocate sites (change 4).

## Architecture rule being enforced

On each device the **disk is authoritative for what the scanner observes**, and the scanner writes
disk state back into the DB (emitting sync events, since `change_origin` is `local` during scans).
Therefore every synced change must be materialized to disk, or the next scan reverts it and
re-emits the revert. The placement path already does this (`apply_local_removals` in
`Backend/src/sync.rs` — see its comment). The dir path does not.

Dir identity lives in a `.biblos_uuid` marker file *inside* each folder: the scanner reads it, or
mints a new uuid if absent (`Backend/src/scanner.rs` ~line 174).

## Bug A: remote dir moves are never materialized → scanner ping-pong

A remote `dir_moved` only updates the DB row. The next scan finds the folder at its old path with
its old uuid, `upsert_dir` writes the old parent/name back, and `trig_dir_moved` fires → a revert
event is pushed. The originating device's scanner then re-asserts the move. The two scanners emit
contradicting events forever.

## Bug B: materialized folders lack markers → duplicate dirs propagate

Only the scanner writes `.biblos_uuid` (verified: single grep hit). But downloads and the
copy-materialization pass create parent folders with bare `create_dir_all`
(`Backend/src/transfer.rs` ~227 and ~313, `Backend/src/sync.rs` ~196 and ~459). Next scan: unmarked
folder → fresh uuid → duplicate `dir` row with the same name → and because the downloaded file's
fingerprint was never stamped on the placement, the scanner hashes it and inserts a duplicate
placement under the new dir, emitting `dir_added` + `book_dir_added` that replicate the duplicates
to every device.

## Changes

1. **Marker-aware dir creation helper.** New helper (suggest `Backend/src/sync.rs` or a small
   module): given a placement's `dir_id`, walk up the `dir` table via `parent_id` collecting
   `(uuid, name)` root→leaf (new SQL query, recursive CTE like `get_book_file_path_components`),
   then create each folder level under `library_path` and write `.biblos_uuid` with the synced
   uuid if the marker is missing. Replace the bare `create_dir_all` calls at all four
   materialization sites with it. (`.bokheim*` files are already excluded from scans by the
   dot-prefix filter, scanner.rs walkdir.)
2. **Materialize remote dir upserts.** In the pull flow: when applying a remote dir add/move,
   resolve the dir's library-relative path from the *current* row **before** the upsert (None if
   the row doesn't exist yet) and again after. Collect `(old_path, new_path)` pairs in apply order
   (mirror the `removed_placements` pattern in `pull_events_from_server`); after commit:
   - old None, new Some → create folder + marker (helper from step 1).
   - both Some, different → `fs::create_dir_all(new.parent)`, then `fs::rename(old, new)`. The
     marker and files travel with the rename; fingerprints (mtime/size/inode) survive, so the next
     scan is a no-op. On failure, log and do nothing — the scanner will revert the move and
     re-emit, which is self-consistent.
3. **Materialize remote dir deletes.** When a remote dir delete applies: after
   `apply_local_removals` has handled the files, delete the folder's `.biblos_uuid` and call
   `fs::remove_dir` (non-recursive — a folder still holding unexpected user files safely refuses;
   the scanner/grace machinery handles whatever remains).
4. **Stamp the fingerprint after download.** After a blob download lands
   (`transfer.rs`, post `tokio::fs::rename(tmp, local)`), compute the same fingerprint the scanner
   uses (see `get_book_dir` / the hashing in `scanner.rs`) and write it to the satisfied
   placement's `local_hash` (new SQL: `update_book_dir_local_hash` keyed by the path triple).
   No trigger fires on `local_hash` updates (triggers watch `deleted_at`/`dir_id`/`file_name`),
   so this is sync-silent. The next scan then takes the cheap skip path instead of re-hashing and
   re-inserting. Apply the same stamping in the copy-materialization pass (sync.rs pass 2) and the
   relocation path (`apply_local_removals`).

## Tests

Tempdir + in-memory DB:

1. Helper writes nested folders with correct markers; idempotent on second call.
2. Remote dir move: seed dir+folder+marker, apply remote move, assert folder renamed and marker
   uuid unchanged; assert a subsequent `upsert_dir` with the scanned (new) values is a no-op
   (no new event_log row) — this is the anti-ping-pong property.
3. Download stamping: after stamping, `get_book_dir_by_hash` with the file's fingerprint returns
   the placement.
