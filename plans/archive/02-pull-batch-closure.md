# Plan 2: Pull-batch dependency closure (deferred FKs, remove skip-and-drop guards)

Independent. Hard prerequisite for plan 4 (`dir_state` singletons re-order the pull stream).

> **STATUS**: step 1 is already done — the pool's `with_init` sets `foreign_keys = ON`
> (`Backend/src/library.rs`, `SqliteConnectionManager::with_init`). Verify, then start at step 2.

## Background: the closure invariant

The pull (`pull_events_from_server`, `Backend/src/sync.rs`) applies a whole batch in one
transaction and advances `last_pull_server_seq` on commit. Invariant: **every batch is
dependency-closed relative to what the client already has.** For any event referencing dir `D`,
either the client already applied some version of `D` (its server_seq was ≤ the cursor), or `D`'s
current row has seq > cursor and is in this same batch. This holds because dir rows on the server
are never removed and overwrites only move their seq forward. Soft-deleted local dirs are valid FK
targets (see the `dir_exists` comment in `Backend/src/db.rs`).

What is NOT guaranteed — and what plan 4 makes worse — is **within-batch order**: a placement or
child dir can precede the dir it references inside one batch (singleton overwrites re-draw
`server_seq` to the end of the stream).

Today's code handles missing dirs by *skipping the event with a log line*
(`apply_remote_book_dir` and `apply_remote_dir_upsert` in `Backend/src/db.rs`). Appends are
delivered exactly once, so a skip is silent permanent data loss.

## Changes

1. **Verify/enable FK enforcement on pool connections.** SQLite's `foreign_keys` pragma is
   per-connection and OFF by default. Find where the `r2d2_sqlite` pool is created (likely
   `Backend/src/library.rs`) and ensure every connection runs `PRAGMA foreign_keys = ON` (e.g.
   `SqliteConnectionManager::...with_init(...)`). Without this, step 2's "fail loudly" is a no-op.
2. **Defer FK checks inside the pull transaction.** In `pull_events_from_server`, right after
   opening the transaction: `PRAGMA defer_foreign_keys = ON` (via `tx.pragma_update`). SQLite
   then checks FKs at commit instead of per-statement; the pragma auto-resets at transaction end.
   Within-batch order stops mattering.
3. **Delete the skip guards.** Remove the `dir_exists` check in `apply_remote_book_dir` and the
   parent-existence check in `apply_remote_dir_upsert` (keep the empty-name/empty-filename
   guards). Remove the now-unused `dir_exists` helper. A genuine dependency violation (server log
   corruption) now fails the commit, the cursor doesn't advance, and the next pull retries —
   visible and blocking instead of silently absorbed.
4. **Document the invariant** in a comment on the pull: pages/batches must be applied in
   server_seq order and the cursor must only advance on commit; future pagination must preserve
   this or closure breaks. Also note the known hole: the server's `fetch_changes` skips
   unparseable events (`Server/src/db.rs`), which can punch a hole in closure — in that case the
   client pull wedges loudly until the server row is fixed, which is intended.

## Tests

In-memory, with `PRAGMA foreign_keys = ON` on the test connection:

1. Within one tx with `defer_foreign_keys=ON`: apply a remote `book_dir_added` for a dir that
   doesn't exist yet, then apply the dir upsert, commit → succeeds; placement present.
2. Same but never create the dir → commit fails with FK violation (proves loud failure).
3. Regression: placement into a *soft-deleted* dir still applies (FK target exists).
