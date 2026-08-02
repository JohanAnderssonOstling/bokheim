# Plan 25: Blob & thumbnail lifecycle (orphan cleanup + location tracking)

Two independent halves; do (1) anytime after plan 7, defer (2) until a feature needs it.

## 1. Local thumbnail orphan cleanup (small, near-term)

Thumbnails of books with no live placement are never deleted. After plan 7 (content_hash
keying): a periodic sweep (e.g. end of scan, or sync) lists `thumbnails/*.jpg`, deletes files
whose hash matches no live placement's `content_hash` (`get_book_assets` complement). Cheap,
bounded, local-only — no sync interaction.

Test: book's last placement tombstoned → next sweep removes its thumbnail; live books' files
untouched.

## 2. Server blob GC via location tracking (deferred, borrowed from git-annex)

The server stores blobs forever; nothing ever deletes one. Safe GC needs to know a blob is
unreferenced: no live placement in the library references its hash. The server can derive
references from its own event log (placement adds/removes carry content_hash) **after** plan 24
exists (or by replaying the full log). Per-device presence tracking ("which devices hold which
blob") is the fuller git-annex model and additionally unlocks:
- "this book is on your e-reader but not here" UI,
- per-device auto-download policies,
- refusing to GC the only copy.

Sketch: clients report blob presence diffs during sync (they already compute exactly this in
`sync_assets` pass 1); server table `(library, device, hash, present, last_seen)`. Design with
plans 23/24 (same device-liveness bookkeeping). Not before.
