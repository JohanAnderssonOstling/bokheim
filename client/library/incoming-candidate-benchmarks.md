# Incoming-sync candidates — 2026-09-09

Test-only experiments; production sync has not been changed.

Run: `cargo test --release -p client-library benchmark_incoming_candidates -- --ignored --nocapture`.

## Method

Same release binary, separate temporary disk databases, WAL with synchronous
FULL, 1,000 existing books with 10 KiB descriptions. Incoming tests use the real
remote reducer, version writes, language projection, cursor writes and commits.
11 alternating-order trials per case, medians. Input pages are already available:
network, storage-queue waiting, acknowledgment handling and HTTP decoding are
excluded. Four 32-mutation pages are a synthetic fixture, not a claim about the
server's normal page size. Writer duration starts after transaction acquisition
and includes commit. Final metadata blobs, language projections, incoming
metadata versions, cursors and empty outgoing queues are checked.

Lookup tests run inside an existing transaction, using cached statements in both
cases, 101 alternating-order trials. Candidate JSON serialization and result
collection are included. They measure only book ID/purge-state resolution, not
folder/version lookup batching or an integrated reducer cache.

## Repeat-run results

| Experiment | Current/baseline | Candidate |
| --- | ---: | ---: |
| 128 metadata mutations: four commits vs one | 58.37 ms | 34.33 ms |
| Longest writer interval for that experiment | 15.05 ms | 34.14 ms |
| Four metadata revisions each for 32 books vs final revisions only | 71.55 ms | 15.52 ms |
| 128 distinct books, identical algorithm control | 32.99 ms | 35.01 ms |
| One book lookup | 0.701 µs | 1.012 µs |
| 32 book lookups | 17.00 µs | 13.18 µs |
| 128 book lookups | 71.20 µs | 52.34 µs |
| 512 book lookups | 305.15 µs | 210.39 µs |

The preceding run measured page commits 60.60 → 32.78 ms and longest writer
15.61 → 32.59 ms; 128 lookups 71.83 → 50.89 µs. The identical-algorithm control
also varied (29.42 → 32.57 ms), so small total-time differences should not be
treated as reliable wins.

## Interpretation

1. **Page commit batching:** meaningful local savings (~41% in the repeat), but
   the longest writer interval more than doubles. A real implementation must
   buffer/fetch pages outside the transaction and preserve cursor/acknowledgment,
   failure and cancellation semantics. It must not hold the writer over network
   requests. This benchmark does not validate such an implementation.
2. **Batched lookups:** saves only ~0.019 ms at 128 books in this warm fixture,
   and regresses single-book lookups. This does not yet justify a complicated
   page-wide ID/version cache; those broader caches remain unbenchmarked.
3. **Projection coalescing:** the repeated-revision result is an upper-bound
   proxy that skips whole intermediate metadata mutations, NOT an isolated
   deferred-projection prototype. The server already upserts one current state
   per `(library_id,kind,entity_key,entity_subkey)` and pulls from that state
   table (`upsert_state_batch.sql`, `fetch_page.sql`). A normal static page
   therefore does not contain four versions of the same metadata register.
   Concurrent edits across pages can repeat a key, but this is not evidence of
   a common-path projection bottleneck. Cross-register deferred rebuilding has
   not been implemented or benchmarked here.
4. **Inventory round trips:** source inspection found no blocking inventory
   audit in ordinary `SyncEngine::synchronize`; `repair_inventory` is separate.
   There is no initial-sync audit round trip to remove on that path. No live
   network speedup was measured or inferred.

Recommendation: if pursuing one candidate, prototype bounded page buffering or
larger pull pages and validate writer latency alongside throughput. Do not ship
lookup or revision-coalescing changes based on these results alone.
