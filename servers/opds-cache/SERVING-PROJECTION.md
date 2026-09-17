# Incremental subject serving

The authoritative catalog remains the crawl/enrichment database. Subject browse
navigation, facets, search, ordered entry IDs and their payloads are served from
an independent WAL file beside it: `<catalog-path>.serving-v3-t<matcher-version>.db`.
Neither deployments nor projection updates replace the authoritative catalog or
the metadata-server snapshot.

The background projector runs independently of administrator-controlled crawl
and enrichment workers. When idle it polls once per second; when busy it drains
bounded batches and yields between them. A batch handles at most 256 dirty book
identities plus 256 identities from one changed source. Gutenberg variants are
reconciled as a work group, including promotion after a preferred edition is
removed.

## Transactions and recovery

1. Triggers coalesce active-book changes into a journal. Inactive crawl
   staging does not dirty individual books; activation records one source marker.
2. The projector briefly reserves the authoritative SQLite writer, pins a second
   connection's WAL read snapshot, and advances the journal observation epoch.
   Subsequent writes cannot be lost by acknowledging the observed epoch.
3. All payload/search/facet preparation and source-key expansion happen using
   that snapshot **after** releasing the authoritative writer reservation. The
   service's global writer gate is not held by the projector.
4. Prepared rows and source cursors commit together in the sidecar. Source
   visibility changes converge in bounded batches, not an all-source transaction.
   Each browse response uses one consistent sidecar read transaction.
5. Exact journal acknowledgements commit in a short authoritative transaction.
   A crash between commits causes safe replay. The observation fence and sidecar
   use FULL synchronization before acknowledgement. A process-local mutex and
   file lock serialize projectors using the same sidecar.

Ordinary catalog revision changes never clear the serving tables or restart a
source cursor. A newer activation of that source restarts only its reconciliation.
The cursor walks both active catalog identities and previous serving identities,
so deletion does not require retaining book rows in the authority.

Initial creation and taxonomy/schema versions use a new sidecar. Readers use the
authoritative fallback until initial source reconciliation is ready, then switch
to the sidecar. Existing sidecars are left on disk; removing obsolete versions is
an explicit maintenance action. Recreating a sidecar bootstraps from all sources,
even if their journal entries were previously acknowledged.

## Client payloads

Public epochs identify a sidecar incarnation, not each update. Existing payload
entry IDs already include the payload hash in their authoritative identity.
Thus ordinary updates preserve client caches, and an older index request can
resolve the exact immutable cards it referenced. Retired cards are kept for one
hour, then collected in batches of 256. A recreated sidecar gets a new incarnation
and rejects epochs from the previous file. The internal projection revision and
source cursors still advance transactionally with changes.

## Diagnostics

Debug log `catalogue serving projection batch` reports book count,
`fence_ms` (writer reservation through durable fence commit), `preparation_ms`,
and `commit_ms` (sidecar transaction). These measure separate resources; the
preparation time is not catalog writer-lock duration.

Verification: `cargo test --release -p catalog-server --lib` includes coalescing,
concurrent writer/snapshot isolation, failed commits and acknowledgement replay,
source deletion/restart, immutable card versions and expiration, sidecar recovery,
Gutenberg promotion, and existing browse/search/facet behavior.
