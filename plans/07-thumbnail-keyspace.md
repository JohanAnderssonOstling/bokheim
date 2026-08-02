# Plan 7: One thumbnail keyspace (content_hash) + remove the APP_DATA_DIR global

Independent of plans 1–6. Typed transfer identities are already in place; this completes the
thumbnail side of the same seam. Pre-release: wipe the thumbnails dir once as part of rollout;
stale files regenerate on next scan / re-download.

## Problems

1. **Two naming schemes in one directory** (`<library>/.bokheim/thumbnails/`):
   - Local generation writes `{book_id}.jpg` (`generate_thumbnail_to_app_data`,
     `Backend/src/library.rs`), and the scanner registers it in `blob_local_ref` with
     `hash = book_id` (`scanner.rs` thumbnail flush).
   - Sync uploads/downloads use `{content_hash}.png` (`thumbnail_path_from_content_hash`,
     `Backend/src/transfer.rs`; `sync_assets` pass 1 in `sync.rs`).
   `resolve_thumbnail` (`home.rs`) only reads `{book_id}.jpg`, so **downloaded thumbnails are
   never displayed**, and the book_id-keyed `blob_local_ref` rows reach state 1 and get
   **uploaded under the wrong server key** (server is content_hash-keyed). `book_id` (decimal
   u64 = first 8 digest bytes) and `content_hash` (64-char hex) are never string-equal.
2. **`APP_DATA_DIR` is a process-global OnceLock set per library** (`home.rs:17`,
   `library.rs` `set_app_data_dir`, error ignored). The second library opened in one process
   silently reads/writes the first library's thumbnail dir. `libraries.csv` supports multiple
   libraries, so this is a live bug.

## Changes

1. **Single scheme**: `{content_hash}.jpg`. Change `thumbnail_path_from_content_hash` from
   `.png` to `.jpg`; it becomes the only path builder.
2. **Scanner**: `HashReady` already carries the freshly computed `content_hash` — add it to
   `ThumbnailTask`, generate to the content-hash path, and register
   `blob_local_ref("thumbnail", content_hash, path, ...)` in the flush (today it keys by
   book_id). This automatically ends the wrong-key uploads: the state-1 sweep in `sync_assets`
   only ever sees content_hash keys afterward.
3. **One generator**: merge `generate_thumbnail` and `generate_thumbnail_to_app_data`
   (near-identical ~70-line twins; the former writes JPEG bytes into a `.png` name) into one
   `generate_thumbnail(library_path, epub_path, content_hash)`. Delete `get_cover_path` if
   unused.
4. **Lookup**: thumbnail resolution becomes a `Library` method (it has the pool and its own
   path): book_id → `db::get_book_hashes` → first existing `{hash}.jpg`. Update all callers of
   `home::get_thumbnail` (library.rs cover loading; grep UI/MobileApp for direct users).
5. **Delete the global**: remove `APP_DATA_DIR`, `set_app_data_dir`,
   `get_app_data_dir`, `get_app_data_thumbnails_dir`, `home::get_thumbnail`/`resolve_thumbnail`.
   The scanner already holds `library_path` in `ScannerState`; thread it where needed.

## Tests

1. Scanner flush registers thumbnail `blob_local_ref` keyed by content_hash with the `.jpg`
   path.
2. `Library` thumbnail resolution: book with a placement whose `content_hash = H` and a file
   `thumbnails/H.jpg` → returns it; no file → None; book with synthetic non-hex hash → None
   without panicking.
3. Two `Library` instances in one process resolve thumbnails under their own paths.
