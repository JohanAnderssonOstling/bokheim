# Plan 11: DB-layer small fixes (schema hygiene)

Independent, low-risk. From the original schema review; previously tracked only in
conversation. Items are unrelated to each other — cherry-pick freely.

1. **`deleted_at` always a real timestamp.** `mark_dir_deleted` / `mark_book_dir_deleted`
   (`Backend/src/database/library.sql`) and `apply_remote_book_dir_removed`'s tombstone upsert
   write literal `1`; the scanner sweep and `mark_book_deleted` write epoch-micros. Standardize
   on epoch-micros (pass `:ts`) so future tombstone GC by age doesn't see 1970. Check whether
   the plan-5 cascade implementation already fixed the dir/book_dir paths.

2. **Prune pushed `event_log` rows.** The push cursor (`last_pushed_local_seq`) only advances;
   rows are never deleted. After a successful push in `push_local_events`
   (`Backend/src/sync.rs`), run `DELETE FROM event_log WHERE local_seq <= :cursor`. Verify
   nothing else reads old rows first (`get_events_after_local_seq` — delete it if unused).
   Note: plan 1's `should_skip_singleton_apply` only queries rows *above* the cursor, so
   pruning is compatible.

3. **`read_pos` / `read_progress` cleanup** (`create_library.sql`, `library.sql`).
   - `read_pos TEXT DEFAULT "0"` → `'0'` (double quotes are SQLite's identifier-fallback
     misfeature).
   - `read_progress INTEGER` → `REAL` (Rust reads it as `f32`).
   - The two carousel queries duplicate a CAST cascade translating legacy numeric `read_pos`
     values into a percentage. Make `read_progress` the single display-progress source
     (written whenever `read_pos` is) and reduce the queries to `book.read_progress`.
     Pre-release: acceptable for legacy rows to show 0% until next opened.

4. **No panics on bad rows.** Row mappers call `book_id.parse().unwrap()`
   (`db.rs`: `get_dir_contents`, `get_recently_read`, `get_recently_added`,
   `get_author_books_grouped`, `get_books_without_title`) — one non-numeric id in the DB kills
   the library view. Switch to parse-or-skip with a `log::warn!`.

5. **Versioned migrations.** Replace `reset_development_schema` + ignored-error `ALTER`s in
   `init_db` with `PRAGMA user_version`-gated migration steps, so a real migration error is
   loud instead of swallowed by the "already applied" path. Do this last — it touches every
   prior migration and is easiest once the schema has settled (after plans 4–6 land).

6. **Stale dev-reset drops.** Whenever a trigger/view is renamed (plans 4, 6),
   `reset_development_schema`'s DROP list and `triggers.sql`'s DROP header must stay in sync —
   verify they only reference current names and prune dead ones (item 5 eventually deletes
   this mechanism entirely).
