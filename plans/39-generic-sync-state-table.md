# Plan 39: Collapse nine per-kind sync tables into two generic tables

**Status:** ⬜ open — **requires plan 38 first**

Touches `servers/sync/crates/server-database/schema/03_sync.sql`,
`servers/sync/crates/server-postgres/src/{sync_store.rs,sync.rs,assets.rs}` and most of
`src/sql/sync/`, `SyncCommon/src/lib.rs`, and the client's decode/identity paths in
`Backend/src/library/database/{remote_reducer.rs,sync_state.rs,mod.rs}`.

**No migration.** Deployment wipes the database; do not write backfill code.

Plan 38 is a hard prerequisite: it makes the placement payload opaque and removes
the conditional-merge policy. This plan finishes the job for the two remaining typed
domains, then collapses the schema.

**Plans 14 and 15 are also prerequisites**, though for a weaker reason than a first
reading suggests. Verified: every upsert sets `server_seq=DEFAULT` on conflict
(`upsert_directory_batch.sql:17`, `upsert_placement_batch.sql:33`), so an LWW
overwrite resequences the row. Pull orders by `server_seq`, so **pull order is
last-modified order, not creation order** — a directory renamed after a book was
placed in it pulls *after* that placement on a fresh device.

That means clients already receive placements for partially-described directories
today, routinely, and `ensure_directory_placeholder` exists to absorb it.
`existing_dependencies.sql` never prevented this: it is a *push-time* existence
check and imposes no ordering on what a different client pulls later. Deleting it
therefore does not create the ordering problem — it only newly admits a placement
whose directory the server has *never* seen, which requires a buggy client today and
becomes legitimately reachable only if plan 23 GCs a deleted directory's rows while a
placement still references it.

Plans 14 and 15 remain prerequisites because test 8 exercises the
partially-described path deliberately, and that path is the one they document as
broken (truncated ancestor chains materializing folders at the wrong location;
delete-ordering resurrecting directories). Fix them first or test 8 fails for
reasons unrelated to this plan.

## Problem

The schema comment at `03_sync.sql:6` already states the intent — the server
"stores and relays synchronized state; it does not interpret it" — but the
implementation honours that only *per payload*, never *per entity kind*. Nine
tables (after plan 38 adds `sync_book_file_name_state`), each carrying:

- its own `(library_id, server_seq)` index;
- three triggers (`lww_guard`, plus insert/update revision statement triggers);
- its own upsert SQL file;
- an encode arm in `sync_store.rs` and a decode arm in `decode_operation`;
- a branch of the eight-way `UNION ALL` in `fetch_page.sql`, padded with
  `ARRAY[]::TEXT[]` / `ARRAY[]::REAL[]` / `'\x'::bytea` placeholder columns so the
  branches typecheck against each other.

Adding a synchronized entity type therefore touches roughly seven places plus a
schema migration, and the pull query's plan is a nine-way union rather than one
index range scan.

Two domains are still typed after plan 38:

- **directories** — `component text` + `value text`, three registers per directory
  (`name`, `parent`, `lifecycle`), plus a CHECK coupling `component = 'lifecycle'`
  to `value IN ('present','deleted')`;
- **reading positions** — `cfi text` + `progress real`, with a
  `progress BETWEEN 0 AND 100` CHECK.

Neither field is ever read by the server. They are typed only because the tables
were built per-kind.

## Design

**Two structurally identical tables**, split by write frequency rather than by kind.
Everything the server needs is kind-independent: a slot identity to collapse LWW
onto, the tie-break tuple, a monotone per-library sequence, and ownership.

```sql
-- identical column list, identical triggers, identical upsert shape
CREATE TABLE sync_state (            -- every cold kind
    library_id    text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    kind          text NOT NULL,
    entity_key    text NOT NULL,
    entity_subkey text NOT NULL DEFAULT '',
    value         bytea NOT NULL,
    -- Client-declared, book-lifecycle rows only; NULL for every other kind.
    -- See "Blob references are declared, not parsed" below.
    present       boolean,
    content_hash  text,
    changed_at    bigint NOT NULL,
    client_id     text NOT NULL,
    client_seq    bigint NOT NULL,
    version_rank  smallint NOT NULL,
    event_id      text NOT NULL,
    server_seq    bigint NOT NULL DEFAULT nextval('sync_server_sequence'),
    PRIMARY KEY (library_id, kind, entity_key, entity_subkey),
    UNIQUE (library_id, event_id)
);
CREATE TABLE sync_reading_state (...);  -- same shape, reading positions only

CREATE INDEX sync_state_library_seq ON sync_state (library_id, server_seq);
CREATE INDEX sync_reading_state_library_seq ON sync_reading_state (library_id, server_seq);
```

**Cell identity is a triple, matching the client exactly.** Most kinds use
`entity_key` alone and leave `entity_subkey` empty; placement needs both halves
(`book_id`, `dir_id`) because a book lives in several directories and a directory
holds several books. The client already represents identity this way —
`sync_state_version` has `state_key`/`state_subkey`, and plan 38 step 16 adds the
same pair to `sync_outbox` — so a second column keeps one representation end to end
with nothing to translate.

The alternative, flattening both halves into one string, was rejected: it requires a
separator that can never occur inside either half, enforced identically in Rust and
in the SQLite triggers that build outbox rows. A leaked separator silently collapses
two cells into one key or splits one cell into two, with no error — precisely the
class of failure this plan exists to remove. Hashing the pair is the same scheme in
disguise and additionally destroys readability.

`kind` is **text**, not a numeric discriminant, and carries no enum CHECK. Text
matches the existing local convention (`sync_outbox.state_kind`) and preserves the
one thing full opacity otherwise costs you: a `psql` reader can still see what a row
*is*. No CHECK, so adding a kind needs no server deploy and no migration — which is
the point of the whole exercise. Text also means one representation end to end:
`sync_outbox.state_kind` and `sync_state_version.state_kind` are already text, so no
kind↔number mapping exists to drift between client and server.

The cost of dropping the CHECK is that a typo (`'reading_positon'`) becomes a
silently separate register that never merges and raises no error — the row stores,
relays, and is skipped by every decoder. Mitigation: every kind string must come from
one total `fn kind(&self) -> &'static str` over `MutationBody` in SyncCommon, never a
literal at a call site. The compiler then enforces that each variant has exactly one
spelling, and a grep for kind literals outside that function should return nothing.

**Reading positions keep their own table, and the reading/other revision split is
kept as-is.** Reading progress is written every few seconds during a reading session;
everything else changes rarely. Two consequences make isolation worth a second table:

- **Vacuum.** Every LWW update sets `server_seq=DEFAULT`, and `server_seq` is
  indexed, so no update can be HOT — each writes a new tuple plus new index entries.
  Merging the hot kind into the cold table would bloat the index that every pull
  scans. A separate table confines that churn and lets it be vacuum-tuned alone.
- **The common-case pull gets cheaper, not just cheaper than today.** With the split,
  "only a reading position moved" scans one small hot table. A single merged table
  would always scan the big one. So two tables beat one here, and both beat today's
  nine-way union.

`sync_library_revision` therefore keeps `reading_revision`/`other_revision`/
`current_revision`, and `pull_route`, `PullRoute`, and `fetch_reading_position_page`
all survive unchanged in intent. This is existing, working code — the plan preserves
it rather than deleting and later regretting it.

The server does learn exactly one kind name to route writes (`reading_position` goes
to `sync_reading_state`, everything else to `sync_state`). That is the entire extent
of its kind knowledge, and it does not cost the deployless-extension property: adding
a new *cold* kind still needs no server change. Only promoting a future kind to hot
would, and reading position is plausibly the only hot kind this system will ever have.

An earlier draft deleted the split, reasoning that a single scan makes the route
redundant. That reasoning was correct for one table and wrong for two. Do not
re-delete it without first merging the tables, and do not merge the tables without a
bloat measurement.

**Directories stay three registers.** `directory_name`, `directory_parent`, and
`directory_lifecycle` become three *kinds* rather than one table with a `component`
discriminator — the `component` column effectively becomes the `kind` column, and
each value becomes wire-encoded instead of raw text. The typed columns still go; the
field-level granularity stays.

An earlier draft of this plan merged them into one `DirectoryState { name, parent_id,
deleted }`. That was wrong, for a reason worth recording so it is not re-proposed:
a merged register makes every rename a read-modify-write over the whole value, so a
device with a stale `parent_id` writes that stale parent back and silently reverts a
move made elsewhere. The loser of that exchange never edited anything. Three
registers make it structurally impossible — a rename writes only the name.

The saving a merge would have bought (one table, one index, three fewer triggers,
three fewer decode arms) is precisely what this plan delivers for *all* kinds
anyway. Post-collapse a kind costs three `MutationBody` variants that already exist
and nothing else, so there is nothing left to buy.

**Tombstone representation is left alone.** With directories keeping a separate
lifecycle register, the three encodings persist: a sibling register for directories,
a `Deleted` variant for book lifecycle, `deleted: bool` inside annotations,
bookmarks, and links. Unifying them would mean merging the directory registers, so
it is off the table. This costs nothing mechanically — `conflict_rank()` reads
inside the value on the client, and the result travels as the `version_rank` column,
which the server stores and never re-derives regardless of how deletion is spelled.

**Blob references are declared, not parsed.** The server maintains
`user_blob_reference` for quota and GC, so it must know which content hashes are live
for a user — a fact that otherwise lives inside an opaque payload. Rather than decode
`BookLifecycleState`, the client declares it: book-lifecycle rows carry `present` and
`content_hash` as columns beside the value, and the server reads only those. This is
the same contract `version_rank` already follows — computed client-side from the
payload, sent as a column, because the Postgres trigger must act without decoding.

The simplification worth noticing: **`content_hash` is immutable per book.** `BookId`
is derived from it, so changed bytes mean a different book, and a book's hash never
transitions. Reference counting therefore collapses from "compare previous and
current hash" to a boolean flip — the reference exists iff the book is present. The
`old_state` CTE and `previous_hash`/`current_hash` projection in
`upsert_placement_batch.sql` are not ported anywhere; they disappear, along with
`PlacementHashDelta` and `apply_placement_deltas`.

These two columns are the only kind-specific data in an otherwise generic table.
That is a deliberate wart: the server genuinely needs the facts, the client declares
them, and no decoding occurs. `reference_count` stays a count rather than a boolean —
one user can hold the same book in two libraries.

**Reading positions become opaque even though they keep their table.** `cfi`/
`progress` move inside the value; the `progress` range CHECK moves to the client,
which already validates it (`ReadProgress::new` in the reducer). The table is
retained for write-frequency isolation, *not* because the server interprets its
contents — `sync_reading_state` has the same generic column list as `sync_state`. The
atomic singleton fast path targets it directly and survives unchanged in shape.

**Dependency checks are dropped.** `existing_dependencies.sql`,
`required_directory`/`required_books`, and `MutationRejectionReason::MissingDependency`
go away. They are not a safety property: under paged pulls a client already receives
a placement before its directory routinely, so it must tolerate a dangling reference
locally regardless. The server check duplicates a client check that cannot be
removed, at the cost of a round trip and two typed columns.

**Wire shape.** `MutationBody` stops being a wire type. On the wire a mutation
carries `{ kind, entity_key, value }`; the ten-variant enum lives only in
client code, which decodes `value` according to `kind`. An unknown `kind` is skipped
rather than an error — required for forward compatibility once the server no longer
gates on kind.

## Changes

### SyncCommon

1. No change to the directory variants — `DirectoryName`, `DirectoryParent`, and
   `DirectoryLifecycle` stay as they are. Their payloads become wire-encoded values
   instead of the raw `text` they occupy today in `sync_directory_state.value`.
2. Introduce the wire form: `WireMutation { kind: String, entity_key: String,
   entity_subkey: String, value: Vec<u8>, changed_at, replica_seq, mutation_id }`,
   plus `MutationBody → WireMutation` encode and `(kind, value) → MutationBody`
   decode returning `Option` so an unknown kind is skipped, not fatal. Every kind but
   placement leaves `entity_subkey` empty.
3. `state_key()` collapses into the wire form's
   `(kind, entity_key, entity_subkey)` — the same triple the server stores and the
   same one `sync_state_version` already uses, so the three representations named in
   plan 38 step 18 become one and cannot drift.
4. `policy()` reduces to one arm: every register is unconditional LWW. Delete
   `StatePolicy`, `MergePolicy`, `ReplacementPolicy`, and `may_replace` entirely —
   plan 38 removed the only conditional register, so the abstraction has one
   inhabitant.
5. `conflict_rank()` is unchanged.

### Server schema and SQL

6. `03_sync.sql`: replace all nine state tables, their indexes, and their 27
   triggers with `sync_state` and `sync_reading_state` — identical column lists —
   one `lww_guard` each and one pair of revision statement triggers each.
   `advance_library_sync_revision_statement()` keeps its two-branch shape but
   branches on `TG_TABLE_NAME = 'sync_reading_state'` against exactly two tables
   instead of nine. `sync_library_revision` is **unchanged**, including both
   cross-column CHECKs. Expect ~265 → ~90 lines.
7. Replace the nine `upsert_*_batch.sql` / `upsert_*.sql` files with **one**
   `upsert_state_batch.sql` parameterized by target table: an `UNNEST` of the seven
   common columns plus `kind`, `entity_key`, `entity_subkey`, `value`, with
   `DISTINCT ON (kind, entity_key, entity_subkey)` ordered by the version tuple.
   Both tables share
   it; only the table name differs, so keep it a single statement built once rather
   than two copies that can drift.
8. `fetch_page.sql` becomes `SELECT ... FROM sync_state WHERE library_id = $1 AND
   server_seq > $2 ORDER BY server_seq LIMIT $3`, and
   `fetch_reading_position_page.sql` the same against `sync_reading_state`. Both are
   single-table index scans; the eight-way `UNION ALL` is gone from both. **Keep**
   `pull_route`, `PullRoute`, `get_pull_heads.sql`, and `fetch_changes`'s existing
   routing — only the queries underneath change.

   Note the `Full` route must now read *both* tables and merge by `server_seq`. That
   is the one place two tables cost more than one: a two-way merge instead of a
   single scan. It is still eight branches fewer than today, and the common route
   avoids it entirely.
9. Delete `existing_dependencies.sql`, `book_exists.sql`, `directory_exists.sql`.
10. `exchange_authorized_reading_position.sql`: keep the name and the shape; only its
    columns become generic (`kind`/`entity_key`/`value` instead of
    `book_id`/`cfi`/`progress`) and its target becomes `sync_reading_state`. Do
    **not** rename it to `..._singleton` — it writes one specific table, so it is not
    kind-agnostic and should not read as though it were.

### Server code

11. `sync_store.rs`: delete `upsert_directory_batch`, `upsert_book_lifecycle_batch`,
    `upsert_placement_batch`, `upsert_reading_position_batch`,
    `upsert_metadata_batch`, `upsert_typed_state`, and `decode_operation`. One
    `upsert_state_batch` and one row→`ServerMutation` decode replace them.
    `insert_changes` keeps a partition step, but it goes from six per-kind buckets to
    two — reading versus cold — driven by the same routing predicate as step 12, not
    by a match over `MutationBody`.
12. `sync_store.rs`: `supports_atomic_singleton` stays kind-aware, because the fast
    path writes one specific table. The gate becomes "exactly one mutation **and** it
    routes to `sync_reading_state`" — behaviourally what
    `matches!(body, MutationBody::ReadingPosition { .. })` does today, but expressed
    through the single routing function so the one hardcoded kind name lives in
    exactly one place. **Keep the fast path**: it collapses push+pull into a single
    statement for the most frequent write in the system. It leaves two exchange paths
    that must stay behaviourally identical; test 4 holds them together, so port those
    assertions rather than rewriting them.
13. `sync_store.rs`: delete `required_directory`, `required_books`, and the
    `provided_dirs`/`provided_books`/`existing_dirs`/`existing_books` set building.
14. `server-core/src/sync.rs:16` `SyncError::UpgradeRequired`: verified **already
    dead** — declared and mapped to a status code at
    `server-http/src/extract.rs:29`, but constructed nowhere in the tree. It is not
    made dead by this plan. Delete it in passing or leave it to a later dead-code audit; either way
    it is not a discovery task.
15. `assets.rs`: delete `PlacementHashDelta` and `apply_placement_deltas` and replace
    them with a boolean-flip adjustment driven by the `present`/`content_hash`
    columns. Key it off the upsert's `RETURNING` so a write the `lww_guard` cancelled
    produces no delta. Call site is `sync.rs:203`.

    If the plan 38 repair already landed this shape in the per-kind schema (it
    should — the same decision governs both), this step is only carrying the columns
    across to `sync_state`.

### Client

16. `remote_reducer.rs`: `apply_remote_change_in_tx` keeps its `MutationBody` match
    — the client still interprets everything — but is now fed by the wire decode
    from step 2. An unknown kind is logged and skipped without advancing local state
    for that cell.
17. `sync_state_identity` (`remote_reducer.rs:153`) and its duplicate at
    `sync_state.rs:73` both become thin wrappers over the wire form's
    `(kind, entity_key)`. Prefer deleting one copy outright; plan 38's test 9 exists
    to catch drift between them, and this is the chance to remove the drift surface
    instead.
18. `create_library.sql` `sync_outbox`: drop the `state_kind` CHECK enum — kinds are
    now open. The placement coordinate columns are already gone (plan 38 step 16
    replaced them with a generic `state_subkey`), so nothing further is needed here.
19. `encode_trigger_mutation` (`mod.rs:175-215`): every arm now emits the wire form
    from step 2 rather than a `MutationBody`; the three directory arms stay. The
    `kind` string each arm produces must come
    from the single `fn kind()` named in the Design section — this function is the
    most likely place for a stray literal to appear, since it is already
    string-dispatched.
20. `reproject_directories` is unchanged — still fires on the three directory
    kinds. The local `dir` table keeps `intent_name`/`intent_parent_id`/`intent_deleted` — the ancestor
    cascade in `bokheim_projection::project_directories` is unrelated to this change.

## Tests

1. **Pull equivalence.** Given a fixed mutation set, the sequence of
   `ServerMutation`s returned by a full pull is identical before and after the
   collapse (same order, same payloads). Build this against the convergence tests before
   touching the schema, so it can run on both sides.
2. **Scan shape.** `EXPLAIN` both routes: the reading-only route is a single index
   scan on `sync_reading_state_library_seq` with no union node; the full route is a
   two-way merge, not a nine-way one. Pins the plan's structural claim.
3. **Route selection and cursor boundary.** Port the existing `pull_route` cases
   verbatim — they still describe live behaviour: cursor equal to both heads returns
   empty; only `reading_revision` past the cursor takes the reading-only route; any
   `other_revision` past the cursor takes the full route; cursor above
   `current_revision` fails `CursorAhead`. Additionally assert the full route returns
   rows from both tables interleaved in `server_seq` order — the merge is new and is
   the most likely place to get ordering wrong.
4. **Atomic singleton still atomic.** Port
   `atomic_exchange_returns_its_committed_winner_and_retries_idempotently` and
   `cursor_ahead_fails_loudly_and_rolls_back_every_exchange_path` to the generic
   path unchanged in intent.
5. **LWW guard is kind-blind, and the key triple separates cells correctly.** Two
   writes to the same `(kind, entity_key, entity_subkey)` with opposing version
   tuples resolve identically for every kind, including a tombstone tie where
   `version_rank` decides. Then assert the halves are independent: one book in two
   directories, and two books in one directory, produce four distinct cells that
   never merge — the property the second key column exists to provide.
6. **Directory field independence survives the collapse.** Concurrent
   rename-on-A + move-on-B of one directory: assert **both** edits survive, as they
   do today. This is the property the three-register design exists to provide, and
   the regression test against ever merging them.
7. **Unknown kind is skipped.** Feed the client a mutation with an unrecognized
   `kind`: assert no error, no local state change for that cell, and that
   surrounding mutations in the same page still apply.
8. **Blob references follow the declared columns.** Book present → one reference;
   deleted → zero; a placement write alone → no change. A losing LWW write produces
   no delta at all, since a cancelled update must not adjust the count. Assert the
   server never decodes a lifecycle payload to reach these numbers.
9. **Dangling reference tolerated.** Two cases, and the first is the one that
   matters because it already occurs in production order:
   a. *Partially-described directory* — deliver `directory_lifecycle` and a
      placement, then `directory_name`/`directory_parent` in a later page,
      reproducing the resequencing described above. Assert nothing materializes at a
      wrong path in the interval, and that the book lands correctly once the
      remaining registers arrive. Write this test **before** plan 39 and run it
      against current `main`; it should already fail, which is the directory materialization bug.
   b. *Never-seen directory and a book link with absent endpoints* — assert both are
      stored, version-stamped, and skipped by the projection without error. This is
      what replaces the deleted push-time check.

## Performance

**This plan is not a performance optimization.** Nothing below is measured; it is
reasoning about query plans. The expected net is between a small win and a wash, with
one tail risk. Do this for the deployment property — adding a synced type becomes a
client-only change — and for the deleted code, not for speed.

**Pull improves modestly.** `fetch_page.sql` today opens eight index scans and
maintains a MergeAppend heap to satisfy `ORDER BY server_seq LIMIT n`. Afterwards the
common reading-only route is a single scan of a small hot table, and the full route
is a two-way merge. But each of today's eight scans is already cheap, MergeAppend
already terminates early, and pull latency is likely dominated by serialization and
network. Constant-factor win at best.

**`Full` is rare, and self-limiting.** `pull_route` selects it only while
`other_revision > cursor`; once the client pulls past that write it reverts to
`ReadingOnly`. Steady state — one device reading, another polling — is `Empty` or
`ReadingOnly` almost entirely. `Full` fires on library edits, annotations and
bookmarks, a concurrent import, and initial sync. The exception worth knowing: a
fresh device's first sync is *all* `Full`, over many pages, and that is the most
latency-visible operation a user meets. So the two-way merge is paid rarely but at
the worst moment — and it is still seven branches fewer than today's plan on that
same path.

**Push is roughly neutral.** Each `upsert_*_batch` already returns before issuing SQL
when its slice is empty, so a batch costs one statement *per kind present*. The
common batches are single-kind — a scan is mostly placements, a reading session is
one reading position — and those already cost one statement today. The nine-statement
case is the worst case, not the typical one. Statement-level revision triggers drop
from up to eighteen to two; row-level `lww_guard` invocations are unchanged.

**Contention does not change.** A library has one owner, so concurrent writers are
that user's few devices, and `sync_server_sequence` is already shared across all nine
tables. The scale variable is total rows across users, not per-library concurrency.

**Storage costs slightly more.** The primary key gains two columns relative to a
narrow per-table PK: `kind` as text (~10–20 bytes per row) and `entity_subkey`, which
is a single empty-string byte for every kind except placement. A Postgres native
`enum` would store `kind` in 4 bytes and still print as text, but `ALTER TYPE ... ADD
VALUE` is a migration, which defeats the deployless-extension goal motivating the
whole plan. Both costs accepted.

**Vacuum concentration is designed out rather than mitigated.** Every LWW update sets
`server_seq=DEFAULT`, and `server_seq` is indexed, so **no update can be HOT**: each
writes a new tuple plus new index entries. Per-update cost is unchanged from today.
What matters is *where* that churn lands, and keeping reading positions in their own
table preserves today's isolation exactly — the cold index that every full pull scans
is never touched by progress updates.

This is the main reason the design is two tables rather than one. A single merged
table would have concentrated the system's highest-frequency writes into the index
its slowest query depends on, leaving `autovacuum_vacuum_scale_factor` tuning as the
only defence. Still worth tuning autovacuum on `sync_reading_state` specifically,
since that table absorbs essentially all the churn.

## Non-goals

- **Adding provenance ordering.** Plan 38 settled that there is no human writer of
  metadata to protect. If that ever changes, `enforce_lww_order()` is now a single
  function over a single table, so the change is one trigger instead of nine.
- **Splitting or partitioning further.** The one split that pays — reading positions
  — is in the design. Any further isolation should come from a measurement, not a
  guess. If a second hot kind ever appears, note that `kind` is in the primary key,
  so converting `sync_state` to `PARTITION BY LIST (kind)` with a `DEFAULT` partition
  is a pure DDL change: no application change, no wire change, and unknown kinds
  still land somewhere valid.
- **Merging the two tables into one.** Tempting for uniformity, and an earlier draft
  did exactly that. It costs the vacuum isolation above and makes the common pull
  scan the big table. Do not do it without a bloat measurement showing the isolation
  is unnecessary.
- **End-to-end encryption.** Not in scope, but worth noting the property this plan
  establishes: a server that provably cannot read payloads can later hold encrypted
  ones without redesign.
