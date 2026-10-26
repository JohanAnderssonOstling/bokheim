# Native application updates

Linux, Windows, and Android are policies over the same `NativeHost<P>` in
`src/host.rs`. Host construction, coordinator ownership, locking/reloading,
approval, discovery, download preparation, trial validation, healthy commits,
acknowledgement, recovery, and generation cleanup each have one implementation.
Public `Host`, `windows::WindowsHost`, and `android::AndroidHost` names are aliases
for the corresponding policy.

`src/migration.rs` owns database backup/candidate copies, taxonomy staging, the
preparation job and result, candidate validation, taxonomy pointer replacement,
application backup entries, and journal promotion. Desktop candidates must match
the approved release; Android candidates must match the installed APK and meet
the approved schema requirement. Platform code supplies package extraction and
the subprocess or Android service that runs the preparation job.

`src/databases.rs` owns database discovery and SQLite workspace sizing.
`shared/update-files` owns durable filesystem operations and
`shared/update-protocol` owns verified download staging. The GPUI adapter in
`apps/desktop-gpui/src/update_binding.rs` owns startup configuration, host
ownership until healthy startup, and binding to the shared browser UI worker.

Linux-specific startup below retains AppImage relaunch supervision. Windows
retains its external executable helper, and Android retains APK installation
and permission handling through the OS bridge.

This host connects the signed update coordinator to the GPUI desktop launcher.
It is enabled only in builds with a configured signing public key. Application
artifacts target `linux-x86_64-appimage` (or the corresponding architecture);
non-AppImage installations can receive compatible taxonomy-only offers.

## Lifecycle

1. The app checks for offers once at startup in a background worker, then waits
   for user actions. Update consent is persisted before downloading all approved
   artifacts. Unfinished downloads resume at startup. Hash-verified staging ends at Restart.
2. The next launch acquires desktop ownership before opening libraries. It checks
   staging again, creates SQLite backups including committed WAL pages, and makes
   separate candidate copies of every device-local library database. The registry
   is backed up too. Book/content directories need not be mounted.
3. The candidate executable runs `--bokheim-prepare-update` against those isolated
   copies. It validates the taxonomy's signed release/format, compiles its client
   routes, migrates schemas, refreshes assignments, and checks SQLite integrity.
   Failure leaves active application/data untouched.
4. A durable journal is flushed before promoting databases, the active taxonomy
   pointer and AppImage. Each replacement retains its prior generation.
5. A child launch with the trial ID selects the new snapshot before opening
   libraries. Background executor tasks are gated until backend/window startup
   succeeds. The child commits the journal, acknowledges installed versions and
   releases background work. The parent observes this acknowledgement.
6. A failed child launch or a trial taking over two minutes is stopped; the old
   launcher restores the journal and reopens the previous generation. Interrupted
   promotion/trial also recovers on the next ordinary launch. Failed releases are
   quarantined; a corrected artifact set needs fresh consent.
7. A committed journal is never rolled back. The next launch finishes bookkeeping
   and removes obsolete staging/backups, retaining the active taxonomy image.

Candidate migration preparation has a 30-minute limit before it is stopped and
recovered. Discovery errors wait for the next launch. Download errors wait for an
explicit Retry or the next launch; no background polling runs. Storage
and installation-permission blockers are exposed through the shared Settings row.
Space preflight uses signed artifact lengths, actual SQLite page counts/sizes,
backup/candidate/journal workspace, existing AppImage size and per-volume free
space. Workspace is an estimate; SQLite/IO errors still roll back safely if a
migration grows beyond that budget or free space changes concurrently.

## Taxonomy and assignments

The snapshot is immutable for a process lifetime. Both the matcher and attached
SQLite browse tree use that snapshot. A per-library content/matcher revision
records successful projection. Refresh visits all stored metadata, including
previously unmatched books, reconciles stale assignments and rebuilds navigation
in one transaction. Canonical metadata and the sync outbox are unchanged.
Existing manual subjects retain surviving stable concept IDs across renamed paths.
New/unavailable library databases use the selected generation before their first
operation; native library databases are device-local even when book folders are
unmounted.

## Verification and rollout

```bash
cargo test --release -p update-client --features native-state
cargo test --release -p linux-update-host
cargo test --release -p library-database --lib
RUSTC_BOOTSTRAP=1 cargo check --release -p desktop-gpui
```

The host integration test signs a fixture manifest, runs the actual migration
entry point in a separate process, simulates an interrupted trial, restores the
old application/data, then approves and commits a corrected release. It checks
new/removed assignments, manual-subject identity, canonical metadata and outbox
preservation. The fixture package is an executable test wrapper, not a packaged
GPUI AppImage. GitHub's Linux job runs this test; the AppImage packaging job also
executes `--bokheim-update-info` to verify the package exposes its actual versions.

Packaged AppImage, native Windows GUI, and physical Android installation trials
remain rollout checks. The shared Rust host does not replace each platform's
application ownership and OS installation requirements.

## Windows host

The `windows::WindowsHost` adapter stages a complete signed ZIP, runs candidate
schema/taxonomy preparation in a separate executable, and journals application,
registry, library and taxonomy replacements. `prepare_or_recover_in_process`
must be called from the copied helper while holding exclusive desktop ownership.
It waits for the migration child to exit before reading its output. The shared
supervisor terminates and reaps timed-out migration processes.

PDFium is statically linked on Windows, so the helper and candidate each need
only their executable. The Windows MSVC host tests pass under Wine, including
rollback and cleanup after a file lock is released. Desktop ownership handoff, helper startup and Settings wiring now use this
host. The Windows process fixture exercises real executable success and rollback
under Wine; native Windows GUI trials remain required before production rollout.
