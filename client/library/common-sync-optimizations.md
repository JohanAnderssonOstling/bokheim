# Common sync optimizations — 2026-09-08

## Native folder-move projection paths — 2026-09-09

After a physical directory rename, the native publisher previously used a
`substr(relative_path,...)` predicate to rewrite stored file paths, scanning
every local placement under the writer. It now uses a BINARY half-open range
on a new nonunique `local_file_projection(relative_path)` index. For a prefix
`/Folder/`, the bounds are `/Folder/ <= path < /Folder0`; this exactly matches
descendants without LIKE wildcard or case semantics. Existing transaction,
filesystem durability, and retry ordering are unchanged.

Release same-binary benchmark: in-memory production projection table, legacy
without the new index versus candidate with it, 21 alternating-order trials,
median SQL-update times. Index maintenance is included; fixture creation,
rollback, disk commit, physical rename and other move work are excluded.

| Total placements | Moved placements | Before | After |
| ---: | ---: | ---: | ---: |
| 1,000 | 10 | 0.103 ms | 0.034 ms |
| 100,000 | 10 | 7.699 ms | 0.112 ms |
| 100,000 | 1,000 | 7.828 ms | 1.283 ms |
| 10,000 | 10,000 | 3.526 ms | 12.494 ms |

This targets small-subtree moves in larger libraries, not whole-library moves:
the latter regress because all paths require index updates. The additive index
also costs space, initial construction, and maintenance for ordinary projection
writes. No end-to-end latency improvement is claimed from this microbenchmark.

Release library validation: **217 passed, 14 ignored**, with the ignored
benchmark separately passing. New tests cover Unicode/wildcard/case boundaries,
sibling prefixes, legacy-equivalent results, rollback, indexed query planning,
and read-only subsequent index-upgrade checks.
Native release validation: **21 focused tests passed** (6 prepared-directory
tests, 14 tests matching `move`, and the changed-subtree reconciliation test).
These include physical book/folder moves, restart recovery, destination/identity
changes, and preserving newly queued work. The full backend suite was not run.

Reproduce: `cargo test --release -p client-library benchmark_move_projection_paths -- --ignored --nocapture`.

## Book/folder move destination naming

Moves previously read and Unicode-normalized every live destination sibling
while holding the write transaction. They now probe the requested portable name
and suffixes 2–8 using an index; heavier collisions retain the original bulk
algorithm. Folder moves reuse `idx_dir_live_path`; book transfers (including
copies) use a new nonunique expression index, `idx_book_dir_live_portable_name`.
The new index deliberately permits existing/synced case-equivalent filenames.
It is installed for fresh and current-schema libraries, with no schema write on
subsequent opens. Folder copy and trash restoration naming are unchanged.

Release microbenchmark, same binary, in-memory destination tables with relevant
production indexes, 31 alternating-order trials, median microseconds:

| Siblings | Book, free name before → after | Folder, free name before → after | Book, one collision before → after | Folder, one collision before → after |
| --- | ---: | ---: | ---: | ---: |
| 10 | 7.89 → 0.93 | 7.61 → 0.98 | 8.47 → 1.92 | 7.93 → 2.00 |
| 1,000 | 481.80 → 1.54 | 513.32 → 1.81 | 530.06 → 4.12 | 507.62 → 3.16 |
| 10,000 | 6544.45 → 11.64 | 5679.32 → 9.55 | 6098.07 → 9.78 | 5960.87 → 15.81 |

These timings measure **name selection only**, not complete moves, commits,
projection updates, filesystem operations, or sync/network latency. Fixtures
exclude production triggers. The book index adds storage and placement-write
maintenance, and an initial index build for existing libraries; those costs
have not been benchmarked. No application was launched or real library changed.

Validation: release library suite **215 passed, 13 ignored**; the ignored naming
benchmark also passed when explicitly run. New tests cover normalized Unicode
collisions, numeric suffix gaps and bulk fallback, deleted entries, self-excluded
folder moves, existing book destinations, index query plans, and index upgrades.
Native backend/filesystem integration was not rebuilt for this change.

Reproduce: `cargo test --release -p client-library benchmark_move_destination_names -- --ignored --nocapture`.

Next candidate from inspection: native folder materialization updates file
projection paths using a `substr(relative_path,...)` predicate, potentially
scanning unrelated placements. Its actual cost is not measured here.

## Rejected experiment: joined outgoing contributor lookup

Tested replacing the contributor encoder's per-credit author-identity lookups
with one LEFT JOIN. Encoded bytes were identical, including ordered mixed-role
and repeated credits; missing identities remained errors. The production encoder
was restored because common small-credit cases showed little benefit and the
zero-credit case regressed. Only the test implementation and benchmark remain in
`contributor_encoding_tests.rs`.

Two same-binary release runs, each 15 alternating-order trials, 32-book disk-WAL
fixtures. The repeat measured:

| Credits/book | Old payload generation | Joined payload generation | Old edit + commit | Joined edit + commit |
| --- | ---: | ---: | ---: | ---: |
| 0 | 0.447 ms | 0.699 ms | 6.842 ms | 7.082 ms |
| 1 | 0.786 ms | 0.708 ms | 7.066 ms | 7.018 ms |
| 4 | 0.986 ms | 0.946 ms | 7.854 ms | 7.824 ms |
| 8 | 1.479 ms | 1.377 ms | 8.209 ms | 7.678 ms |

The first run also had slower zero-credit payload generation (0.585 to 0.761 ms)
and essentially unchanged one-credit encoding (0.752 to 0.751 ms). These results
do not support adopting this as a general common-path speedup. Payload generation
here means the stored mutation body, not protobuf/HTTP encoding or network time.

Code inspection also found that rusqlite 0.40.2's function-context
`get_connection()` constructs a fresh non-owning Connection wrapper via
`Connection::from_handle`; a `prepare_cached` call on that wrapper does not retain
its cache across UDF calls. This is a potential explanation for repeated planning
cost, not a measured breakdown or justification for unsafe statement caching.

The full library suite passed 212 tests with the candidate. After restoring the
production encoder, the focused byte-equality/error test is rerun. All earlier
accepted optimizations remain unchanged. No deployment was performed.

Reproduce: `cargo test --release -p client-library --lib benchmark_outgoing_contributor_join -- --ignored --nocapture --test-threads=1`.

## Follow-up: exact unchanged-contributor fast path

The incoming-ID-preserving metadata path compares ordered contributor rows with
one cached query joining author identities. Exact matches skip identity preparation,
per-credit UPSERTs, trimming, and the nested contributor savepoint. Equality includes
position, stable ID, name, nullable sort name and role; author-name validation is
retained. A difference falls back to the existing preparation/update path. Local
name-based identity-reuse policy is not changed.

Same release binary, immediately previous publication-fast-path implementation
versus this additional optimization, 32 domain updates per transaction, 15 trials
with alternating order. Commit and remote-origin transitions are included. Wire
decoding, version/conflict checks, server/network and UI costs are excluded.

| Contributors per book | Changed credits? | Before | After |
| --- | --- | ---: | ---: |
| 0 | No | 16.522 ms | 8.454 ms |
| 1 | No | 19.119 ms | 6.706 ms |
| 4 | No | 44.955 ms | 7.173 ms |
| 8 | No | 68.007 ms | 9.700 ms |
| 4 | Last credited name changes each trial | 79.083 ms | 78.940 ms |

Unchanged-credit cases improved 49–86%; the changed-credit fallback was essentially
unchanged in this run. These are application-stage results, not an end-to-end sync
speedup or an estimate of how often contributors change in user libraries.

Validation: 211 library tests passed, 11 ignored, and the manually invoked benchmark
passed. Added tests check equality invalidation and correct application for ordering,
identity, credited name, sort name, role changes, and removal. No backend rebuild or
deployment was performed for this internal change.

Reproduce: `cargo test --release -p client-library --lib benchmark_unchanged_contributors -- --ignored --nocapture --test-threads=1`.

## Follow-up: unchanged publication metadata fast path

Incoming metadata whose publication value equals the stored value now updates only
the title column (using a cached statement), then applies contributor differences.
It no longer clones/re-encodes/rebinds the unchanged publication blob or targets
that column in SQL. Changed or undecodable publication values retain the existing
full update/projection path. This does not remove publication metadata from sync.

Same release binary, previous implementation retained only in the benchmark,
32 title-only domain updates per committed transaction, 15 trials with alternating
order. Fixtures include an author, publisher, ISBN, language and classified subject.
The extra-size cases add synthetic rights text to stress larger publication values;
these are not description sizes or a measured distribution of users' metadata.

| Fixture | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| Representative small publication metadata | 19.587 ms | 16.336 ms | 17% |
| Plus 1 KiB of publication data | 19.924 ms | 16.289 ms | 18% |
| Plus 10 KiB of publication data | 21.161 ms | 16.886 ms | 20% |

These timings include writer reservation, remote-origin flag changes and commit,
but exclude incoming wire decoding, conflict/version checks, transport and UI work.
They measure the domain-application stage, not end-to-end sync latency.

Validation for this follow-up: 210 library tests passed, 10 ignored; the manually
invoked benchmark passed. A new trigger-based test rejects any publication-column
update during a title-only edit and verifies stored publication bytes are unchanged.
The backend suite was not rebuilt for this internal, signature-preserving change;
the earlier backend results below still apply to that earlier revision.

Reproduce: `cargo test --release -p client-library --lib benchmark_title_only_publication_fast_path -- --ignored --nocapture --test-threads=1`.

Implemented without a schema change or reduced SQLite durability. The retained
payload prototype remains removed. Measurements use release builds and temporary
disk-backed libraries, never the user's library. Other desktop work was active;
small differences are not reliable speedup guarantees. Network/server time is excluded.

## 1. One remote-origin guard per incoming page

`apply_remote_changes_in_tx` lazily enters remote mode on the first winning
mutation and restores the prior mode on completion/error. The existing single-item
API is preserved. Unknown/stale-only pages do not write the flag. Native and browser
pull application both use the page API. A 32-winner page writes the flag twice,
instead of 64 times. Per-mutation conflict/version checks remain unchanged.

Same-binary comparison of per-mutation versus per-page scope, before applying
the statement-cache change; 1,000 books with 10 KiB descriptions, seven trials:

| Incoming workload | Per-mutation guard | Page guard |
| --- | ---: | ---: |
| One reading-position update | 6.286 ms | 6.369 ms |
| 32 metadata updates | 19.313 ms | 18.928 ms |
| 32 description updates | 18.016 ms | 17.767 ms |
| 32 stale metadata updates | 5.749 ms | 5.797 ms |

The measured latency benefit is modest and close to noise. This comparison uses
the retained single-item API with the same restoration/error-handling code as the
page API, not an old checkout. The flag-write reduction is independently tested.

## 2. Prepared SQL reuse

Cached the incoming winning-version read/write and purged-book check. Acknowledgment
prepares its delete statement once per batch and reuses it. Mutation-ID matching
is unchanged, so stale acknowledgments cannot delete newer queued edits.

Same-binary old generated delete method versus new prepared method, nine trials,
alternating order. SQL timings exclude transaction acquisition/commit; total includes both:

| IDs | SQL before | SQL after | Total before | Total after |
| --- | ---: | ---: | ---: | ---: |
| 1 | 17.1 µs | 12.8 µs | 5.570 ms | 5.534 ms |
| 32 | 138.0 µs | 60.5 µs | 5.611 ms | 5.476 ms |
| 256 | 840.1 µs | 296.8 µs | 6.499 ms | 5.857 ms |

Sequential before/after incoming-page runs showed no clear latency improvement:
32 metadata updates with the page guard measured 18.928 ms before caching and
18.992 ms afterward. Do not attribute earlier desktop-load differences to caching.

## 3. Native combined acknowledgment/pull commit

The sync engine validates both response sections and requests combined persistence.
For native responses with up to 256 acknowledgments, the account-checked storage
command deletes acknowledged mutations, applies incoming changes, and advances the
cursor in one transaction. No acknowledgment IDs uses the existing pull path;
larger acknowledgment sets retain the bounded separate path.

Failure semantics:

- Invalid push acknowledgment sets still fail before applying anything.
- A malformed pull envelope still persists independently valid acknowledgments.
- Pull application/commit failure rolls back the combined transaction; valid
  acknowledgments are then retried separately, under a fresh account check, before
  returning the pull error. A crash before that retry may resend already accepted
  IDs, consistent with at-least-once delivery; no uncommitted pull is announced.
- A stale acknowledgment cannot clear a newer edit, and an old account response
  cannot clear another account's pending changes or advance its cursor.
- Browser storage retains its existing separate command flow; combined commits
  are native-only. No browser command protocol change was introduced.

Same release backend binary, actual queued `AccountStore` calls, 1,000-book fixture,
nine trials with alternating order. Both paths already include optimizations 1 and 2:

| Acknowledgments / incoming mutations | Separate | Combined | Reduction |
| --- | ---: | ---: | ---: |
| 1 / 0, advancing cursor | 21.210 ms | 10.415 ms | 51% |
| 1 / 1 metadata update | 20.804 ms | 9.625 ms | 54% |
| 32 / 32 metadata updates | 36.331 ms | 24.770 ms | 32% |

These backend timings include storage-command dispatch and must not be compared
directly with the direct-SQL acknowledgment timings above. Fixture construction,
local edits generating pending work, and synthetic response construction are excluded.

## Validation and reproduction

- Library suite: 209 passed, 9 ignored.
- Backend suite: 408 passed, 7 ignored, 1 failure in
  `runtime::tests::directory_import_opens_files_only_after_previous_import_finishes`.
  That import error-message assertion also failed in isolation; the import code
  was not changed by this work. All new sync tests passed.
- All three manually invoked benchmark tests passed.
- New tests cover origin-write counts, stale pages, restoration on errors, rollback,
  failed combined pull/cursor writes retaining acknowledgments, stale acknowledgments,
  and account rebinding. No application deployment was performed.

Commands (from the client workspace):

```sh
cargo test --release -p client-library --lib benchmark_common_incoming_sync -- --ignored --nocapture --test-threads=1
cargo test --release -p client-library --lib benchmark_ack_statement_cache -- --ignored --nocapture --test-threads=1
cargo test --release -p bokheim-backend --lib benchmark_combined_response_commits -- --ignored --nocapture --test-threads=1
```
