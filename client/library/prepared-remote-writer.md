# Preparing incoming pages before the writer — 2026-09-09

Native incoming pages now decode mutation bodies and construct state identities
on the storage worker **before** `library_write_transaction`. The shared browser
page helper also prepares before starting its transaction. Account checks,
version comparisons, lifecycle checks, domain writes, acknowledgments and cursor
updates remain transactional. Batch sizes and commit counts are unchanged.

Prepared values contain no database snapshots. A newer local edit made after
preparation still wins under the ordinary version gate. Decode failures occur
before any page write; combined-response acknowledgment fallback remains intact.
Unknown kinds retain their skip behavior. Decoded memory is bounded by one page.

This moves CPU work off SQLite's writer, not off the storage command queue.
Other queued commands can still wait for preparation; no end-to-end UI latency
claim is made. Classification/projection and publication encoding remain inside
the transaction, so this is a limited contention improvement, not a full fix.

## Measurement

`cargo test --release -p client-library benchmark_prepared_remote_writer -- --ignored --nocapture`

Same release binary and preparation/reducer implementation, decode under writer
versus before writer. Two temporary disk libraries, WAL/FULL, 128 existing books,
11 alternating-order trials, median lock duration including commit. Input page
construction, HTTP, and queue waiting are excluded. Final book metadata and
absence of outgoing echo mutations are checked. Full payload preparation is
timed separately; it is moved, not eliminated.

| Incoming page | Writer before → after, first run | Writer before → after, repeat |
| --- | ---: | ---: |
| 32 winning metadata changes, 1 KiB rights each | 18.25 → 17.93 ms | 18.45 → 18.61 ms |
| 32 winning changes, 64 KiB rights each | 62.42 → 64.31 ms | 71.45 → 68.61 ms |
| 128 stale changes, 64 KiB rights each | 3.21 → 0.208 ms | 3.75 → 0.219 ms |

No consistent overall lock-time gain was measured for winning pages: decoding
was only ~0.07 ms for small payloads and ~0.33–0.39 ms for the large ones, below
the variation in the much larger database/commit cost. Large stale pages show
a repeatable ~94% writer-time reduction, with no extra commits. This is a stress
case, not a claim about typical payload size or stale-page frequency.

Repeat-run total preparation-through-commit time: small winning pages
18.57 → 18.88 ms; large winning pages 71.69 → 69.66 ms; stale pages
3.89 → 3.30 ms. A production-wide throughput benefit is not established.

Validation: release library suite **218 passed, 16 ignored**; native account
store tests **4 passed, 1 ignored**; native library-sync tests **41 passed,
4 ignored**. The benchmark passed twice. Browser runtime and live PostgreSQL
end-to-end tests were not run. No application was launched or deployed.
