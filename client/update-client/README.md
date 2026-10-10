# Bokheim client updater

Client discovery, compatible offer selection, preparation, and activation.
The signed manifest format and signature/artifact verification live in
[`shared/update-protocol`](../../shared/update-protocol), which is also used by the publisher.
Native HTTP and durable state are optional features; browser hosts supply fetched bytes.

## Trust and configuration

The workspace Cargo configuration supplies the production public key. Builds
outside that configuration must set `BOKHEIM_UPDATE_PUBLIC_KEY_HEX` to the trusted 32-byte
Ed25519 public key encoded as 64 hex characters. `BOKHEIM_UPDATE_KEY_ID` defaults to
`release-1`. The private signing key is never bundled; this repository configures public trust for its release identity. Verification fails
closed when the public key is absent. Never embed or commit the private key.

`BOKHEIM_UPDATE_ENDPOINT` overrides the endpoint at build time and, in the native
desktop adapter, at runtime. Other hosts can pass `DiscoveryConfig` explicitly.
Runtime endpoint changes do not change the trusted signing keys. Keys must be
provisioned through trusted application releases; for rotation, distribute clients
trusting old and new keys before switching signatures. The configuration API accepts
multiple keys; the current bundled configuration provides one key.

Endpoints and artifact URLs must use HTTPS without credentials or fragments.
The native client follows at most five redirects and rejects HTTP downgrades.
Download hostnames and filenames are not part of release selection.

## Host integration

The coordinator persists the highest verified sequence, authenticated envelope,
and approved plan, scoped to the configured channel and trust keys. It rejects
equal-sequence content changes, checks discovery expiration, and pins an approved
plan independently of later publications.

Hosts supply scheduling, staging directories, installation, and compiled library
migrations. The native-state feature provides durable storage and verified staging;
[`client/update-host`](../update-host) integrates it with native startup and platform
installation. Browser hosts can use the coordinator with their own transport and store.

## Validation

```sh
cargo test --release -p update-client --features native-state
cargo check --release -p update-client --target wasm32-unknown-unknown
```

## Durable coordinator (implemented)

`coordinator::Coordinator` separates discovery, explicit approval, preparation,
readiness, and startup activation. It persists before publishing a transition.
`Store` uses compare-and-swap revisions so stale processes cannot overwrite newer
approvals. Signed envelopes, the highest publication sequence, the approved plan,
completed artifacts and diagnostic errors survive a restart.

The `native-state` feature adds `native_state::SqliteStore` under the private
`updates/` directory, a process lock, startup discovery, and verified file staging.
Completed downloads are reused after interruption; incomplete downloads restart.
The host must provide actual extra installation/backup/workspace estimates and a
safety reserve for the staging volume. Other volumes need their own preflight.
File hashes are rechecked before activation, not merely trusted from a database
flag. A corrupt staged file returns to preparation and waits for another launch.

A host integration follows this order:

1. Open the coordinator with a unique ID for the current process launch and the
   actual installed application, schema, and taxonomy versions.
2. Under `UpdateLock`, if `activation_due()`, validate `staged_paths()` and call
   `begin_activation()` **before opening libraries**. Resume interrupted activation
   using the platform's installation and transactional migration checkpoints.
   Android APK plans cannot begin data activation until the new APK version runs.
3. The host applies and validates its actual installation/migrations, then calls
   `activated(actual_versions)`. Do not mark completion merely because files were
   downloaded. On failure recover the prior usable state; only then quarantine a
   defective release. A corrected artifact plan becomes an offer needing approval.
4. In a background worker call `check_for_updates` once at startup. The worker then
   waits for user actions without polling. Network work never runs inside a library
   transaction or on the UI thread.
5. On explicit Update call `approve`, then `prepare` all approved artifacts. Resume
   unfinished approved downloads at startup; failures wait for Retry or another
   launch. A Ready state never activates in the same launch, including when preparation resumed
   after a previous crash. The Restart action belongs to the platform host; Android
   uses Install when the plan contains an APK.
6. Forward `view()` changes to the settings UI. Native errors that escape staging
   must be diagnosed and scheduled by the host; do not show generic error dialogs.

`browser-ui::UpdateControls` and `AppServices::with_updates` now bind the shared
view/action model to one conditional row above Account in Settings. The state is
shared across windows; changes invalidate the settings page. Space deficits are
rounded up by at most one MB. Technical errors never enter the view model.

**Linux host wiring is implemented** in `client/update-host` and
`apps/desktop-gpui/src/linux_updates.rs`. Signing-key-configured Linux builds bind
Settings, check at startup and stage in the background, prepare database copies with the candidate
executable, and use journaled next-launch activation with automatic rollback.
See `client/update-host/README.md` for verification and the remaining production
rollout checks. The old desktop updater modules remain unused.

Android Settings/startup wiring is implemented with a private migration service
process and explicit OS installer/permission actions. Device validation remains
pending; see `apps/desktop-gpui/android/README.md`.

The shared activation journal and staging now support Windows file operations.
`native_files` flushes writable handles and uses write-through file replacement;
Windows CI exercises recovery and a destination held open by another process.
Windows offers no unprivileged POSIX directory-fsync equivalent. This layer does
not yet implement Windows application supervision or the installer/Settings adapter.
The API choices follow Microsoft's
[MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
and [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)
contracts.

Windows application activation, browser storage/activation, production signing
configuration and endpoint publication are still pending.

`supervisor::wait_for_trial` watches the journal's matching healthy-startup commit
while observing the spawned process. It returns a stopped outcome only after the
child has exited/reaped, including timeout and receipt-read failures. Callers must
then consult the journal again: a commit racing a timeout must never be rolled
back. Failure to prove the process stopped is an error, not permission to restore
its files. Linux uses this shared supervisor; Windows application wiring remains
pending. The supervisor is tested with real child processes on Linux and Wine.
