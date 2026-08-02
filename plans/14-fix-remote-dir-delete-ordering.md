# Plan 14: Fix remote dir-delete materialization ordering (bug, from plan 1–5 review)

Do before plan 13 (its acceptance criteria reference this bug). Atomic fix in
`Backend/src/sync.rs`.

## Bug

`materialize_remote_dir_deletes` processes deletes in **batch order**, but re-drawn singleton
order is arbitrary. Parent-before-child: `remove_dir(parent)` fails (non-empty) *after its
marker was already removed* → next scan finds a marker-less folder, mints a fresh uuid, and
re-emits the dir — a remotely deleted dir resurrects as an empty dir on every device.
(The local cascade is correct: `get_dir_delete_dir_paths` orders `depth DESC`.)

## Fix

1. Sort `dir_deletes` by path depth descending (component count of `old_path`) before
   processing — children removed before parents.
2. Remove the marker **only after** `fs::remove_dir` succeeds; on failure (non-empty because
   of stray user files — legitimate), leave the marker in place so the scanner keeps the
   existing identity instead of minting a new one.

## Tests

1. Unit: deletes for `/A` and `/A/B` given in parent-first order → both folders removed.
2. Non-empty folder (stray file): folder remains **with marker intact**; a follow-up scan
   emits no new dir identity (assert no `dir_state` event for a fresh uuid).
