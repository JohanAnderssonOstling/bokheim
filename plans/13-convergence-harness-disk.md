# Plan 13: Convergence test harness, tier 2 (disk + scanner)

Requires plan 12 (extends its harness). Benefits from plan 9 (scanner robustness) being done
first — tier 2 will otherwise rediscover plan 9's known panics/wedges immediately, which is
fine but noisy.

## What tier 2 adds

Tier 1 proves the *resolution rules* converge. Tier 2 proves the **three-replica
reconciliation** converges: disk ↔ local DB ↔ server. The scanner is the adversary here — it
faithfully reports any disk/DB drift as new local events, so any materialization bug
(un-renamed folder, marker-less folder, wrong-path creation, delete-order leftovers) becomes
either event ping-pong or divergence the harness can catch. This is the class that produced
the remote-delete resurrection and orphan-path-truncation bugs found in review; tier 1 cannot
see them.

## Architecture changes vs tier 1

1. **Device gains a tempdir** (`tempfile::TempDir`) as its library root. Real files: tiny
   synthetic "epubs" (content = the seed string; identity hashing doesn't care that they are
   not valid EPUBs as long as metadata extraction is bypassed — drive placements via the
   scan-path that hashes content, or stub extraction to return None).
2. **Local operations become filesystem operations** (this is the point):
   - add book = write a file into a folder;
   - remove book = delete the file;
   - move book = `fs::rename` the file;
   - create/rename/move/delete dir = the same on folders (markers ride along, as for a user
     using a file manager).
   DB-driven ops from tier 1 (reading position, app-driven dir delete via `db::delete_dir` +
   `remove_deleted_dir_artifacts`) stay, so both mutation styles mix.
3. **Scan step**: add `scan` to the scheduler's action set, running the real walk +
   reconciliation against the device's tempdir and DB. Two options, decide at implementation:
   reuse `LibraryScanner::scan_library` if it can be invoked synchronously without the
   thread/watcher machinery, else extract the walk+reconcile core into a callable function
   (preferred — it also makes plan 9's fixes testable in isolation). Include the vanish-sweep
   so the grace-window logic (`VANISH_GRACE_SCANS`) is exercised by repeated scans.
4. **Pull materialization**: the pull step now also runs the production materialization
   (`materialize_remote_dir_upserts/deletes`, `apply_local_removals` — currently methods on
   `SyncManager`; extract or replicate thinly so the harness calls the real logic).
   Blob transfer is **not** simulated; instead, treat "file exists because another device
   placed it" as: the receiving device's placement stays `is_downloaded = 0` with no file —
   which is a legitimate production state the invariants must accept — and optionally add a
   `download` scheduler action that copies the bytes and stamps the fingerprint, mimicking
   `transfer.rs`'s post-download bookkeeping.

## Extended assertions

At quiescence (all devices: sync + scan repeated to fixpoint — note scans can *create* events,
so fixpoint means a full round of scan+sync produces no new events on any device):

1. Tier 1's identical-DB-state assertion across devices.
2. **Per-device disk/DB consistency**: every live placement with `is_downloaded = 1` has its
   file at its resolved path; every `.epub` on disk corresponds to a live placement; every
   folder's `.biblos_uuid` matches a live (or stub-tolerated) dir row whose resolved path is
   that folder; no two folders share a marker uuid.
3. **No event ping-pong**: at fixpoint, one further scan on every device emits zero events
   (this is the assertion that catches un-materialized moves and marker bugs — they manifest
   as scans that never stop emitting).
4. Tombstoned dirs deleted on all devices have no folder remnants (modulo the documented
   non-empty-folder exception, which the generator should also produce: drop a stray non-epub
   file into a folder before remote-deleting it).

## Scope and budget

- Markedly slower than tier 1 (real fs + hashing): default `#[ignore]`, run explicitly
  (`cargo test --test convergence -- --ignored disk`), small default case count (~64 × ~100
  steps × 2–3 devices), deep mode for CI-nightly if that ever exists.
- Time control: the vanish grace window counts *scans*, not seconds, so no sleeping is needed —
  schedule repeated scan actions instead.
- Known-acceptable nondeterminism to tolerate in assertions: `last_scan` values, `local_hash`
  fingerprints (per-device), blob_local_ref contents — excluded from the cross-device dump.

## Acceptance

1. Passes on current code once the two review bugs are fixed (remote delete ordering, orphan
   path truncation) — and **fails with a shrunk case if either fix is reverted**, which is the
   proof the harness sees this bug class.
2. A device that only ever manipulates files (never the app API) still converges with a device
   that only uses app-level ops.
3. Documented runtime ≤ a few minutes in default mode.
