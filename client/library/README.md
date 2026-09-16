# Client library features

`client-library` owns headless library behavior and its SQLite persistence on
native and browser targets. It replaces the former `library-sqlite` package.
Call the owning feature directly, for example
`client_library::reading::get_stored_reading_position` or
`client_library::assets::book_upload_intent`.

## Ownership

| Module | Responsibility |
| --- | --- |
| `metadata`, `authors`, `descriptions` | Publication metadata, contributors, identifiers, and descriptions |
| `browse`, `books` | Library queries and book-card projections |
| `reading`, `marks`, `toc` | Positions, annotations, and navigation |
| `audiobooks`, `audible_jobs`, `pdf_metadata` | Persisted format-specific inspection results |
| `external_metadata`, `prepared_enrichment`, `subject_pipeline` | Enrichment selection, results, and durable work |
| `assets`, `thumbnail_jobs` | Asset presence, requests, upload intents, and thumbnail work |
| `placements`, `file_work`, `scanner` | Directory placement and durable filesystem work |
| `sync_state`, `remote_reducer` | Replica state, outbox, recovery, and incoming mutations |
| `schema` | Connection setup and ordered initialization/migration of the shared database |

SQL belongs in the owning module's `sql/` directory. Rust binds parameters,
decodes rows, manages transactions, and coordinates behavior. Complete queries,
DDL, triggers, migrations, and test fixtures live in `.sql` files. Do not build
queries with `format!` or embed SQL strings in Rust. Use bound parameters (including
JSON arrays for variable-length sets) or select a complete SQL variant.

Named SQL bundles use `include_sql!` in each feature's `queries.rs`. Standalone
statements use `include_str!`. Generated method traits are private to the crate;
the public API consists of feature functions, not a global database facade.

All features still share one database. A caller can pass an existing transaction
across features so metadata, projections, and synchronization work commit
atomically. Schema version and migration order remain coordinated by `schema`.
The PDF outbox migration reads and reinstalls existing trigger definitions to
preserve installed triggers; its schema transformation is also in a SQL file.

Backend owns platform handles, library lifecycle, task scheduling, and application
capabilities. Its `library/reading.rs`, `browse.rs`, and `annotations.rs` adapt
feature results for application consumers. Platform-only stores such as the
remote publication cache keep their SQL next to that platform module.

## Validation

```sh
python3 scripts/checks/library-sql.py
cargo test --release -p client-library --lib
cargo test --release -p bokheim-backend --lib
cargo check --release -p bokheim-backend --no-default-features --target wasm32-unknown-unknown
```

The source check covers this crate and backend, including Rust test fixtures and
examples. It checks for embedded SQL statements, SQL formatting, and missing
included SQL files. Compilation and behavior tests remain necessary for query
semantics and transaction invariants.

Features import persistence helpers and SQL traits from their owning modules.
The crate root is not an internal prelude; new feature code must not import it
with `use crate::*` or reach through root aliases for persistence functions.
