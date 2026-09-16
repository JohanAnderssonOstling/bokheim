# Durable book uploads

## Contract

Only import/scan observations of new or changed local publication bytes create
durable upload intent. Receiving metadata, downloading books, unchanged rescans,
and moving/copying placements do not create new upload work.

- SQLite stores observed versions, each placement's current checksum/provenance,
  and one latest pending upload per book. Jobs contain identity, byte checksum,
  size and a monotonic ID; acknowledgment deletes only that exact generation.
- Download publication records its verified checksum as a rescan baseline. An
  import that keeps an existing destination does not queue the discarded bytes.
  Per-placement baselines distinguish a genuine edit/reversion from a copy.
- Existing `asset_work` remains the scheduler input. Upload eligibility depends on
  durable intent, never on absence of `remote_asset`. Metadata/placement sync,
  thumbnails and requested downloads retain independent paths.
- The existing transfer worker negotiates up to 32 books per batch using captured
  checksums, before reading files. Owned/claimed versions complete their jobs;
  missing versions upload independently, with ordinary retry/authentication and
  progress reporting. Asset planning does not wait for this network request.
- Missing uploads open and verify the exact queued revision, including when two
  placements contain different bytes under one permanent identity. File copies
  with a known baseline also select and verify that revision. A scanner keeps
  one stable catalogued filename when duplicate identities share a folder.
- Account rebind retains latest pending bytes, rotates tokens, and requeues live
  local-origin sources for the new account. Download-only sources stay excluded.
  Stale account/version responses cannot complete or suppress replacement work.
- Server negotiation separates permanent book identity from immutable checksum.
  It authorizes the library and verifies stored identity outside the writer.
  Global reuse charges the account and binds its revision atomically, without
  uploading bytes. Download authorization remains mandatory; immutability does
  not make files publicly downloadable.
- Per-entry identity/size/quota rejection does not block valid peers. An already
  charged blob whose physical file is missing can be repaired without charging
  the account twice, even at full quota.

## Compatibility / rollout

The manifest wire contract now requires `checksum`; update client and server
together. No old-wire compatibility default is provided.

New tables are device-local and added without deleting existing catalog data.
No upload jobs or historical source provenance are inferred from old
`is_downloaded` / server-availability flags. New local byte observations establish
provenance; metadata receipt alone never enrolls publications.

No deployed application, live library, or production server was modified.
Verification uses optimized release builds and disposable test services.

## Verification

Completed release checks:

- SQLite: 221 passed, 16 manual benchmarks ignored.
- Upload provenance: 3 passed, including actual metadata move/copy followed by
  account rebind, restart/rollback, old acknowledgments, and edit/reversion.
- Backend sync: 122 passed, 5 ignored; covers metadata independence, batch
  negotiation without local file reads, exact-revision reader, and no-intent
  download/upload behavior.
- Native platform: 66 passed, including duplicate M4B scanning and file recovery.
- Library operations/imports: 52 passed.
- Server PostgreSQL: latest 33 passed with ignored integration tests enabled,
  including global revision reuse, identity rejection with valid peers,
  authorization/quota and repair at full quota.
- Shared sync contract, transport and asset-transfer package tests passed.
- Browser backend: release wasm check with `--no-default-features` passed.
- HTTP E2E script: 4 backend/client-server tests passed, plus account/database
  integration and public registration. The release server binary timestamp is
  newer than the final server/manifest source edits.

Final reruns completed:

- Session 23855: 122 backend `sync::` release tests passed, including the added
  delayed manifest response racing a newer local version. Four HTTP scenarios
  ran separately; one manual benchmark remains intentionally ignored.
- Session 79730: final browser release check passed after the last manifest enum edit.

## Reproduction

Native/client tests use `CARGO_TARGET_DIR=/tmp/browse-release-target`,
`CARGO_BUILD_JOBS=2`, and a disk-backed TMPDIR under client/target.
Always use `cargo test --release` / `cargo check --release`.

Server workspace: servers/sync, `SQLX_OFFLINE=true`.
`backend/tests/run_sync_e2e.sh` builds/tests/launches release only and owns
cleanup of its temporary database/server. Completed session: 11062.

Standalone PostgreSQL test cluster at /tmp/bokheim-upload-pg.mn4ssL/data
(port 55439, user upload_test, database postgres) was stopped cleanly after
session 47216. Its disposable files remain; no production data was used.
