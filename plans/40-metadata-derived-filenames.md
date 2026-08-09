# Plan 40: Drop the synced filename register; derive materialized names from metadata

**Status:** ⬜ open — **assumes plan 38 is implemented; does not assume plan 39**

Touches `SyncCommon/src/lib.rs`, `servers/sync/crates/server-database/schema/03_sync.sql`,
`servers/sync/crates/server-postgres/src/{sync_store.rs,sql/sync/}`, and
`Backend/src/{platform/scanner.rs,sync/transfer.rs,library/database/}`.

**No migration.** Deployment wipes the database; do not write backfill code.

This plan deletes `MutationBody::BookFileName` — a register plan 38 introduces. That
is deliberate, not an oversight: the decision was taken after 38 was already being
implemented, and 38 was left alone rather than edited mid-flight. **Before starting,
verify what 38 actually landed**, since every deletion below is defined against its
step list rather than against observed code.

Plan 39 is *not* assumed. The server still has one table per kind, `MutationBody` is
still the wire type, `decode_operation` still has per-kind arms, and `fetch_page.sql`
is still a `UNION ALL` with a branch per table. This plan removes one of each.

## Problem

Plan 38 syncs a per-book `file_name` register so a device downloading a book can name
the file as the origin device named it. Weighed against what it costs, that does not
pay:

1. **It only ever affects newly downloaded files.** Phase 2 (renaming existing files
   to match the register) was excluded by design, so the register never corrects a
   file already on disk. Its entire effect is choosing a name at download time.
2. **It forces a decision point into the scanner.** Plan 38's rename-detection rule —
   compare the observed name against the *previously recorded* `book_dir.file_name`,
   never against the synced register, or partially-synced devices ping-pong renames —
   is the subtlest reasoning in that plan, and it exists only to feed point 1.
3. **Metadata already gives a better name deterministically.** Title, contributors,
   and publication year are synced, immutable (a pure function of the file's bytes),
   and identical on every device. A name derived from them needs no register, no
   propagation, and no scanner decision.

What is genuinely lost: a device that downloads a book no longer reproduces the
origin device's filename for it. Since scanned files are never renamed either way,
the two devices already diverge for every book that originated locally; this extends
that divergence to downloaded books. The names are consistent and predictable
instead of user-chosen. Accepted.

## Design

**Nothing about filenames is synced. Nothing on disk is ever renamed.**

`book_dir.file_name` stays exactly what plan 38 made it — a purely local column — and
is written by exactly two paths:

- **Scan**: whatever name was observed on disk, recorded verbatim.
- **Download**: the name produced by `materialize` below.

The scanner therefore never compares an observed name to anything. There is no
"did the user rename this?" question to answer, so plan 38's rename-detection rule is
deleted rather than simplified.

**`materialize` produces the shortest name that is free in the target directory.**

```
materialize(metadata, format, collision_probe) -> FileName
```

A filename carries no more than it needs. Try each rung in order and stop at the
first whose sanitized stem is unclaimed in the target directory:

```
1.  <title>.<ext>
2.  <title> - <author>.<ext>
3.  <title> - <author> (<year>).<ext>
4.  <title> - <author> (<year>) [<book_id>].<ext>
```

- author = first contributor with role `aut`; a rung whose components are all absent
  is skipped rather than rendered with empty separators
- year from `PublicationMetadata`
- `collision_probe` reports whether a name is taken in that directory — by a file on
  disk *or* by a `book_dir` row for a pending download, so two concurrent downloads
  cannot pick the same rung

Rung 4 always terminates: `book_id` is unique per book, and the in-folder duplicate
rule means one directory never holds two entries for the same book. Prefer `book_id`
over a hash prefix here — it is exactly as intrinsic and cannot collide by truncation.

**Fallback**: if the title sanitizes to nothing, the stem is the `book_id` alone. That
always exists.

**Stability, not purity.** The chosen rung depends on what else is in the directory
when the file is written, so it is not a pure function of the book. Three properties
make that safe, and each has a test below:

- **Idempotent**: re-materializing a book that already has its file returns the same
  name, so a repeated download never creates a second file.
- **Non-retroactive**: an incumbent is never renamed. If `Dune.epub` exists and a
  second edition arrives, the newcomer escalates to rung 2; the incumbent keeps rung 1
  permanently. Names never oscillate.
- **Terminating**: rung 4 cannot fail.

The visible consequence is asymmetric pairs — `Dune.epub` beside
`Dune - Frank Herbert.epub` — and different devices resolving the same two books
differently depending on download order. Both are cosmetic, and both are already
implied by never renaming.

**Sanitization**, applied to the rendered stem:

- replace path separators and characters illegal on FAT/exFAT/NTFS (`< > : " / \ | ? *`)
  and control characters
- collapse runs of whitespace; trim leading/trailing whitespace and trailing dots
  (Windows rejects both)
- reject reserved stems (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`)
- cap the stem so stem + extension fits the filesystem limit, truncating on a
  character boundary. Apply the cap per rung, and on rung 4 truncate the *title
  portion* only, so the `book_id` that guarantees uniqueness survives

**Collision probing is download-only.** For scanned files the filesystem already
guarantees path uniqueness, so there is nothing to resolve. `collision_probe` is
consulted on the download path alone.

## Changes

Each item names what plan 38 introduced and removes it.

### SyncCommon

1. Delete `MutationBody::BookFileName` (38 step 2) and its `StateKey` variant.
2. Delete its arm from `policy()` (38 step 3). No other arm changes — placement and
   everything else keep the unconditional-LWW policy 38 established.
3. Remove `file_name` from wherever 38 step 12 seeded it on the book record, if it
   added a local `book.file_name` column; the local column `book_dir.file_name` is
   the only filename storage that remains.

### Server

4. `03_sync.sql`: drop the `sync_book_file_name_state` table (38 step 6), its
   `(library_id, server_seq)` index, and its three triggers.
5. Delete the upsert SQL file 38 added for it, and its `UNION ALL` branch in
   `fetch_page.sql`.
6. `sync_store.rs`: delete its encode arm and its `decode_operation` arm. Since plan
   39 is not assumed, both are still per-kind matches and both need the arm removed.

### Backend

7. `scanner.rs`: delete the rename-detection rule (38 step 15) and the
   `BookFileName` emission. The scanner records observed names into
   `book_dir.file_name` and nothing else. **Keep** the in-folder duplicate rule from
   38 — lexicographically smallest filename is indexed, the other left on disk and
   reported — it is unrelated to naming.
8. `remote_reducer.rs`: delete the `BookFileName` arm (38 step 12).
9. `encode_trigger_mutation` / `bokheim_mutation` (`mod.rs:175-215`): delete the
   `"book_file_name"` arm (38 step 17).
10. `sync_state_identity` (`remote_reducer.rs:153`) and its copy at
    `sync_state.rs:73`: delete the `BookFileName` arm from **both** (38 step 18).
11. `create_library.sql` `sync_outbox`: remove `'book_file_name'` from the
    `state_kind` CHECK (38 step 16).
12. `sync/transfer.rs`: the download target becomes
    `dir_path + materialize(metadata, format, probe)` instead of
    `materialize(book.file_name, collisions)` (38 step 19). Everything downstream —
    inserting the `book_dir` row before the transfer so `rel_path` resolves — is
    unchanged.
13. Implement `materialize` as specified above, in one module, as a pure function
    taking metadata plus a probe closure. It must not read the database directly;
    the caller supplies the probe so the function is testable without one.

### Tests

14. Delete 38's test 4 ("stable folder produces zero mutations") and test 5 ("copies
    with different local names do not fight"). Both assert properties of the
    rename-detection rule and become vacuous.

## Tests

1. **Shortest free rung.** With an empty directory a book is named `<title>.<ext>` —
   no author, no year, no id. The plan's headline property: a filename carries no
   more than it needs.
2. **Idempotence.** Re-materializing a book whose file already exists returns that
   same name rather than escalating past it. Without this a repeated or resumed
   download silently creates a second file.
3. **Non-retroactive escalation.** Place book A as `Dune.epub`, then materialize
   book B with the same title: B lands on rung 2, A is untouched. Re-run the whole
   sequence and nothing oscillates.
4. **Termination.** Two books with identical title, author, and year resolve to
   distinct names, the second on rung 4. Then a third with the same metadata also
   resolves. No rung can loop or exhaust.
5. **Sanitization.** Titles containing `:` `/` `?` `|` `"` `*`, control characters,
   a trailing dot, leading/trailing spaces, and a reserved stem (`CON`) all produce
   names legal on FAT/exFAT/NTFS and on ext4.
6. **Length.** A very long title truncates on a character boundary and keeps its
   extension. On rung 4, assert the `book_id` survives truncation — truncating it
   away would break the uniqueness guarantee that makes the ladder terminate.
7. **Missing metadata.** Absent author skips rung 2; absent year skips rung 3;
   absent or all-punctuation title falls back to the `book_id` stem. Each case is
   deterministic.
8. **Scan path is untouched.** A file discovered by the scanner keeps its on-disk
   name verbatim in `book_dir.file_name`, regardless of what `materialize` would have
   produced for that book. Guards against someone "helpfully" normalizing scanned
   names later.
9. **A scanned file occupies a name.** A user-named `Dune.epub` already on disk makes
   a downloaded book with title "Dune" escalate to rung 2. The probe must see the
   filesystem, not just app-managed rows, or the download collides with a real file.
10. **No filename traffic.** A rename on disk produces no outbox row of any kind
    after the next scan. The register is gone; this asserts nothing crept back in.

## Non-goals

- **Renaming existing files to match the template.** Same exclusion plan 38 made for
  the register, for the same reason: it needs per-file last-written-name tracking and
  reintroduces the feedback-loop risk where a partially-synced device reads its own
  output back as a user edit.
- **Making filenames match across devices in general.** They match for downloaded
  books and diverge for locally scanned ones. Closing that gap means renaming, above.
- **Changing the template later.** Not forbidden, but note it is sticky: new
  downloads use the new scheme, existing files keep the old, and nothing reconciles
  them. Pick one and keep it.

## Effect on plan 39

One fewer kind to migrate: 39 step 6 collapses eight tables rather than nine, and its
`fetch_page.sql` union has one fewer branch. No other change — 39 does not depend on
anything this plan touches.
