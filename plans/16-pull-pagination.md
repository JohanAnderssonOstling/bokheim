# Plan 16: Paginated pull (borrowed from CouchDB checkpointed replication)

Atomic; touches `Server/src/db.rs::fetch_changes`, `Server/src/routes/sync.rs`,
`Backend/src/sync.rs::pull_events_from_server`. Plan 12's fake server should grow a `limit`
knob to exercise this.

## Problem

The pull returns *everything* past the cursor in one response and applies it in one
transaction — a fresh device with a large library gets one giant body, one giant tx, and an
all-or-nothing apply.

## Change

1. Server: `fetch_changes` takes `limit` (e.g. 500), returns rows + `has_more` flag (or the
   client infers from `len == limit`).
2. Client: loop — pull page, apply in one tx (deferred FKs as today), advance
   `last_pull_server_seq`, commit, repeat until empty page. Materialization
   (`dir_upserts`/`dir_deletes`/`removed_placements`) runs **per page, after that page's
   commit** — the closure invariant holds per committed prefix (documented at sync.rs:432),
   so each page is self-consistent.
3. Keep page size server-controlled with a sane default; client sends no preference.

## Tests

1. Fake-server/limit harness: 1200 events, page 500 → three transactions, cursor advances per
   page, final state equals unpaginated apply.
2. Failure mid-stream: make page 2's apply fail → cursor remains at page 1's end; retry
   completes. (This is the closure-invariant regression test.)
