# Plan 15: Fix orphaned-chain path truncation (bug, from plan 1–5 review)

Atomic fix in the current database path-resolution and materialization code.

## Bug

`get_dir_path_components` walks upward from `:dir_id`, requiring `deleted_at IS NULL` on every
ancestor. If an ancestor is soft-deleted (e.g. tombstoned earlier in the same pull batch), the
recursion stops mid-chain and returns a **partial** component list. `dir_path_from_components`
builds a path from whatever it gets, so an orphaned dir resolves to a rootward-truncated path
(`"Child"` instead of `"Parent/Child"`) — indistinguishable from a real root-level dir.
Materialization can then create/rename folders at wrong locations.

## Fix

Return None for orphans: also select `parent_id` per component and, in
`collect_dir_path_components`/`dir_path_from_components`, verify the **topmost** collected
component's `parent_id == '__root__'`; otherwise treat the path as unresolvable (None).
Audit the same pattern in `get_book_file_path_components` and apply the same guard if it is
still in use.

## Tests

1. Chain root→A→B with A tombstoned: `get_dir_rel_path_tx(B)` → None (today: `Some("B")`).
2. Live chain unchanged: root→A→B → `Some("A/B")`.
3. Pull-batch integration: batch = [tombstone A (dir_state deleted), dir_state update for B] →
   no folder created at library root named `B`.
