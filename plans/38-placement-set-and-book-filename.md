# Plan 38: Placement becomes an add-remove set; filename and hash move to the book

**Status:** ⬜ open

Atomic but wide. Touches `SyncCommon/src/lib.rs`, `SyncCommon/src/library_state.rs`,
`servers/sync/crates/server-database/schema/03_sync.sql`,
`servers/sync/crates/server-postgres/src/{sync_store.rs,assets.rs}` and its
`sql/sync/upsert_placement_batch.sql`, `Backend/src/library/database/remote_reducer.rs`,
`Backend/src/platform/scanner.rs`, and the local `book_dir` schema.

This is the correctness half of the placement rework. It is a prerequisite for the
single generic `sync_state` table (a later plan): placement is currently the only
register the server must interpret, and this plan makes its payload opaque.

**No migration.** Deployment wipes the database; do not write backfill code and do
not preserve existing sync state.

Renaming existing files on disk to match the synced name is **out of scope** — see
[Deliberately excluded](#deliberately-excluded).

## Problem

A placement is a *matching* between books and paths, modelled as independent
per-path LWW registers keyed `(library_id, dir_id, file_name)`. A set of
independent registers can only enforce one side of a matching:

- "at most one book per path" — structural, guaranteed by the key;
- "at most one path per book" — **not representable**, nothing arbitrates it.

That asymmetry produces five concrete defects.

1. **Convergent-but-wrong state.** Device A moves `dune.epub` into Shelf1 while
   offline device B moves it into Shelf2. Result: `Present{book N}` at both paths,
   each the legitimate LWW winner of its own register, plus `Vacant` at the origin.
   Every replica converges on the same state, and that state has one book at two
   paths. LWW cannot help — the conflict is *between* registers.
2. **The conditional policy is a symptom.** `PlacementState::Vacant` carries
   `removed_book_id` so a path register can reason about state it does not own
   (`SyncCommon/src/lib.rs:290`). It guards only the removal direction; `Present`
   replaces unconditionally, so the *claim* direction has no guard at all.
3. **Repair is unauthored and order-dependent.** `claim_physical_book_path`
   (`Backend/src/library/database/remote_reducer.rs:295-311`) evicts whatever other
   book occupied the path via `remove_live_book_from_path`, and returns a
   `RemovedBookDir` that the caller uses to delete a local file. That transition
   appears in **no register**: nothing in the sync log records that the displaced
   book lost its placement, and a replica applying the same mutation set in a
   different order need not perform the same eviction. Placements are therefore
   convergent per-register plus a local heuristic, which is not convergence.
4. **`Present{book_id, content_hash}` is over-determined and unvalidated.**
   `BookId::from_content_hash` (`BokheimIdentifiers/src/lib.rs:20`, used at
   `Backend/src/library/model.rs:56`) derives the id from the hash. The DB `CHECK`
   validates only hash *format*, `may_replace` compares only `book_id`, and blob
   refcounting in `upsert_placement_batch.sql` follows only `content_hash` — so a
   mismatched pair makes the conflict rule and the quota accounting key off
   different facts.
5. **It blocks a dumb relay.** Placement is the sole reason the sync server still
   reads inside a synchronized payload.

## Settled decisions

Recorded so a fresh session does not re-litigate them.

- **`content_hash` is immutable per book**, because `BookId` is derived from it: if
  the bytes change, the id changes and it is a different book. It therefore needs no
  LWW semantics and goes into `BookLifecycleState::Present` alongside `added_at`,
  which is already a set-once, seed-only register.
- **`file_name` gets its own sibling register**, not a field in `BookMetadataState`.
  Metadata is *immutable* (a pure function of the file's bytes); `file_name` is
  *mutable* (the user renames files on disk). Folding a mutable field into an
  immutable register would rewrite the entire metadata payload — title,
  contributors, description, publication — on every rename.
- **No write-provenance field.** There is no human writer of metadata to protect:
  `update_book_metadata` (`Backend/src/library/database/metadata.rs:247`) and
  `split_book_author` are called only from tests, and no command or UI path edits a
  title. Every metadata write derives from the same EPUB bytes, so two devices
  write identical values and LWW between them is a no-op. Metadata is a **cache of
  a pure function**, not shared mutable state, and metadata conflicts are
  impossible in principle.
- **`book_dir` primary key becomes `(dir_id, book_id)`** — see step 13.
- **`book_dir.intent_deleted` is dropped** — see step 14.

## Design

**Placement becomes an add-remove set.** Key `(book_id, dir_id)`, value
`present: bool`. A set has no cross-register invariant to violate, so:

- concurrent moves cannot produce an inconsistent state — a book in two
  directories is a *legal* state of the model;
- a half-applied move (add arrived, remove did not) is a temporarily visible extra
  entry, not corruption, so nothing has to repair it;
- `Vacant`, `removed_book_id`, the conditional policy, and the eviction heuristic
  all disappear.

**Filename moves to a per-book synced register.** `MutationBody::BookFileName`,
last-writer-wins, overwritten in place. Renames therefore do not grow the
keyspace — contrast keeping `file_name` in the placement key, where every rename
permanently adds a dead triple and makes tombstone retention load-bearing.

**Blob refcounting rewires to book lifecycle.** A hash is referenced iff its book's
lifecycle register is `Present`.

**Path resolution stays local and authoritative.** `book_dir.file_name` becomes a
locally owned column written by the scanner from observation. It remains the only
input to `db::get_book_file_path` (`Backend/src/library/operations.rs:46`) and to
`rel_path` in `get_book_assets` (`sql/browsing.sql:26-41`). The synced register is
never consulted to *find* a file — only to *name a new one*. Consequences:

- a synced name that is illegal on this filesystem (`:`, `|`, trailing dot,
  reserved stem like `CON`) degrades to "this device keeps its own name" instead of
  making books unopenable;
- materialization failure is cosmetic, never fatal.

**`materialize` is the only new pure function.** Roughly
`materialize(&FileName, &CollisionSet) -> FileName`: sanitize for the target
filesystem, then apply a deterministic suffix on collision (lowest `book_id` keeps
the plain name). Used **only to name newly downloaded files**; no inverse
(`parse_observed`) is needed anywhere in this plan.

The collision set is the live `(dir_id, file_name)` rows already in `book_dir` for
the target directory — including rows inserted for pending, not-yet-downloaded
placements (step 16), so two concurrent downloads into one directory cannot pick the
same name. Note the consequence: because the collision set depends on the order
books arrive on *this* device, two devices can assign different names to the same
book, and with renaming out of scope that difference is never reconciled. This is
consistent with, and bounded by, the limitation in
[Deliberately excluded](#deliberately-excluded).

**Rename detection compares local-to-local.** The scanner writes the book's
`BookFileName` register only when the observed on-disk name differs from the
*previously recorded* `book_dir.file_name` for that row. It must **never** compare
against the synced register. Two reasons:

- a stable folder produces zero writes, so there is no scan-time write loop;
- copies of one book in different directories may legitimately have different local
  names, and comparing each against a single shared register would make them fight
  every scan.

Seeding: when a scan first creates a `book_dir` row, that is a change-from-nothing
and does write the register. Across devices this settles by LWW, because after
seeding the observed names stop changing.

**In-folder duplicates are not modelled.** Two byte-identical files in one
directory carry no information beyond the second filename. The scanner indexes the
one with the **lexicographically smallest filename** (a pure function of observed
state, so every replica agrees without communicating) and leaves the other on disk,
unindexed. Never deleted — a destructive act triggered by a sync-model constraint
is not worth the risk. The unindexed file gets no `book_dir` row at all; because
the scanner walks files rather than rows, deleting the indexed copy causes the
other to be picked up on the next scan with no promotion logic.

## Changes

### SyncCommon

1. `library_state.rs`: delete `PlacementState`. Add `content_hash: ContentHash` to
   `BookLifecycleState::Present`.
2. `lib.rs`: `MutationBody::Placement { dir_id, file_name, value }` becomes
   `Placement { book_id, dir_id, present: bool }`. Add
   `MutationBody::BookFileName { book_id, value: FileName }`. Update `state_key` to
   `StateKey::Placement { book_id, dir_id }` and `StateKey::BookFileName(book_id)`.
3. `lib.rs:263` `policy()`: placement and `BookFileName` join the unconditional LWW
   group. Delete `MergePolicy::ServerOrderedConditional`,
   `ReplacementPolicy::PlacementRemovalTargetsCurrentBook`, and the placement arms
   of `may_replace`.
4. `lib.rs:301` `conflict_rank()`: drop the placement exception; `present: false`
   becomes a rank-1 tombstone like every other unconditional register.
5. `lib.rs:182` `reduce_same_replica_mutations`: delete the placement carve-out.
   Placements now reduce like any other register.

### Server

6. `schema/03_sync.sql`: `sync_book_placement_state` becomes
   `PRIMARY KEY (library_id, book_id, dir_id)` with an opaque `value bytea`. Drop
   `file_name`, `content_hash`, `present`, and
   `sync_book_placement_state_present_hash_check`. Add a
   `sync_book_file_name_state` table shaped exactly like
   `sync_book_metadata_state`.
7. `sql/sync/upsert_placement_batch.sql`: delete the `old_state` CTE, the
   `previous_hash`/`current_hash` projection, and the
   `WHERE (EXCLUDED.present OR ...book_id=EXCLUDED.book_id)` conflict guard. It
   becomes the same shape as `upsert_metadata_batch.sql`.
8. `sync_store.rs`: delete `PlacementHashDelta`, the `placement_deltas` field of
   `InsertChangesResult`, the `placement_counts`/`repeated` serialization loop
   (`:242-248`), and the placement arm of `decode_operation`. Placements and
   `BookFileName` route through the generic opaque path alongside annotations.
9. `assets.rs`: replace `apply_placement_deltas` with a book-lifecycle-driven
   equivalent — `user_blob_reference` gains/loses a reference when a book's
   lifecycle register transitions `Present`↔`Deleted`. Call site is `sync.rs:203`.
10. `sync_store.rs:255` `required_directory`: keep as-is (placement still carries
    `dir_id`). Dropping dependency checks belongs to the generic-table plan.

### Backend

11. `remote_reducer.rs`: delete `claim_physical_book_path` and
    `placement_removal_targets_current_book`, and the
    `remove_live_book_from_path` / `displace_live_book_at_path` queries that back
    them. **Keep `RemovedBookDir` and the `removed` return value.** It has two
    producers and only the eviction one goes away:
    - `:60` — legitimate: a remote removal means delete the local file, consumed by
      `apply_local_removals` (`sync/manager.rs:729`). This must survive, or remote
      deletions stop propagating to local files.
    - `:306` — the eviction inside `claim_physical_book_path`. Delete this one only.
12. `remote_reducer.rs`: the lifecycle arm now also writes `content_hash`. The new
    `BookFileName` arm seeds `book.file_name`. Preserve the existing seed-only
    discipline (`apply_remote_book_added:265-279` +
    `COALESCE(book.title, excluded.title)` in `sql/books.sql:14`) for `content_hash`.
13. `create_library.sql:161-187`: `book_dir` primary key becomes
    `(dir_id, book_id)`, matching the synced placement key one-to-one so no local
    row lacks an upstream representation. Keep the partial unique index
    `idx_book_dir_live_path` on `(dir_id, file_name) WHERE deleted_at IS NULL` —
    one live book per path is still a local requirement. A scanner dedup bug now
    trips the primary key instead of silently duplicating.
14. `create_library.sql:182`: drop `book_dir.intent_deleted`. Its only
    distinguishing writer was `remove_live_book_from_path`
    (`sql/remote_reducer.sql:111`), which set `deleted_at` *without*
    `intent_deleted` precisely to suppress a same-path loser — the behaviour this
    plan removes. Every other writer moves the pair in lockstep. Keep the column
    name `deleted_at` so the dozens of `deleted_at IS NULL` predicates across
    `browsing.sql`, `views.sql`, `book_access.sql`, and `placements.sql` need no
    rewrite. **The `dir` table's `intent_deleted`/`intent_parent_id`/`intent_name`
    triple stays** — `bokheim_projection::project_directories` cascades deletion
    and reparenting through ancestors, which is unrelated to placement.
15. `scanner.rs`: implement the rename-detection and in-folder duplicate rules
    above. Emit a `BookFileName` write on observed change. Report the unindexed
    duplicate with a new `LibraryEvent` variant (transient, no stored row).
16. `create_library.sql:202-227` `sync_outbox`. Verified current behaviour:
    placement rows use `state_key = lower(hex(randomblob(16)))`, a **nonce**
    (`triggers.sql:233`), so `unique(state_kind, state_key)` never coalesces them —
    the local mirror of the `reduce_same_replica_mutations` carve-out deleted in
    step 5. Real cell identity is carried by the `placement_dir_id`/
    `placement_file_name`/`placement_book_id` columns purely so
    `trig_sync_outbox_stamp_insert` can stamp `sync_state_version` without parsing
    `body`.

    Change: add a generic `state_subkey TEXT NOT NULL DEFAULT ''` column; make the
    constraint `unique(state_kind, state_key, state_subkey)`; give placements
    `state_key = book_id`, `state_subkey = dir_id` so they coalesce like every other
    register; **drop all three coordinate columns and their CHECK**; add
    `'book_file_name'` to the `state_kind` CHECK. The stamp trigger's two
    `CASE WHEN NEW.state_kind='placement'` expressions (`triggers.sql:384-385`)
    collapse to plain `NEW.state_key` / `NEW.state_subkey`.
17. `encode_trigger_mutation` (`Backend/src/library/database/mod.rs:175-215`),
    registered as the SQLite function `bokheim_mutation` and called from every
    outbox trigger: rewrite the `"placement"` arm (`:207`) to build
    `Placement { book_id, dir_id, present }` from three args instead of
    `PlacementState` from six, and add a `"book_file_name"` arm. The three call
    sites are `triggers.sql:234`, `:248`, and `:261`.
18. `sync_state_identity` (`remote_reducer.rs:153-166`) maps
    `Placement → ("placement", dir_id, file_name)` for `sync_state_version`; it
    becomes `("placement", book_id, dir_id)`, plus a new `BookFileName` arm. Cell
    identity is computed in **three** places — this function, its copy at
    `sync_state.rs:73-79`, and the stamp trigger's CASE expressions (removed in
    step 16). All must agree; see test 9.
19. `sync/transfer.rs:555`: the download target path is
    `dir_path + materialize(book.file_name, collisions)` rather than a placement's
    synced `file_name`. Insert the `book_dir` row with that name before the
    transfer, so it joins the collision set and `rel_path` resolution downstream is
    unchanged.

## Tests

1. **Concurrent move no longer duplicates.** Two replicas move one book to
   different directories; converge. Assert the book is in exactly the directories
   the union of writes says, and that *both* replicas agree — this is the defect
   from Problem 1, currently unrepresentable as a passing test.
2. **Apply-order independence.** Replay one placement mutation set in several
   permutations (including split across pull pages) and assert identical final
   `book_dir` state. Regression test for Problem 3; it should fail against the
   current `claim_physical_book_path`. Plan 12's convergence harness helps here.
3. **Remote deletion still propagates.** A remote `present: false` deletes the
   local file via `apply_local_removals`. Guards the step 11 correction — the
   easiest way to get this plan wrong is to delete `RemovedBookDir` wholesale.
4. **Stable folder produces zero mutations.** Scan twice with no filesystem change;
   assert the second scan emits no `BookFileName` writes. Guards the write-loop
   risk.
5. **Copies with different local names do not fight.** Same book in two directories
   with different on-disk names; scan repeatedly; assert neither name is rewritten
   and the register does not flap.
6. **In-folder duplicate.** Two byte-identical files in one directory: assert the
   lexicographically smallest is indexed, the other is untouched on disk and
   reported, that a second replica reaches the same choice, and that deleting the
   indexed copy causes the other to be indexed on the next scan.
7. **`materialize` determinism.** Property test: identical `(name, collision set)`
   input always yields identical output; output is legal on the target filesystem;
   two distinct books in one directory never map to the same name. Do **not** assert
   device-independence — collision sets legitimately differ across devices.
8. **Blob refcount follows lifecycle.** Book `Present` → 1 reference; `Deleted` → 0;
   placement add/remove alone → no change. Replaces the placement-delta assertions
   in `servers/sync/crates/server-postgres/src/sync.rs:658-707`
   (`canonical_path_state_does_not_resurrect_displaced_books`), which tests
   behaviour this plan deletes and should be **removed rather than adapted**.
9. **Cell identity agrees across every mapping.** For each `MutationBody` variant,
   assert `sync_state_identity` (`remote_reducer.rs:153`) and its copy at
   `sync_state.rs:73` produce the same `(kind, key, subkey)`, and that a trigger-
   generated outbox row for the same logical write stamps the same triple into
   `sync_state_version`. Guards the step 18 trap: if these drift, outbox coalescing
   and local version stamping disagree about which writes are the same cell, and
   the symptom is lost or duplicated pending mutations rather than an error.
10. **Placements now coalesce.** Two local edits to the same `(book_id, dir_id)`
    before a sync leave exactly one pending outbox row, not two. This is the
    behaviour change from replacing the nonce `state_key` in step 16, and it is the
    local counterpart of deleting the carve-out in step 5.
11. **Illegal synced name degrades.** A `BookFileName` value illegal on the local
   filesystem: assert the book still opens via `book_dir.file_name` and that no
   error reaches the reader.

## Deliberately excluded

**Renaming existing files to match the synced register.** Without it, the synced
`file_name` affects only *newly downloaded* files: a fresh device pulling the
library gets names matching its origin, which is the actual value. Existing files
keep whatever they are called locally, so two devices can hold the same book under
different names indefinitely, and a hand-rename on one device updates the register
without renaming anything elsewhere.

Accepted on purpose: adding it later needs a `parse_observed` inverse plus per-file
last-written-name tracking, and that is the entire feedback-loop risk surface (a
partially-synced device reading its own materialization back as a user rename). None
of it is required for the correctness fixes above, and none of it is foreclosed here.
