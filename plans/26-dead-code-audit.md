# Plan 26: Dead/duplicate code audit (Backend/src/db.rs and friends)

Atomic, mechanical. Each dead query is a stale behavior assumption waiting to be "fixed".

Procedure: for each item, grep the whole workspace (Backend, UI, MobileApp) for callers;
delete the Rust function **and** its SQL block if unreferenced; if referenced, leave a doc
comment naming the caller.

Candidates (verify, don't trust this list blindly):

- `get_events_after_local_seq` (library.sql + no Rust wrapper found) — superseded by
  `get_unpushed_events`.
- `get_books_without_title` / `get_book_paths_without_title` — extraction now happens inline
  in the hash worker; plan 6 *changes* this query, so resolve audit vs plan 6 first (if plan 6
  lands, it's alive again).
- `delete_book_metadata` / `book_has_remaining_paths` — check for a UI deletion flow.
- `insert_book_with_title` / `update_book_title` — import path that may no longer exist.
- `get_book_file_path` vs `get_book_file_path_as_pathbuf` — near-duplicates; keep one.
- `delete_unclaimed_transfer_jobs` — superseded by `prune_unclaimed_transfer_jobs`?
- `get_blob_any_path` vs `get_blob_local_path` — verify both call sites still exist.
- `generate_file_hash` (library.rs) — already listed in plan 10; coordinate.
- `get_cover_path` (library.rs) — already listed in plan 7; coordinate.
- Legacy migration helpers in `init_db` (`event_kind` drop, `pending_remote_change` drops,
  `migrate_book_dir_primary_key`) — pre-release, all devices migrated: collapse into the
  plan 11 `user_version` rework rather than deleting piecemeal.

Acceptance: `cargo build` + tests green; grep shows no orphaned `-- name:` SQL blocks (every
named query has a caller).
