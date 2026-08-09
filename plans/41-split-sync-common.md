# Plan 41: Split `sync-common` into a wire crate and a client-only domain crate

**Status:** ⬜ open — **requires plan 39 first**

Touches `SyncCommon/`, `BokheimDomain/`, and the `Cargo.toml` of every crate that
depends on either. No behaviour change: this is a crate boundary that makes an
existing invariant compiler-enforced.

Plan 39 is a hard prerequisite. Until it lands the server still decodes
`ReadingPositionState` in two production paths, so the domain types cannot leave.

## Problem

After plan 39 the server relays payloads without interpreting them, but nothing
prevents that from regressing. Opacity is a convention held by review discipline;
one `use sync_common::BookMetadataState` in a server crate reintroduces the coupling
silently, and the compiler is happy.

Two supporting costs today:

- **`specta` is in the server's dependency graph for no reason.** It is used in
  exactly one file — `SyncCommon/src/annotations.rs`, on `PdfAnnotationRect`,
  `PdfAnnotationAnchor`, and `AnnotationAnchor`. All five server crates
  (`Server`, `server-asset-store`, `server-core`, `server-http`, `server-postgres`)
  compile `specta` and `specta-typescript` to generate TypeScript bindings for the
  mobile app that no server code uses.
- **Every domain type change rebuilds the server.** Adding a field to
  `AnnotationState` invalidates five server crates that cannot observe the field.

## Why this is cheap now

Verified against the tree, all three preconditions already hold:

1. `domain` exists and **depends on `sync-common`**, so moving domain types
   into it creates no cycle — the arrow already points the right way.
2. **No server crate depends on `domain`.** Its dependents are all
   client-side: `Backend`, `Backend/crates/book-metadata`, `BokheimAppContract`,
   `BokheimLibraryFiles`, `BokheimCatalogProvider`.
3. After plan 39 the server's production use of domain types is **zero**. Today the
   only two sites are `sync_store.rs:350` (the `decode_operation` reading-position
   arm, deleted by 39 step 11) and `sync_store.rs:463` (the singleton exchange
   destructuring `cfi`/`progress`, made generic by 39 step 10).
   `BookLifecycleState` and `BookMetadataState` already appear only under
   `#[cfg(test)]`.

## Prerequisite: blob references are declared, not parsed — settled in plan 39

The one thing that could block this split is blob reference counting: the server must
know which content hashes are live, and that fact otherwise lives inside an opaque
payload.

**Resolved, and specified in plan 39** ("Blob references are declared, not parsed"):
book-lifecycle rows carry `present` and `content_hash` as client-declared columns
beside the value, and the server reads only those. Because `content_hash` is
immutable per book, refcounting is a boolean flip rather than a hash-delta
computation, and `PlacementHashDelta`/`apply_placement_deltas` disappear.

Nothing further is required here — 39 lands the columns and the refcount rewrite.
This plan only depends on the outcome: no server crate needs `BookLifecycleState`.

Rejected alternatives, recorded so they are not re-proposed:

- *Server decodes `BookLifecycleState`* — defeats this plan entirely.
- *Client calls an explicit blob-reference API* (the plan 33 direction) — cleaner in
  the abstract, but it splits the reference write from the lifecycle write into two
  transactions, so a crash between them leaks or orphans a reference.

## Design

Two crates, split by who interprets what.

**`sync-common` — the wire contract.** Everything both sides must agree on:

- the wire mutation form introduced by plan 39 step 2, and `ServerMutation`
- `MutationId`, `UnixMillis`, `ReplicaSeq`, `LibraryRevision`
- `MutationRejection`, `MutationRejectionReason`
- `MAX_PUSH_MUTATIONS`, `MAX_PULL_CHANGES`, `MAX_LWW_FUTURE_SKEW_MS`
- `pub use wire as wire`
- `api` (all transport: `BlobManifest*`, `LibrarySummary`, `UpsertLibraryRequest`,
  `NotificationMessage`)
- the `identifiers` re-exports the server needs (`LibraryId`, `ClientId`,
  `ContentHash`, `BookId`, `DirId`, `FileName`)

Dependencies after the move: `serde`, `uuid`, `identifiers`, `wire`,
`account-contract`. **No `specta`, no `specta-typescript`.**

**`domain` — client-only interpretation.** Moves out of `sync-common`:

- `MutationBody` and `StateKey`
- `VersionKey` (client-side merge decisions; the server orders in SQL)
- `library_state`: `BookLifecycleState`, `BookMetadataState`, `ReadingPositionState`
- `reader_state`: `BookmarkState`, `BookLinkState`
- `annotations`: `AnnotationState`, `AnnotationStyle`, `AnnotationAnchor`,
  `PdfAnnotationAnchor`, `PdfAnnotationRect` — and the `specta` dependency with them
- `conflict_rank()` and `state_key()`
- `reduce_same_replica_mutations`

`policy()`, `may_replace`, `StatePolicy`, `MergePolicy`, and `ReplacementPolicy` do
**not** move — plan 39 step 4 deletes them.

The invariant this buys: a server crate cannot name a domain type, because it does
not link the crate that defines one.

## Changes

1. Confirm plan 39 landed the `present`/`content_hash` columns and the boolean-flip
   refcount. If it did not, stop — moving the domain types would leave the server
   unable to count references at all.
2. Move the modules listed above from `SyncCommon/src/` to `BokheimDomain/src/`,
   preserving file names (`library_state.rs`, `reader_state.rs`, `annotations.rs`).
   Move `MutationBody`/`StateKey`/`VersionKey`/`conflict_rank`/`state_key`/
   `reduce_same_replica_mutations` out of `lib.rs` into a new
   `BokheimDomain/src/mutations.rs`.
3. Move `specta` and `specta-typescript` from `SyncCommon/Cargo.toml` to
   `BokheimDomain/Cargo.toml`. Verify the mobile app's binding generation still
   resolves — it consumes these types through whichever crate `BokheimAppContract`
   re-exports, so that re-export is the thing to check.
4. Update `use` paths in the client crates that consume the moved types: `Backend`,
   `Backend/crates/book-metadata`, `Backend/crates/projection`, `BokheimSyncClient`,
   `BokheimAppContract`, `BokheimOpds`. Prefer adding `domain` as a direct
   dependency over re-exporting the moved types from `sync-common`, which would
   defeat the split.
5. Remove `sync-common`'s now-unused re-exports and confirm nothing in `servers/sync/`
   references a moved type. Move the server's test fixtures that construct
   `BookLifecycleState`/`BookMetadataState` (`sync_store.rs:511-583`) to build opaque
   byte payloads instead — a server test should not need the domain crate either.
6. Add `domain` to the server workspace's forbidden-dependency check if one
   exists; if not, this plan's guard is test 1 below.

## Tests

1. **The server cannot link the domain crate.** A check that fails if any crate under
   `servers/sync/` gains a direct or transitive dependency on `domain`. `cargo tree
   -p server-postgres -i domain` returning non-empty is the failure condition.
   This is the only durable enforcement of the plan's invariant — without it the
   split decays back to a convention.
2. **`specta` is out of the server graph.** `cargo tree -p Server -i specta` is empty.
   Concrete and easy to assert; also the measurable build-time win.
3. **Wire round-trip is unchanged.** Encode a mutation of every kind through
   `sync-common`, decode through `domain`, assert equality with the
   pre-split fixtures. Guards against a serde representation shifting during the move
   — a renamed module can silently change a `#[serde(rename_all)]` tag path.
4. **No server crate reaches a domain type through a dev-dependency either.** Test 1
   must cover `cargo tree -e all`, not just normal edges — the server's own fixtures
   at `sync_store.rs:511-583` construct `BookLifecycleState` today, and step 5 moves
   them to opaque byte payloads. A dev-dependency edge would silently satisfy a
   normal-edge check while leaving the coupling in place.

   (Refcount behaviour itself is plan 39's test 8, not this plan's.)

## Sequencing note

Plan 35 relocates `SyncCommon` to `crates/sync-common` — a path move with no split.
The two do not conflict, but doing 35 first means this plan lands on the new path and
avoids re-editing the same `Cargo.toml` set twice. If 35 is not imminent, do this
plan first and let 35 move both crates together.

## Non-goals

- **Splitting `domain` further.** It already holds unrelated concerns
  (author identity, contributor roles, paths). Tidying that is plan 35's territory.
- **Changing any wire representation.** If a payload's bytes change, this plan has
  overreached; test 3 exists to catch that.
