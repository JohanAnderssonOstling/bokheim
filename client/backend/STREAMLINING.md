# Backend correctness and platform streamlining

Scope: the nine items accepted in the conversation. This is a progress record,
not a claim that all nine are complete.

1. Shared owned-task cancellation: implemented and tested with the native
   executor and actual browser worker scheduler.
2. Shared staged asset API: imports use staged readers and common inspection;
   downloads share streaming hash/capacity checks. Native file adoption remains
   an I/O adapter. Browser readers stream OPFS generations without copying books.
3. Publication/replacement: native publication uses one rename, synced file
   contents, and parent-directory sync where supported by the adapter. Browser
   publication commits a generation through the SQLite index. Browser worker
   restart tests cover interrupted replacement. Power-loss durability on every
   native filesystem/OS remains unverified; no bespoke Windows API was added.
4. Coordinated cleanup: committed-asset collection checks SQLite references
   under a transaction and skips active leases; native leases span processes.
   Browser generation cleanup runs at exclusive bootstrap. Native abandoned
   staging cleanup still needs stronger cross-process coordination.
5. Availability versus retention: still requires a schema migration, derived
   folder state, scanner/sync changes, and eviction tests. Current per-placement
   downloaded flags still conflate physical bytes with retention intent.
6. Shared import validation/inspection: implemented for EPUB/PDF/MOBI/M4B using
   readers; metadata, placement, scan sequence, and cover work commit together.
7. Thumbnail recovery: durable device-local work survives failed writes and
   cancellation after import commit. Local maintenance precedes the account
   gate; database wakes collect pending cover work even while signed out.
   Regression tests cover failed publication, reopen recovery, and stale cover
   generations. Browser contracts additionally exercise real library recovery.
8. Shared transfer rules: upload verification and download validation are shared;
   download metadata revalidates placement after publication waits. Browser HTTP
   uploads still buffer the body because of the current transport adapter.
9. Contract tests: native regressions and a real Chromium worker runner cover
   partial writes, wrong hashes, cancellation, generation pinning, worker restart,
   staged imports, and publications above the former 128 MiB browser limit.
   Full native filesystem/OS durability and availability migration coverage
   remain outstanding.

Preserve concurrent worktree changes. Do not mark the broader task complete
until the remaining implementation and verification above are resolved.
