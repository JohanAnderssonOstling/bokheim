# Plan 27: Timestamp unit convention (documentation-mostly)

Atomic, tiny. No behavior change.

## Problem

Three units coexist, internally consistent but documented nowhere:
- epoch **micros**: `book.added_at`, `book.last_read`, `deleted_at` (where real), book_opened
  payload `last_read`;
- epoch **millis**: `event_log.timestamp`, HLC `pt` (`changed_at` on the wire);
- epoch **seconds**: `transfer_jobs.*`, `blob_state.last_checked`, `blob_local_ref.last_seen`.

One future `now_ts_secs()` pasted into a micros column = silent 50-year skew that LWW happily
honors.

## Change

1. Comment block at the top of `create_library.sql` declaring the unit per column group, and
   on `now_ts_secs`/`now_ms` in Rust.
2. Rename the Rust helpers to carry units (`now_us`, `now_ms`, `now_s`) and route the
   scattered inline `SystemTime::now()...as_micros()` expressions (db.rs has several) through
   them — grep proves no raw `duration_since(UNIX_EPOCH)` remains outside the three helpers.
3. Explicitly **do not** convert columns to one unit — not worth the migration; the convention
   plus helpers is the protection.

Acceptance: grep-clean per (2); units documented per (1).
