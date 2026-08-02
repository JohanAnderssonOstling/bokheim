# Plan 5: Cascade dir deletion through the subtree

Requires plan 4 (children emit `dir_state` with `deleted`); benefits from plan 3 (folder removal).

## Problem

`delete_dir` → `mark_dir_deleted` (`Backend/src/database/library.sql`) tombstones only the one
dir row. Child dirs and `book_dir` placements underneath stay live (`deleted_at IS NULL`) but
become unreachable through the path CTEs (which filter deleted dirs). Inconsistencies: the
carousels' `EXISTS(live placement)` checks still count those books (un-openable UI entries), and
no events are emitted for the subtree, so other devices never delete the child placements/files.

## Design

Cascade **locally at the point of deletion**, per entity, and let the existing per-row triggers
broadcast it. The remote DB side needs no cascade: the origin device emits one event per affected
entity, and each is applied/resolved independently (placements through the existing HLC gate,
child dirs as `dir_state` singletons).

Per-entity resolution is deliberate: dir rows and book placements resolve independently. A
concurrent move of a child directory can rescue the directory row itself if its `dir_state` wins,
but it does **not** automatically rescue descendant `book_dir` placements. Those placements survive
only if their own `book_dir_added` / `book_dir_removed` HLC resolution keeps them live.

## Changes

1. **Collect the subtree** (new query): recursive CTE from `:dir_id` down via `parent_id`,
   live dirs only.
2. **Collect file paths to delete locally, before tombstoning**: live, downloaded placements in
   the subtree with their resolved paths (reuse the `dir_paths` CTE/view pattern). Paths must be
   full library-relative paths, not paths relative to the deleted subtree root: deleting
   `/Parent/A` must collect `Parent/A/file.epub`, not `A/file.epub`. The local device must delete
   its own files — `mark_vanished_book_dirs_deleted` must not be the thing that notices, and remote
   devices handle theirs via the `book_dir_removed` events + `apply_local_removals`.
3. **Tombstone placements** in the subtree (`UPDATE book_dir SET deleted_at = :ts WHERE dir_id IN
   subtree AND deleted_at IS NULL`). Fires `trig_book_dir_deleted` per row → `book_dir_removed`
   events, each HLC-stamped by `trig_event_log_hlc` (this nested-stamp path already works for
   multi-row updates — see `mark_vanished_book_dirs_deleted`).
4. **Tombstone dirs** in the subtree (children and the target). With plan 4, each row fires the
   dir update trigger → `dir_state {deleted: 1}` singleton per dir.
5. **Delete local files** for the collected paths, then remove markers + empty folders bottom-up
   (`fs::remove_dir`, non-recursive; leftovers are the user's problem and the scanner's grace
   window handles stragglers).
6. Wire this as the implementation of `db::delete_dir` so all callers (UI delete) get the cascade.
   Use a real timestamp for `deleted_at` (micros, like the scanner sweep), not the current
   literal `1`.

Remote apply still materializes filesystem deletions. It should collect all `dir_state{deleted}`
paths from the pull batch and remove markers/directories after commit in deepest-first path order.
Do not rely on event order from the multi-row local `UPDATE`; SQLite trigger order for affected
rows is not a durable contract.

## Ordering note

Tombstone placements before dirs or after — triggers don't care — but collect paths *before* any
tombstoning (the path CTE filters deleted dirs, so resolving after would return nothing).

## Tests

1. Tree root→A→B with books in A and B: delete A → A, B, and both placements tombstoned; event_log
   has one `dir_state{deleted}` per dir and one `book_dir_removed` per placement; files gone;
   carousels (`get_recently_read`/`count_all_books`) no longer count the books.
2. Sibling dir untouched.
3. Re-creating a same-named dir afterwards works (new uuid, no PK collisions).
4. Nested delete path correctness: root→Parent→A with a file in A; delete A → physical removal
   targets `Parent/A/file`, not `A/file`.
5. Remote materialization order: apply deleted dir events for A and A/B in parent-first order;
   after post-commit materialization both folders are removed.
6. Concurrent move semantics: delete A while another device moves A/B out. Assert the chosen
   behavior explicitly: the B dir may survive if its `dir_state` wins, but placements under B
   remain governed by their own `book_dir` HLCs.
