# Plan 10: Small cleanups (Backend)

Independent, low-risk items; can ride along with whichever plan touches the same file.

1. **Tolerant libraries.csv parsing** (`Backend/src/home.rs`). `get_library_entries` panics on
   any malformed CSV row (`Reader::from_path(...).unwrap()`, per-row `res.unwrap()`), while the
   tolerant `read_library_entries` sits in the same file. Unify on the tolerant one (filter_map
   + log a warning per skipped row), delete the duplicate.

2. **epub.rs hygiene** (`Backend/src/epub.rs`).
   - `get_epub_id_title_author` returns an always-empty `id` — drop it from the tuple and
     rename to `get_epub_title_author` (single caller in `scanner.rs`).
   - Map empty extracted title/author to `None`: an EPUB with an empty `<dc:title>` currently
     stores `title = ""`, which displays blank *and* (being non-NULL) permanently blocks
     re-extraction via `get_book_paths_without_title`.
   - (Multi-creator extraction is plan 6's job, not this one.)

3. **Delete `generate_file_hash`** (`Backend/src/library.rs`). Legacy first+last-16KB
   fingerprint, superseded by `db::local_file_fingerprint` (stat-based). Verify zero callers
   first (grep workspace, including UI/MobileApp). Two coexisting "local fingerprint"
   definitions is how scan-skip bugs are born.

4. **Debug output → log.** `println!("Creating {library_path}")` and `dbg!("Before scan")`
   (`library.rs`), `println!("Csv: ...")` (`home.rs` `remove_library`),
   `println!("transfer worker error: ...")` → `log::error!` (`transfer.rs` worker loop),
   `eprintln!` in `db.rs` (`get_book_dir_by_hash`, `get_books_without_title`).

5. **Scanner Drop joins** (`Backend/src/scanner.rs`). `Drop` joins the hash threads but only
   `take()`s (detaches) watcher/worker/thumbnail handles. They all exit on `should_finish`
   within their poll timeouts — either join all four groups or add a one-line comment stating
   detaching is intentional. Prefer joining (deterministic shutdown, flushes thumbnail batch).

6. **Redundant log filters** (`Backend/src/lib.rs` `init_logging`): the per-module
   `filter_module(..., Warn)` calls match the global `filter_level(Warn)` — delete them or
   lower the global to Info and keep the module overrides (decide which behavior was wanted).

Explicitly deferred (known, not worth doing now):
- Eager `Book.cover: Vec<u8>` loading in list queries — revisit as a lazy path/handle when
  large folders make it measurable.
- `read_pos`/`read_progress` cleanup and other schema items live in plans 1–6 follow-ups
  ("small fixes" list), not here.
