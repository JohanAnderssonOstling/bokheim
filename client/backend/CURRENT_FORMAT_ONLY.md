# Current-format storage

The client no longer upgrades older application data or decodes legacy metadata
representations. New databases use library schema 30, catalogue cache schema 6,
and persisted-value header version 13.

Removed:

- Metadata, identifier-source and edition-candidate migrations, including the
  repeated full-library metadata scans and alternative-edition rollback.
- SQLite column upgrades, retired-domain cleanup, old projection backfills and
  the unconditional folder ownership rebuild on database initialization.
- Scalar sync-cursor promotion and legacy browser asset adoption.
- Catalogue-cache schema upgrades and obsolete table cleanup.
- Legacy identifier encodings, missing-provenance fallbacks, collection aliases,
  and defaults used to accept incomplete persisted preferences/metadata/sessions.
- Bearer-only logout fallback and persisted-only FB2, CBZ, HTML and WebPub formats.

Older schemas are rejected before initialization writes. Existing user databases
and browser files are not deleted, reset or converted. The changed persisted
identifier layout and supported-format enum require the new storage versions;
this is an explicit format break with no migration path.

Current schema creation, durable outbox/restart recovery, incremental projection
maintenance and taxonomy refresh remain part of normal operation. Standards
support for reading supported publication files is independent of application
data compatibility. Stable identifier generation is also retained.

Regression coverage includes unsupported-schema rejection without mutation,
reopening without decoding or rewriting metadata, current identifier round trips
and rejection of missing provenance, and current supported publication formats.

Native validation: 438 tests passed across backend, SQLite, replica, model,
account, catalogue, preferences, asset transfer and storage codec crates; four
PostgreSQL-dependent tests were ignored. Full log:
`.test-tmp/remove-compat-final-tests.log`.

Final browser verification (2026-09-06): release bundle
`91ce0118fa9638894ffe4ee5` passed Firefox worker and sync contracts. Settings
responded in 3 ms and local book opening in 5 ms under held CPU/stalled HTTP.
Background phases completed. Recovery completed in 848 ms with the slowest
interactive probe at 169 ms. Old OPFS schema rejection preserved the schema
version and saved data. Shared downloads, account changes, exact outbox
acknowledgments, transient retry and worker restart passed.

The initial bundle lacked its required `backgroundCycle` WASM export. The
coordinator module was rebuilt, its exports checked, and the corrected bundle
was tested again. The release build now checks the required coordinator exports.
Test hooks also now wait for phase completion and buffer connections during
fixture WASM startup.

Logs: `.test-tmp/remove-compat-verified-workers.log` and
`.test-tmp/remove-compat-verified-sync.log`. These runs used the frozen release
scripts `.test-tmp/test-release-workers.mjs` and
`.test-tmp/test-release-sync.mjs`, reusing built Rust probe adapters and serving
the release's JavaScript rather than concurrently changing source modules.
The final preference regression rerun passed all 11 tests.

The release is served at http://127.0.0.1:4173/ and opened in a separate Firefox
profile titled “Bokheim — verified release”. Existing user profiles and data
were not reset. Full native executable artifacts were not rebuilt.
