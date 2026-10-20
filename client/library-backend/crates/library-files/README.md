# Library files

Owns concrete access to book bytes and the physical library filesystem:

- `asset_store`: native files and browser OPFS, verified staging, leases,
  book and thumbnail access.
- `filesystem` (native): directory traversal, entry metadata, fingerprints,
  readers, and marker-file reads/writes.

The crate does not classify books or persist library state. The
database-backed scanner, watcher policy, and thumbnail job scheduler live in
`library-backend`; they call this crate for physical access.

## Host interfaces

On the browser, `hashing::HashService` provides incremental hashing sessions.
`library-backend` implements this through its CPU worker, including session
cleanup when an interrupted operation drops its hasher.

Library registration, lifecycle, relocation, account state, and UI events remain
backend responsibilities. This crate does not depend on `library-backend`.

Windows audiobook downloads repair filenames that Windows cannot create, using
synced chapter titles with numbered fallbacks. The audiobook folder keeps a local
`.bokheim-audiobook-paths.json` mapping from original archive names to physical
filenames. Playback resolves that mapping; scanning and uploads retain the original
names, order, ZIP header metadata, and sidecar bytes so the content hash stays unchanged. Placement
copies carry the mapping with the files. Keep the mapping when moving or backing
up a repaired audiobook folder.

All native downloads preserve the source ZIP metadata and member order when
needed, so audiobooks created on Windows retain their identity on Unix clients
as well. Unix downloads keep their original filenames.

## Validation

```sh
cargo check --release -p library-files
cargo test --release -p library-backend scanner:: --lib
cargo test --release -p library-backend thumbnail_jobs:: --lib
cargo check --release -p app --target wasm32-unknown-unknown --no-default-features
python3 scripts/checks/library-sql.py
python3 scripts/checks/backend-boundaries.py
```
