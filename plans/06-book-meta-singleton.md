# Plan 6: Sync book metadata as a `book_meta` singleton, with multi-author credits

Requires plan 1 (singleton guard). Independent of the dir plans (3–5). Server: no code change
(singleton routing is automatic once the type is registered); wipe dev `library_events` as in
plan 4 — the payload shape must not be retrofitted later.

## Problems

1. `update_book_metadata` (`Backend/src/db.rs`) writes `title`/`author_id` with a raw UPDATE — no
   trigger, no event — so metadata never syncs; devices without the blob show "Untitled".
2. `book.author_id` is a single FK and `epub.rs:15` keeps only the *first* author-role creator:
   co-authors, translators, editors, and `opf:file-as` sort names are discarded even though
   `epub_provider` already supplies them.
3. `author.id` is a local AUTOINCREMENT and must never appear in a sync payload.

## Design

- One `book_meta` singleton per book; the payload is the **full ordered credit list** plus title —
  always complete (build-from-scratch rule), resolved server-side by HLC like any singleton.
- Authors stay **name-keyed and local**: receiving devices resolve names to local `author` rows.
  No author entity sync, no merge feature (deliberate — see conversation/plans history).
- The credit list is an LWW register: applied by delete-and-rebuild, never diffed per credit.
- `book.author_id` is kept as the denormalized *primary* author (first author-role credit) so the
  carousels and `get_books_in_dir` need no query changes.

Payload:

```json
{ "title": "...", "authors": [
    {"name": "Terry Pratchett", "sort": "Pratchett, Terry", "role": "author"},
    {"name": "Neil Gaiman",     "sort": "Gaiman, Neil",     "role": "author"}
] }
```

Array index = position. `book_added` shrinks to identity + `added_at`; title ownership moves
entirely to `book_meta`.

## Changes

### Schema (`create_library.sql` + opportunistic ALTER in `init_db`, matching existing style)

```sql
-- on author:
ALTER TABLE author ADD COLUMN sort_name TEXT;  -- ignore error if present

create table if not exists book_author(
    book_id    TEXT NOT NULL REFERENCES book(id) ON DELETE CASCADE,
    author_id  INTEGER NOT NULL REFERENCES author(id) ON DELETE CASCADE,
    role       TEXT NOT NULL DEFAULT 'author',
    position   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (book_id, author_id, role)
);
```

PK permits the same person in two roles. No backfill from `book.author_id` — see Extraction.

### Extraction (`Backend/src/epub.rs`, `Backend/src/scanner.rs`)

- Return all creators as `Vec<(name, sort_name, role)>` (map `epub_provider::CreatorRole` to the
  role strings; `file-as` → sort_name). Keep deterministic order (declaration order).
- `update_book_metadata(tx, book_id, title, creators)`: resolve/insert each author
  (`get_or_create_author_id` extended to upsert `sort_name` when non-empty), delete + reinsert the
  book's `book_author` rows with positions, set `book.author_id` to the first author-role credit
  (NULL if none), **then** UPDATE `book.title`/`author_id` last — the trigger reads the credit
  rows, so they must be in place when it fires.
- Re-extraction condition (`get_book_paths_without_title`): change to
  `title IS NULL OR NOT EXISTS (SELECT 1 FROM book_author ba WHERE ba.book_id = b.id)` — every
  already-scanned book re-extracts once to pick up full credits (pre-release; cheap).

### Triggers (`triggers.sql` + drop list in `reset_development_schema`)

- `trig_book_meta`: AFTER UPDATE OF `title`, `author_id` ON book, WHEN
  `(OLD.title IS NOT NEW.title OR OLD.author_id IS NOT NEW.author_id)` AND change_origin local.
  Payload builds the array from the join:

```sql
json_object('title', NEW.title, 'authors',
  (SELECT json_group_array(json_object('name', name, 'sort', COALESCE(sort_name,''), 'role', role))
   FROM (SELECT a.name, a.sort_name, ba.role FROM book_author ba
         JOIN author a ON a.id = ba.author_id
         WHERE ba.book_id = NEW.id ORDER BY ba.position)))
```

- Companion AFTER INSERT ON book WHEN `NEW.title IS NOT NULL` (covers `insert_book_with_title`;
  credits may be empty at that instant — the later metadata update re-emits the full list).
- `trig_book_added` / `trig_book_added_reactivate`: drop `title` from the payload.
- **Accepted limitation** (document in the trigger comment): an edit changing only non-primary
  credits while title and primary author stay identical does not emit. Machine extraction is
  deterministic so this can't happen today; if a manual credit editor is ever built, add a
  `meta_rev` counter on `book` bumped by writers to force emission.

### SyncCommon (`SyncCommon/src/lib.rs`)

- `SyncOp::BookMeta { book_id, title, authors: Vec<Credit> }`,
  `Credit { name, sort, role }`; add `"book_meta"` to `SINGLETON_EVENT_TYPES`; wire
  `event_type/entity_id/payload/from_event_parts` (tolerate missing/empty `authors`).
- `BookAdded`: drop `title` (keep `added_at`).

### Client apply (`Backend/src/db.rs`)

- `SyncOp::BookMeta` arm: `first_book_insert` (stub identity if the book row hasn't arrived),
  then the same rebuild as extraction: resolve names (creating author rows with sort names),
  delete + reinsert `book_author`, set `book.author_id` to the first author-role credit, update
  title. `change_origin` is `remote`, so the triggers stay silent.
- `SyncOp::BookAdded` arm: switch from `insert_book` (overwrites title — last-applied-wins
  clobber) to `first_book_insert`.
- Plan 1's guard automatically covers `book_meta` (keys on `is_singleton()`).

### Queries (`library.sql`)

- `get_authors` / `get_author_books`: join through `book_author` with `role = 'author'` instead of
  `book.author_id` — co-authored books now count under every author. Books with no author-role
  credit appear under no author (same as today's empty-author behavior).
- Carousels / `get_books_in_dir`: unchanged (primary author via `book.author_id`). Full credit
  display on a book-detail view is future UI work, not this plan.

## Tests

1. Extraction-style `update_book_metadata` with two authors + translator → one `book_meta` event;
   payload array ordered, roles/sorts present; repeating identical values → no second event.
2. Remote `BookMeta` for an unknown book → book stubbed, author rows created by name with
   sort_name, `book_author` rebuilt, `book.author_id` = first author-role credit; a later
   `BookAdded` doesn't clobber the title (`first_book_insert`).
3. Re-apply with a shorter credit list → old rows gone (delete-and-rebuild, no leakage).
4. `get_authors` counts a co-authored book under both names; translator-only credit does not get
   an authors-page entry.
5. Unpushed local `book_meta` + older incoming remote → plan 1 guard skips.
6. Empty/whitespace author names skipped (matches `get_or_create_author_id`).
