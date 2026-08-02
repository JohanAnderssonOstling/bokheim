# Plan 12: Convergence test harness, tier 1 (metadata only)

Independent of plans 6–11 but should land **before or alongside** them: each of those modifies
resolution-relevant code, and this harness replaces hand-proving convergence with a property
test. Prerequisite for plan 13 (tier 2 adds disk).

## Property under test

After every device syncs to quiescence, all devices hold identical library state — for *any*
sequence of local operations, any concurrency, any sync interleaving, any offline periods.
Existing tests (`remote_book_dir_add_remove_converge_regardless_of_order` etc. in
`Backend/src/db.rs`) check hand-picked interleavings; this generates thousands.

## Architecture (all in-process; no network, no real server)

New integration test `Backend/tests/convergence.rs` + a small `fake_server` module.

1. **Simulated device**: in-memory SQLite via `db::init_db` (own `client_id`), as the existing
   db.rs tests already do. Local mutations go through real SQL so the real triggers fire and
   the real HLC stamps. Push = `db::get_unpushed_events` + `db::mark_events_pushed`;
   pull = real `db::apply_remote_change_in_tx` inside a transaction with
   `defer_foreign_keys = ON` (mirror `pull_events_from_server`, `Backend/src/sync.rs`), cursor
   in `sync_metadata` like production.
2. **Fake server** (~100 lines over `Vec<EventRow>`), mirroring `Server/src/db.rs`:
   - append insert deduped by `(client_id, client_seq)`;
   - singleton upsert keyed `(entity_id, event_type)`, overwritten only by strictly greater
     `(hlc_pt, hlc_l, client_id)`, **re-drawing server_seq** on overwrite;
   - fetch: `server_seq > cursor AND client_id != caller`, ordered, optional `limit` (to
     exercise pagination once plan-borrow #2 lands).
   - **While building this, extract the singleton comparison into `SyncCommon`** (e.g.
     `fn singleton_wins(incoming: (Hlc, &str), stored: (Hlc, &str)) -> bool`) and use it from
     the real server, the fake server, and `should_skip_singleton_apply` — one rule, three
     callers, drift impossible.
3. **Operation generator** (seeded RNG): create/rename/move/delete dir (incl. occasionally a
   deleted or nonexistent target), add/remove placement, update reading position, open book.
   Arguments drawn from the device's current state. Dir ops via `upsert_dir`/`delete_dir`;
   placements via the same SQL the scanner uses (`insert_book_dir`,
   `mark_book_dir_deleted`-style updates) so triggers fire.
4. **Scheduler**: each step the RNG picks a device and one of {local op, push, pull, full sync,
   nothing}. "Nothing" for long stretches = offline device. Fault injection knobs:
   - a push round-trip that reports transient rejection (exercises the plan-1 guard);
   - pull served in pages of random size;
   - per-device HLC wall-clock skew.
5. **Quiescence + assertion**: run sync rounds until fixpoint (no unpushed events anywhere, no
   pull returns rows), then dump canonical state per device and assert byte-identical:
   - `dir`: (id, parent_id, name, deleted_at IS NULL), sorted;
   - live placements: (dir_id, book_id, file_name, content_hash), sorted;
   - `book`: (id, read_pos, last_read) for books with live placements.
   Mid-run invariants after every commit: `PRAGMA foreign_key_check` empty; `__root__` intact.

## Shrinking

Use `proptest` (dev-dependency): model the run as a `Vec<Step>` strategy; on failure proptest
shrinks to a minimal counterexample (typically 2–4 ops) and persists the seed in
`proptest-regressions/`. Every shrunk failure also gets hardcoded as a named regression test.

## Scope notes

- Tier 1 has **no filesystem and no scanner** — placements are driven directly via SQL. Disk
  reconciliation bugs (materialization, markers) are plan 13's job.
- Books: use synthetic ids/content hashes (`format!("{i:064x}")`); no real EPUBs.
- Keep runtime in normal `cargo test` budget: default ~256 cases × ~200 steps × 3 devices;
  a `#[ignore]`d "deep" variant runs 10k cases.

## Acceptance

1. Harness passes on current code at depth (≥10k cases locally once).
2. Re-introducing a known historical bug makes it fail with a small shrunk case — verify by
   temporarily (a) removing the remove-wins tie-break in `apply_remote_book_dir`,
   (b) disabling `should_skip_singleton_apply` while enabling transient-rejection injection.
3. `singleton_wins` is the single comparison implementation used by server, fake server, and
   client guard (grep proves no other tuple comparison remains).
