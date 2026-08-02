# Plan 21: Missing-file recovery on open (and mobile presence reality)

Touches `Backend/src/library.rs::open_book_reader`, `db.rs`. Especially relevant on mobile,
where `scanner_stub` means nothing ever reconciles disk reality.

## Problem

`is_downloaded = 1` is trusted blindly. If the file is gone (OS storage eviction, user deleted
it via a file manager on mobile where there is no scanner, SD card unmounted), `open_book_reader`
returns a dead error and the state never self-corrects. On desktop the scanner eventually
notices; on mobile, never.

## Change

1. `open_book_reader`: on `File::open` NotFound for a placement marked downloaded —
   - set `is_downloaded = 0` for that placement (its file is demonstrably absent),
   - update `blob_local_ref` for its content hash at that path to `exists = 0`,
   - call `request_download(book_id)` and `request_sync()` (plan 20),
   - return a typed `BookNotDownloaded` error (distinct from "unknown book") so the UI can
     show "downloading…" instead of a failure.
2. **Caution — do not emit a placement removal**: a missing file is a *blob* problem, not a
   placement removal; the placement row stays live. (The desktop vanish-sweep is the only
   thing allowed to tombstone, with its grace window; this path must not shortcut it.)
3. Mobile-only cheap presence check: on app start (or library open), verify file existence for
   downloaded placements (a stat-walk over placements, not a content scan) and downgrade
   `is_downloaded`/`blob_local_ref` accordingly. Desktop skips this (scanner owns it).

## Tests

1. Downloaded placement, file removed, open → `BookNotDownloaded`, `is_downloaded = 0`,
   download pending; **no `book_dir_removed` event emitted** (the critical assertion).
2. Presence check downgrades a missing file and leaves present files untouched.
