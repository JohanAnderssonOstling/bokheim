# Directory causal dependencies via auto-create + path-collision merge

## Goal
Stop events from being silently lost when they arrive at the server in causal-violating order across devices. Replace the current stash-and-drain machinery for directory parents with two simpler mechanisms: auto-create missing parent dirs on demand, and merge dirs that end up sharing the same canonical path.

## Non-goals
- No changes to content-shaped parent dependencies (e.g., `reading_position_updated` arriving before `book_added`). Those keep their current behavior or grow their own rule later.
- No deterministic-from-path `dir_id` scheme. Considered and rejected — see Decisions.
- No changes to HLC ordering, push idempotency, or set-membership conflict handling. Those have their own plans.

## Background

### The headline bug: silent server drop

`Server/src/db.rs::insert_changes` (`db.rs:43-128`) loops per change and calls `causal_guard_allows` (`db.rs:130-167`). When the guard returns `false`:

```rust
if !causal_guard_allows(tx, library_id, change).await? {
    continue;
}
```

The change is skipped — no row inserted, no `server_seq` allocated. The response is still 200 OK. The client's `mark_events_pushed` then marks the change as pushed locally based on `max_local_seq`. **The change is lost forever, on every device.**

Concrete trigger: client B (offline) queues `book_dir_added(b, dir_path=/Music/Jazz, file_name=…)` referencing a dir that client A created. If B's push reaches the server before A's `dir_added(/Music/Jazz)`, B's change gets silently dropped. When A's `dir_added` arrives later, the dir exists everywhere but B's book never appears in it — anywhere.

### The current client-side compensation

`Backend/src/db.rs::apply_remote_change_in_tx` (`db.rs:381-493`) calls `stash_pending_remote_change` in 5 places, all in `dir_moved` and `book_dir_added` arms (`db.rs:393, 398, 402, 408, 471`). Stashed changes live as a JSON blob in `sync_metadata['pending_remote_changes']` (`db.rs:619-632`) and are drained once per pull via `drain_pending_remote_changes_in_tx` (`db.rs:531-542`).

This compensates for the *pull* side of causal violations but doesn't help the push side, where the server has already dropped the change. Coverage is also patchy: `book_dir_removed` and `book_dir_deleted` paths don't stash — they silently no-op if the dir is unknown (`db.rs:469-481`), which is a smaller version of the same lossy bug.

### The local hierarchy already has implicit creation

`ensure_dir_for_path` (`db.rs:563-583`) walks a path and creates intermediate `dir` rows on demand when handling `dir_added`. The codebase already accepts "create the dir hierarchy implicitly when a path needs it." Extending that idea to "create the dir on demand whenever any child event references an unknown path" is consistent with existing local semantics.

### Why not deterministic IDs from path

The natural-seeming alternative is to derive `dir_id = uuid_v5(NAMESPACE, canonical_path)`. Two devices creating `/Music/Jazz` independently would land on the same id and merge automatically with no extra logic.

This breaks on the **create/rename/recreate** sequence:

- t=1 `dir_added(path=/Music/Jazz, id=X = v5("/Music/Jazz"))`
- t=2 `dir_moved(/Music/Jazz → /Music/Bebop, id=X)` — id stays, dir now at `/Music/Bebop`
- t=3 `dir_added(path=/Music/Jazz, id=???)` — and `v5("/Music/Jazz")` is still X

Either the new create collapses with the renamed dir (one row, two distinct user-intent dirs collapsed — silent data confusion) or the duplicate id is rejected (user's local action silently fails to propagate). No salting trick (HLC, slot count, nonce) preserves both the offline-same-path-merge property and the rename-recreate-distinctness property.

Conclusion: keep `dir_id` as a randomly generated UUID; do the merging at apply time on path collision instead.

## Design

### Canonical path

A single function in `SyncCommon`:

```rust
fn canonical_dir_path(input: &str) -> Option<String>
```

Rules:
- Apply Unicode NFC normalization.
- Case-sensitive (do not lowercase). Document the consequence: `/Photos` and `/photos` are different folders.
- Strip exactly one leading slash, no trailing slash, collapse runs of slashes.
- Reject `..` and `.` components and embedded NUL — return `None`.
- Empty input or `/` returns `Some("")` representing the root.

Used by every emit path that writes a path into a payload, and by every apply-time path-collision check.

### Auto-create on missing parent

Replace each `stash_pending_remote_change` call in dir-dependent arms with auto-create. When applying a remote change whose payload references a path that doesn't currently resolve to a `dir` row:

1. Compute the canonical path. If it fails canonicalization, log and drop the change (malformed).
2. Walk the canonical path, creating intermediate `dir` rows on demand with freshly-generated random UUIDs. Mark them with `auto_created = 1` so debugging can identify synthesized rows.
3. Use the resulting `dir_id` as the parent and proceed with the change's effect.

The auto-create is silent — it does not emit a `dir_added` event. The dir's existence is implied by the child event; if the user explicitly created the dir on another device, that device's real `dir_added` will arrive later and the path-collision merge will reconcile.

### Path-collision merge

When applying a `dir_added` or `dir_moved` post-image, after writing the row, check whether another live (non-tombstoned) `dir` row now shares the same canonical path. If so, merge:

- **Pick canonical**: the dir whose creating event has the lower HLC wins. Tie-break: lower `dir_id` lexicographic.
- **Rewrite references**: every `book_dir` row pointing at the loser is updated to point at the canonical id, with `(dir_id, book_id, file_name)` collisions resolved via the set-membership HLC rule (which keeps the higher-HLC version).
- **Tombstone the loser**: the loser's `dir` row is marked deleted with HLC = `max(loser.hlc, winner.hlc) + epsilon`. The loser's `dir_id` is recorded in a `dir_alias` table mapping old id → canonical id, so any late-arriving event referencing the old id resolves correctly.

Auto-created rows participate in this merge identically to explicit creates. When the real `dir_added` arrives, it shares the path of the auto-created row; the auto-created row almost always loses (its HLC is the child's, which is later than the real parent's HLC); references rewrite to the real id; auto-created row tombstones.

### Server-side: stop the silent drop

Replace the `causal_guard_allows` skip in `Server/src/db.rs::insert_changes` with the same auto-create rule. When a child event arrives whose parent dir doesn't exist in `library_events`:

- Synthesize a `dir_added` event for the missing path, with `client_id = "__server_synth__"`, a fresh random `dir_id`, and HLC = `(child.hlc.pt, child.hlc.l.saturating_sub(1))`. Insert it before the child.
- Insert the child as normal.

Other clients pulling later see the synth event and apply it like any `dir_added`. When the real client's `dir_added` for the same path arrives, it gets a real `server_seq`, replicates out, and the path-collision merge runs on every client (and on the server, if you want a server-side index of path → canonical id — see Open question below).

The `__server_synth__` client_id is reserved and rejected on push (no real client may claim it).

### dir_alias table

```sql
CREATE TABLE dir_alias (
    old_dir_id TEXT PRIMARY KEY,
    canonical_dir_id TEXT NOT NULL,
    merged_at INTEGER NOT NULL
);
```

Late-arriving events referencing `old_dir_id` (e.g., a `book_dir_added` from a device that hasn't yet seen the merge) resolve via this table. On apply: if `dir_id` in payload is in `dir_alias`, substitute `canonical_dir_id` before proceeding.

### Coverage of every dir-dependent arm

- `dir_added`: auto-creates intermediate ancestors. After insert, run path-collision check.
- `dir_moved`: ensure old dir exists (auto-create if needed), apply the move, run path-collision check on the new path.
- `dir_deleted`: if dir is unknown, no-op. Tombstone is recorded so any future event that auto-creates this path gets a fresh id rather than resurrecting the deleted dir.
- `book_dir_added` / `book_dir_removed` / `book_dir_deleted`: auto-create the referenced dir if missing. The set-membership HLC rule (separate plan) gates the actual mutation.
- `book_added` / `book_deleted` / `reading_position_updated` / `book_opened`: no dir dependency. Unchanged.

## Steps

### 1. Path canonicalization
Add `canonical_dir_path` to `SyncCommon`. Unit tests pinning down: NFC vs NFD inputs hash equal; case is preserved; trailing slash removed; `..` rejected; root paths.

Migrate every emit path that writes a dir path to call `canonical_dir_path` first. Currently the trigger payloads at `Backend/src/database/triggers.sql` build paths via SQL string ops — either call the canonicalization in Rust before emit, or implement an equivalent SQL function. Recommend Rust-side canonicalization at the emit boundary; keep triggers simple.

### 2. dir_alias table + lookup helper
Add the schema. Add `resolve_dir_id(payload_id) -> canonical_id` used everywhere a `dir_id` is read from an incoming change.

### 3. Path-collision merge
Implement in `Backend/src/db.rs`:
- `find_other_dir_at_path(tx, path, except_id) -> Option<DirRow>`
- `merge_dirs(tx, winner_id, loser_id)` — rewrite `book_dir` refs (using set-membership HLC where they collide), tombstone the loser, insert into `dir_alias`.
- Hook into `dir_added` and `dir_moved` apply arms.

### 4. Replace stash with auto-create on apply
In `apply_remote_change_in_tx`, replace the 5 `stash_pending_remote_change` calls with the auto-create-then-proceed path. Mark synthesized rows with `auto_created = 1` (add column to `dir`).

### 5. Server-side synth
In `Server/src/db.rs::insert_changes`, replace the `continue` after `causal_guard_allows` with a synth-and-insert path. The synth event is inserted with `client_id = "__server_synth__"`. Add a route-level guard rejecting any push whose `client_id` matches the reserved string.

### 6. Remove the stash machinery
Delete `stash_pending_remote_change`, `load_pending_remote_changes`, `drain_pending_remote_changes_in_tx`, the call from `pull_events_from_server`, and the `pending_remote_changes` `sync_metadata` key. Migration: clear that key on first run after the upgrade — anything still stashed at that point is best handled by re-pulling from `last_pull_server_seq = 0` once, but in practice the server-side synth means the original cause is gone.

### 7. Tests
- **Out-of-order push.** Client B pushes `book_dir_added` referencing `/Music/Jazz` before any `dir_added` for that path. Assert the server inserts a synth `dir_added`, then the child. Client A pulls, sees both, then later pushes its real `dir_added(/Music/Jazz)`; merge collapses to one canonical dir; B's book is in the canonical dir.
- **Two-device offline same-path create.** A and B both create `/Music/Jazz` offline; sync; assert one canonical dir, both books present.
- **Create / rename / recreate.** A creates `/Music/Jazz` (id=X), renames to `/Music/Bebop`, then creates new `/Music/Jazz` (id=Y). After sync everywhere: two distinct dirs, X at `/Music/Bebop`, Y at `/Music/Jazz`. No merge.
- **Rename into occupied path.** A creates `/Music/Jazz` (id=X). B creates `/Music/Bebop` (id=Y). A renames Jazz to Bebop. After sync: merge into one canonical dir at `/Music/Bebop`; books from both X and Y combined.
- **Delete then recreate.** A creates `/Music/Jazz` (id=X), deletes it. A creates `/Music/Jazz` again — fresh id Z. Z is alive, X is tombstoned. No resurrection.
- **Late event references aliased id.** Trigger a merge so X aliases to Y. Then apply a `book_dir_added` payload still referencing X. Assert the alias resolves and the book lands under Y.
- **Reserved client_id rejected.** Push with `client_id = "__server_synth__"` returns 400.
- **Canonicalization.** NFD `/Café` and NFC `/Café` from two devices land in the same dir (one row, no merge needed because canonical paths are equal at insert time).

### 8. Cleanup
- Drop the `pending_remote_changes` metadata key (migration).
- Remove `causal_guard_allows` once the synth path is in place — it has no remaining callers.

## Decisions

- **Random UUIDs for `dir_id`, not v5(path).** v5 collapses the create/rename/recreate sequence. The path-collision merge is needed anyway for the rename-into-occupied-path case, so reuse it for everything.
- **Case-sensitive paths.** Simplest rule, matches Linux semantics, matches the wire format. Cross-platform consequence (a Mac user might expect `/Photos` and `/photos` to be the same) is acceptable and documented.
- **Auto-created dirs do not emit `dir_added`.** Avoids amplifying one event into many on every device. The eventual real `dir_added` arrives via normal sync.
- **Server synthesizes events on causal miss instead of stashing server-side.** Simpler: no server-side stash table, no drain, no dead-letter. The synth event is real once inserted and replicates normally.
- **`dir_alias` is permanent.** Late-arriving events from devices that haven't seen the merge could appear weeks later. Storage is trivial.

## Risks

- **Path canonicalization drift.** If any emit path forgets to canonicalize, two paths that should hash equal won't, and you'll get a phantom duplicate dir until merged. Mitigation: canonicalize at the central emit boundary, not per-arm. Add a debug assertion that any path in a pulled event is already canonical.
- **Auto-create + tombstone race.** A child event auto-creates a path that was just deleted on another device with a higher HLC. The tombstone needs to be checked first; if a tombstoned dir at this canonical path exists with HLC ≥ child's HLC, drop the child (or attach it to the tombstoned dir, which then resurrects under the path-collision rule against the next real create — TBD). Spell this out in the implementation.
- **Merge during transaction explosion.** A single `dir_moved` could trigger a merge that rewrites thousands of `book_dir` rows. Acceptable for a rare event but should be one transaction. Watch for slow queries.
- **Server synth client_id collisions.** `__server_synth__` must be reserved at the auth/route layer too, not just at DB. Document the reserved prefix.

## Estimated effort
~2 days. Path canonicalization (~few hours), `dir_alias` + resolve helper (~few hours), path-collision merge (~half day), auto-create wiring (~few hours), server-side synth (~half day), tests (~half day), removing stash (~hour).

Best landed after the dir-id-payloads plan (so payloads carry `dir_id` consistently) and the set-membership conflict plan (so `book_dir` ref rewriting during merge uses the right HLC rule for tuple collisions). No dependency on push-idempotency or asset-sync.
