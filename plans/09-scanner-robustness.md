# Plan 9: Scanner robustness — no panics, no permanent wedge, no duplicated dir markers

Independent of all other plans. All changes inside `Backend/src/scanner.rs` (+ small touches in
`library.rs`/`db.rs`).

## Problems

1. **Panics on filesystem races / read-only trees / odd names.** A file deleted between the
   walk and its use, an unwritable folder, or a non-UTF8 filename panics a scanner or
   hash-worker thread:
   - `db::local_file_fingerprint(...).unwrap()` (`scanner.rs` `get_book_dir`)
   - `File::open(path).unwrap()` in `generate_content_hash_blake3` (`library.rs`)
   - `fs::write(uuid_path, ...).unwrap()` for `.biblos_uuid` markers (`scanner.rs`)
   - `entry.file_name().to_str().unwrap()` (`scanner.rs` dir + file branches) — **non-UTF8
     filenames panic the walk**; skip such entries with a warning (they can't round-trip
     through the TEXT schema/JSON payloads anyway).
2. **A dead hash worker wedges scanning forever.** A panicked worker never removes its entry
   from `pending_hash_jobs`; `drain_hash_results_on_scanner_thread` loops while
   `pending_len > 0`, so the scanner thread spins there indefinitely — and since `worker_loop`
   is inside `scan_library`, **no scan ever runs again** until app restart.
3. **Copying a folder duplicates its identity.** `cp -r` copies `.biblos_uuid`, so two physical
   folders carry the same dir uuid. Each scan upserts the same `dir` row with whichever folder
   the walk visits last → `parent_id`/`name` flip-flops every scan, emitting a dir event each
   time, which syncs the flip-flop to every device.

## Changes

1. **Fallible hashing/fingerprinting.** `local_file_fingerprint` already returns `Result` —
   stop `unwrap()`ing it; on Err, log at debug and skip the file (next scan retries).
   Change `generate_content_hash_blake3` from `(BookId, ContentHash)` to
   `io::Result<(BookId, ContentHash)>`.
2. **Worker accounting that always closes.** The hash worker must remove the task from
   `pending_hash_jobs` (or send a failure result that does) on *every* exit path: hash error,
   metadata error, send error. Simplest shape: make `HashReady` an enum
   `{ Done(fields...), Failed { file_hash } }`; the drain loop removes `Failed` from pending
   and moves on.
3. **Drain safety valve.** In `drain_hash_results_on_scanner_thread`, track time since the last
   received result; if pending > 0 and nothing has arrived for ~30s, log an error listing the
   stuck count, clear `pending_hash_jobs`, and exit the loop. This bounds the damage of any
   future accounting bug to one slow scan instead of a dead scanner.
4. **Marker write failure.** If `.biblos_uuid` can't be written (read-only mount), do *not*
   proceed with a fresh uuid (it would mint a new dir identity every scan → row churn + event
   spam). Log an error once and skip descending into that directory's identity handling —
   treat it like an unreadable dir.
5. **Duplicate marker detection.** Keep a `HashSet<String>` of marker uuids seen during the
   current walk. On a duplicate: mint a fresh uuid, rewrite the *second* folder's marker, and
   use the new uuid (first-seen keeps the identity). Log info. Also `trim()` the marker file
   content on read (defends against editor-added newlines).

## Tests

1. Hash worker receives a task whose file no longer exists → drain loop terminates, scan
   completes, file simply absent this scan (no panic, no wedge). (Testable by sending a
   `HashTask` for a bogus path through the real channels, or by unit-testing the
   `Failed`-variant accounting.)
2. Duplicate markers: two temp folders containing the same uuid string → after one scan, the
   second folder's marker file content differs and two distinct `dir` rows exist; a second scan
   emits no dir events (stable).
3. `generate_content_hash_blake3` on a missing path returns Err (no panic).
