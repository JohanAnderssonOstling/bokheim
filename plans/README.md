# Plan index

Each plan is self-contained: a fresh session can execute it without conversation history.
Update the Status column as work lands (✅ done · 🔶 partial · ⬜ open · 💤 deferred-by-design).

## Status overview

| # | Plan | Status | Depends on |
|---|------|--------|------------|
| 15 | [FIX: orphan path truncation](15-fix-orphan-path-truncation.md) | ⬜ open — **bug** | — |
| 33 | [Separate self-hosted blob service](33-separate-blob-service.md) | 💤 deferred | — |
| 35 | [Repository structure cleanup](35-repository-structure-cleanup.md) | ⬜ open | checkpoint current work |
| 38 | [Placement → add-remove set; filename/hash on the book](38-placement-set-and-book-filename.md) | ⬜ open — **bug**; its filename register is superseded by 40 | — (unblocks 39) |
| 39 | [Collapse nine sync tables into two generic tables](39-generic-sync-state-table.md) | ⬜ open | **38, 15** (hard) — eight tables now, not nine |
| 40 | [Drop synced filename register; derive names from metadata](40-metadata-derived-filenames.md) | ✅ done | landed without 38; nothing here waits on it |
| 41 | [Split `sync-common` into wire + client-only domain crate](41-split-sync-common.md) | ⬜ open | **39** (hard); sequence with 35 |

## Suggested order

1. **15** — known path-resolution bug.
2. **38** — the placement/set fix; skip its filename register, which 40 removed.
3. **39** — wants 38 and 15 landed first.
4. **41** — follows 39; sequence with 35.
5. **35** — repository structure; wants a checkpointed working tree.
6. **33** — optional deployment split; wake only when storage capacity, bandwidth,
   or independent scaling justifies a second service.

## What 40 landed

A book's format is now synchronized state (`BookLifecycleState::Present.format`,
`book.format`) and download filenames are derived from metadata through a
collision-aware ladder in `Backend/src/library/filenames.rs`. No filename register
exists on the wire or in the schema, and scans record observed names verbatim.

Two notes for whoever picks up 38 or 39:

- 40's test 4 misstates the ladder: with an author and a year present, the fourth
  book with identical metadata reaches the `[book_id]` rung, not the second. The
  test in `filenames.rs` asserts the real sequence.
- A placement still keeps naming a file that was deleted while tracking a
  surviving copy's fingerprint, so that book will not open. Unrelated to naming;
  it belongs with missing-file recovery.

## Conventions

- Plans state decisions, not options — implementing sessions should not re-open settled
  questions (e.g. `.jpg` extension, DB resolution over hex-derivation, remove-wins ties).
- Synchronized state must be sufficient to reconstruct the projected entity.
- "Which book?" → opaque `BookId`; "which bytes?" → opaque `ContentHash`. `BookId` owns
  parsing and its canonical decimal-string encoding for SQLite, sync, JSON, and sessions;
  callers only carry the type. `ContentHash` accepts only full lowercase BLAKE3 digests;
  missing legacy values use `Option<ContentHash>` and synthetic tombstone keys stay strings.
  Never mix the identities or their encodings.
- Blob persistence uses closed `BlobKind`, `TransferJobKind`, and `BlobState` domains. Raw
  storage strings and integers are confined to the SQL adapter; callers request named state
  transitions rather than supplying magic values.
- Pre-release assumption: server event log and dev databases may be wiped instead of writing
  legacy-format migration code where the current schema supersedes the old one.
- Each plan carries its own test list.
