# Application and library updates

Investigation date: 2026-09-28. Proposed design based on the current working tree; no runtime changes have been implemented or tested.

## Agreed update policy

Application code and schema migrations form one application update. Taxonomy content has an independent release version, but the user sees one combined update action. A compatible update plan pins the selected application, schema, taxonomy, and assignment algorithm versions; it can also contain only a taxonomy update.

Settings has one conditional section at the top, containing one row:

| State | Message | Action |
| --- | --- | --- |
| Available | Update available | Update |
| Preparing | Preparing update… | Disabled |
| Prepared | Update ready | Restart |
| Complete | Section absent | None |

Discovery never authorizes installation. Clicking Update durably authorizes preparation, automatic recovery, and activation on a subsequent launch. Download, verify, and stage immutable artifacts in the background. Do not replace the active taxonomy, recalculate active assignments, migrate the active schema, or restart automatically during the current application run. Preparation ends at Ready; it never starts activation on a timer.

Restart is an optional shortcut. Otherwise, close and reopen normally. At the next launch, finish the already-approved plan before exposing library handles or starting library workers. Validate the staged plan and acquire exclusive access, apply required schema migrations, recalculate assignments using current canonical evidence, rebuild browsing, and commit the completed generation. Show startup progress while this runs. Activation is a coordinated sequence, not a single atomic transaction across executable replacement and every library database.

Preparation may continue after an interrupted download on a later session, but reaching Ready in that session still waits for another launch. A library unavailable during activation completes the authorized upgrade before its next use. Its persisted pending state must survive restarts.

The backend owns transient retries, checkpoints, and recovery. Expose action only for a blocker requiring user involvement, such as installation permission or insufficient storage. Calculate additional storage required per volume from the remaining download, staging, backup and migration workspace, plus a defined safety reserve, minus usable free space; account for already allocated artifacts without double counting. Recheck automatically and resume when the requirement is met. Keep detailed failures in diagnostics, not retry dialogs. Quarantine repeatedly failing releases instead of retrying them endlessly. Recovery must respect executable/schema compatibility; do not revert to an incompatible binary.

The mockup simulates this policy; it does not download, install, or migrate real data.

### Android delivery

Android uses direct APK distribution, with no app store. Download and verify the APK in persistent app-private update staging storage. For a plan containing an APK, the ready row is `Update ready · Install`. Only an explicit press hands the APK to Android's system installer, which owns application replacement and any required installation approval or unknown-source permission. Merely restarting the old application cannot install an APK. If installation is cancelled, retain the prepared plan for a later Install action.

On the next launch, verify that the required application version is installed before applying its schema or taxonomy changes. Taxonomy-only updates retain `Update ready · Restart` and activate on the next launch. Neither flow interrupts the running session automatically.

Proposed persistent storage is `<app-private-data>/updates/state.sqlite` for authorization and orchestration, `staging/<update-id>/` for downloads and verified artifacts, and `recovery/<update-id>/` for recovery files. Per-library migration completion remains inside the library database and commits with its changes. This storage layout and installer integration remain unimplemented.

## Provider-independent discovery implementation

`shared/update-client` now owns the v1 signed manifest contract, Ed25519 verification,
platform/version selection, taxonomy compatibility selection, and optional native
HTTPS discovery/download adapters. The proposed default endpoint is
`https://updates.bokheim.se/v1/stable`; hosts can configure it. Download URLs,
lengths, and SHA-256 hashes come from authenticated metadata. GitHub is an optional
publishing/storage implementation detail, not a client discovery dependency.

The parked Linux and Windows updaters now call this shared discovery code instead
of GitHub's API and filename conventions. A release-signing public key must be
bundled explicitly; no production key or hosted endpoint has been provisioned.
See [the protocol and publishing instructions](../shared/update-client/README.md).
Durable polling state, activation coordination, Android installer integration, and
the live settings UI remain subsequent implementation work.

## Existing foundations and gaps

- `client/library-backend/crates/library-database/src/lib.rs`: `initialize_library` upgrades library schema versions 59 through 75 inside a transaction, rejects unsupported nonzero versions, and initializes new databases separately. It then ensures the subject browse cache. Preserve these existing migrations and the unsupported-version guard.
- `client/library-backend/crates/library-database/src/browse/subjects/cache.rs`: cache freshness includes `BUNDLED_UNIFIED_TAXONOMY_REVISION_ID`. Rebuilding walks existing `book_unified_concept` assignments; it does not recalculate those assignments from source evidence. Thus a changed selector can require work beyond the existing cache rebuild.
- The library connection attaches `DEFAULT_UNIFIED_TAXONOMY_SQLITE` as `curated`. The matcher also supports a replaceable process-wide runtime. A future downloadable taxonomy must keep the SQL attachment and matcher on the same content revision.
- `client/library-backend/crates/library-database/src/sync/apply.rs`: classification preparation and reconciliation already cover source codes, assignments, concept membership, and evidence. `project_prepared_book` also updates canonical metadata; a maintenance operation needs a narrower entry point for derived classifications.
- `shared/subject-projection/src/lib.rs`: matcher versions already exist, including `UNIFIED_MATCHER_VERSION`. Existing assignment rows carry versions, but that alone cannot record that a book was processed and had zero matches.
- `shared/subject-projection/src/runtime.rs`: immutable matcher handles exist alongside the global runtime. Extend the projection API to accept a pinned handle so an update cannot change the matcher midway through a job.
- `servers/taxonomy/README.md` describes release discovery, downloads, and client hydration. The server contract and shared overlay utilities exist, but searches of the current client Rust code did not find the documented release-check/overlay-install integration. Treat the README's client behavior as unverified rather than an available update coordinator.
- `apps/desktop-gpui/src/updater.rs` downloads and replaces an AppImage with checksum verification and a previous-binary backup. This is a delivery mechanism; it does not establish database rollback compatibility.
- The sync server already owns a separate PostgreSQL migration ledger in `servers/sync/crates/server-postgres/src/database.rs`. Keep server migration ownership there.

## Ownership and versioning

Place scheduling and progress in `client/library-backend`, with durable library state and transactions in `library-database`. The application opens libraries through this boundary on every platform. Platform installers deliver executables. Startup activates an explicitly authorized, prepared update before library access. Libraries that were closed or on disconnected storage complete the already-authorized upgrade before use; opening a library never selects a new taxonomy release.

Track these versions independently:

| State | Meaning | Update trigger |
| --- | --- | --- |
| Database schema version | Tables, columns, constraints, triggers | Application code requires a newer schema |
| Taxonomy format version | Snapshot representation understood by the client | Explicit format compatibility check |
| Taxonomy release and content digest | Published content and exact installed snapshot | Bundled or downloaded content changes |
| Assignment algorithm version | Code interpreting subject evidence | Matcher/projection logic changes |
| Browse projection version | Navigation, compaction, counts | Browse algorithm changes |

An assignment generation identifies at least `(taxonomy content digest, assignment algorithm version)`. A browse generation additionally includes its own algorithm version. Sparse overlays need their own exact content identity: the server release alone does not identify which concepts are installed.

Use compiled, ordered migration definitions with stable IDs and checksums. Keep `PRAGMA user_version` for the schema gate and add a migration ledger for auditability. Baseline existing databases accurately; do not fabricate records claiming historical migrations were individually observed. Apply each migration's changes and success record transactionally. Validate the version again under the writer lock. Never execute remotely supplied SQL.

## Initial implementation

Start with taxonomy shipped in the application and a blocking, transactional assignment upgrade at the launch that activates an approved update. This fits the current attachment model and establishes correctness before adding live taxonomy switching.

1. Acquire exclusive update ownership for this library before exposing handles or starting imports, enrichment, and sync writes. Ownership must cover separate processes and browser tabs, not just one Rust mutex.
2. Validate schema compatibility and run required schema migrations. Use a consistent database backup for migrations that need a restore path; account for SQLite WAL through a supported backup mechanism.
3. Compare the target assignment generation with the library's completed generation.
4. If different, read canonical subject evidence, recalculate classifications with the pinned bundled taxonomy, and reconcile assignments plus evidence. Include books with no previous assignments and books that now have no matches.
5. Rebuild the dependent subject browse projection and write the completed generation in the same transaction as the assignment changes.
6. Commit, then start normal library workers and notify the UI.

This approach may block opening a large library. Measure representative libraries before setting an acceptable threshold. It is a useful first implementation because failures roll back and completion has a precise meaning. Matching must remain deterministic and use local evidence; a taxonomy correction must not require rescanning book files or repeating external metadata searches.

Use a dedicated `reproject_subjects` operation that does not update canonical metadata or generate outbound sync mutations. Reuse the classification reconciliation machinery after extracting the necessary subject-specific preparation and writes. Genuine corrections to canonical metadata are a separate migration category and need explicit sync semantics so old devices cannot silently reverse the correction.

## Deferred: resumable updates for large libraries

The initial design performs assignment work at the activation launch. If measurements later justify background staging, introduce durable jobs; activation must still wait for the next launch:

```text
update_job: id, kind, target_generation, status, cursor, last_error
subject_generation: id, taxonomy_digest, algorithm_version, status
subject_projection_book: generation, book_id, input_digest, status
library_projection_state: active_generation, desired_generation
```

Keep the job inside the library database so progress commits with its output. A failed batch retries safely. The per-book completion record includes zero-match results; assignment rows alone cannot provide completeness. Store errors after the failed transaction rolls back.

Build assignments, evidence, and browse state in a staging generation. Continue serving the complete previous generation. Checkpoint after bounded batches using stable book IDs. A cursor is only a scanning aid: track inserts, deletions, and metadata changes during the job with a durable dirty set. Compare input digests before accepting prepared results, and retry stale work.

At the next launch, before starting normal writers, drain/reconcile the final dirty set, verify completeness and concept references, and switch the active generation in one library transaction. Readers pin a generation for each request. Taxonomy attachments and matcher handles must follow that pinned generation. Every connection needs an explicit refresh/reopen protocol; changing a process-global matcher alone is insufficient.

Retain the previous immutable taxonomy while any library generation references it. Each library activates independently; avoid a distributed transaction across all libraries. On restart, unfinished staging work resumes, and the active generation remains usable. A newly available target can supersede an unfinished job without replacing the active state.

## Downloadable taxonomy releases

Once generation handling works, add a shared taxonomy manager that stages an immutable snapshot, validates its format, digest, structural integrity, and client compatibility, and then schedules affected libraries. Discovery metadata should identify a release-specific immutable download; verify that downloaded bytes match the announced release. Retain the last usable snapshot on network or validation failure.

Prefer complete snapshots initially, subject to measuring download and memory costs. Sparse hydration requires tracking coverage as part of generation readiness: a missing response is not a confirmed negative match. Requests spanning a server deployment must not mix releases. Scope both positive and negative resolution caches to the target release. Do not discard the working overlay before its replacement can support the active libraries.

Keep concept IDs stable across rename and reparent operations. Route IDs and labels are presentation state. For automatic assignments, re-evaluate original evidence after merges, deletions, or splits. If user-selected subjects are introduced, store those choices separately with provenance; explicit redirects can handle unambiguous merges, while splits must preserve intent and may require user review.

## Recovery, compatibility, and validation

Reject databases newer than the running binary understands. A previous executable is not a sufficient rollback plan after a schema upgrade. Prefer forward repair, or restore a consistent backup with explicit handling of newer local writes. Downloadable taxonomy releases must declare supported formats and matcher requirements; unavailable compatible data leaves the existing generation active.

Local derived assignments should not sync. Devices with different taxonomy releases can temporarily show different classification while retaining the same canonical evidence. Schema changes to synchronized canonical data also require a wire compatibility plan and server rollout ordering.

Before implementation is considered complete, validate:

- Every supported historical schema can upgrade; newer and unsupported schemas fail without mutation.
- Selector changes reassign existing books; removed matches delete old assignments; newly matching books gain assignments even if previously unassigned.
- Rename/reparent changes preserve concept identity and refresh paths; matcher-only and browse-only version changes trigger the correct work.
- Re-running a completed update is a no-op and derived maintenance leaves canonical metadata and the sync outbox unchanged.
- Failure before commit, interrupted staging, disk exhaustion, and incompatible taxonomy format preserve a usable prior state.
- Concurrent import, sync edits, deletion, multiple libraries, and competing processes/tabs cannot activate incomplete or stale projections.
- Native and browser storage execute the same logical transitions; measure large-library latency and storage overhead using optimized release builds.

Suggested sequence: migration ledger and compatibility gate; transactional bundled assignment refresh; progress reporting; resumable generations if measurements justify them; independent taxonomy downloads.

## Implementation progress: durable lifecycle and settings row

The shared update client now includes a persisted coordinator with compare-and-swap
state writes, explicit approval, pinned signed plans, retry deadlines, verified
native staging, next-launch activation gates, and a platform-neutral UI state.
Native state is stored in SQLite with a separate process lock. Android plans wait
for the required APK version before allowing data activation. Tests cover crashes,
stale writers, damaged staging, metadata expiry, and actual storage deficits.

The current GPUI settings page has the agreed optional single row through
`AppServices::with_updates`. It is hidden by default; production launchers do not
supply the binding until installation/startup activation is implemented. This is a
backend and UI integration boundary, not a claim that automatic updates are live.
See `shared/update-client/README.md` for the precise host integration sequence and
remaining work. The HTML mockup remains a simulation.

## Implementation progress: server publication

`servers/updates` now supplies an operator/CI publisher for signed bundles, with
immutable artifact verification, publication locking, monotonic sequences,
archived envelopes, and atomic feed promotion after artifact installation. The
shared Caddy configuration serves the update feed and immutable application and
taxonomy downloads. An SSH deployment script builds the publisher in release
mode and runs it as an unprivileged publishing account.

See `servers/updates/README.md` for bundle layout, signing and deployment steps,
withdrawals, manifest renewal, and existing backend deployment ordering. Existing
GitHub workflows remain draft artifact producers; they are not connected to live
publication. Production trust keys, DNS, account provisioning, Caddy validation on
the server, Android signing and client activation are still pending. Nothing has
been published to the live server by this implementation.

## Implementation progress: Linux lifecycle

The Linux launcher now binds the updater when a signing key is bundled. Candidate
preparation migrates isolated SQLite copies and reprojects all stored subject
evidence against the approved immutable taxonomy. A durable promotion journal
restores prior databases and the AppImage if the startup trial fails. Background
sync/import tasks wait for the trial's commit. Desktop ownership now spans the
entire GUI lifetime. See `client/update-host/README.md` for the implemented flow,
release tests, storage calculation and remaining packaged production smoke check.
