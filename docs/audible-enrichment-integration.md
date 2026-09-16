> Current implementation: M4B files are read-only. Audible lookup results,
> chapters, identifiers and job completion commit together in the library
> database. The separate application stage, publication-enrichment crate and
> vendored mp4ameta writer patch have been removed. Standard mp4ameta remains
> for metadata and cover extraction. Writer checkpoints below are historical.

# Audible enrichment integration

Status: in progress; native and browser workers dispatch database-only Audible jobs. Shared audiobook inspections queue work for imports, scanner discovery and browser playback recovery. Browser runtime and full cross-device validation remain incomplete. Existing browser entries without jobs require a fresh inspection.

Database-only browser wiring validation (2026-09-07): optimized release build of
`bokheim-backend --no-default-features --target wasm32-unknown-unknown --example sync_coordinator`
passed; all 157 `library` library tests and 16 backend enrichment tests passed
with `cargo test --release`. This verifies compilation and native regression tests,
not an end-to-end browser session. Enrichment tests assert unchanged M4B bytes,
transaction rollback, rescan preservation, and Audible-before-Open-Library ordering.

The full behavior is specified in the audiobook workspace:
`.migration/chapter-enrichment/BOKHEIM_PIPELINE_PLAN.md`.

## Implemented foundation

- M4B scanning retains recording evidence: album, explicit subtitle/narrator/publisher,
  validated ASINs/ISBNs, raw date and separately tagged recording release date.
- Embedded identifiers enter publication metadata with `embedded-m4b` provenance.
- Recording evidence is persisted alongside local audiobook metadata and cascades on deletion.
- Shared bibliographic/author normalization is extracted unchanged from Open Library
  matching into `metadata_contract::matching`; Open Library uses that shared implementation.
- Audible request/candidate/result/chapter-plan contracts distinguish unavailable,
  no match, ambiguous and supported outcomes.
- Settings progress and bounded background write batches already exist. Audible must
  participate in their progress accounting once it is connected.

## Remaining required integration

1. Bounded, cached server-side Audible adapter accessible to native and WASM clients:
   embedded ASIN, API first, website ASIN discovery when no suitable API result,
   structured product and precise chapter-tree lookup. Distinguish transient errors
   and no-match; bound response size, candidate counts, concurrency and retries.
2. Shared-normalization query preparation retaining subtitle and recording clues;
   title/duration-led ranking, author/narrator support, recording-date tie-break,
   regional-variant grouping and explicit ambiguity. Do not derive publishers from blurbs.
3. Descriptive-chapter planning: equal-count names only without changing timestamps;
   section/number/context anchors with local span-duration checks; bounded experimental
   duration-pattern alignment; boundary import only for substantially coarser local
   structure with recording support. Preserve descriptive names, reject conflicts,
   and never scale times to force a match.
4. Resumable per-M4B pipeline before Open Library. Refresh missing evidence for existing
   files, fingerprint inputs/policy to invalidate stale results, persist provenance,
   feed validated Audible ISBNs into the normal metadata flow, and expose status.
5. Embed enriched chapters and identifiers into M4Bs after atomic staging and
   audio-preserving verification. Refresh local playback metadata and synchronize
   the resulting file revision; database-only overlays do not complete enrichment.
6. Ensure slow Audible requests do not strand ebook work; preserve active-library
   cancellation and bounded yielding/worker-based alignment. Implement effective
   native background queue priority and test interactive overtaking.
7. Verify mocked API/fallback/error/cancellation paths, matching and alignment controls,
   persistence/resumption, playback integration, native/WASM builds and representative
   on-disk fixtures from the Python experiments. Do not claim integration from
   foundation-only tests.

Foundation validation: 12 book-metadata tests, shared normalization test and recording
evidence persistence/deletion test passed. Metadata server and library SQLite compiled.

## Adapter and planner checkpoint

- Added server `/v2/audiobooks/audible` and native/WASM metadata-client method.
- Adapter: embedded ASIN -> API title/subtitle and normalized-author queries ->
  website product links -> structured ASIN and precise chapter-tree lookups.
  Nonempty but unsuitable API results trigger website fallback. Requests have
  deadlines, response-size limits, two concurrent lookups, rate limiting and a
  bounded in-memory response cache. Failures are not cached as successful no-match.
- Recording selector uses shared normalization, precise runtime, supporting credits,
  separate recording-date tie-breaks and regional equivalence; retains ambiguity.
- Chapter planner baseline covers equal-count names, section/number/context anchors,
  bounded duration-pattern matching and coarse-to-fine boundary imports. Selection
  and planning run on server blocking workers. Existing descriptive names remain.
- Eight adapter/parser tests passed, including a real public Audible response for
  B0DWXY6C69 (22 chapters, 48,731,333 ms and ISBN 9781668651223).
  Eleven selection/alignment tests passed.
- Still not connected to library enrichment or playback. Next: persistent candidate
  queue, stale-evidence handling, chapter overlay application, ISBN handoff and
  progress accounting. Full requirements above remain active.
- Before enabling application: extend duration-pattern plans to combined local
  sections and shuffled controls, audit repeated/uniform-duration false positives,
  test broader real library fixtures, and verify partial request failure cannot
  prematurely resolve an ambiguous set. Also validate effective native scheduling.

## User decision: write enriched metadata into M4B

The user explicitly requested self-contained M4Bs. Verified file writing is now a
required part of successful enrichment, not merely optional export or a database
chapter override. Preserve the original book identity, audio, covers and unrelated
tags; replace the source only after staging and verification. Do not retain duplicate
originals after successful atomic replacement.

Implemented writer foundation in publication-identity:
- native adjacent-temp-file publication with concurrent-change checks and fsync;
- chapter lists/tracks plus ASIN/ISBN tags;
- stream-based encoded audio sample fingerprinting, including audio configuration
  and timing tables, excluding mutable chapter tracks;
- read-back verification of chapters, all metadata/cover data, duration and identity;
- untouched source on invalid input or failed verification.

Next integration must connect the resumable enrichment job to this writer through
the asset store, account for file revisions/sync publication, and provide equivalent
safe browser storage staging. A database-only result must not be reported as an
embedded M4B update. Native writer tests alone do not prove pipeline completion.

## Review corrections

- M4B write plans now require the raw file revision captured with lookup evidence.
  Permanent identity alone is insufficient. Revision mismatch aborts before staging
  can replace the source, including when a newer chapter edit retains the same ID.
- Plausible candidates missing precise chapter duration keep selection unavailable
  and retryable; another completed candidate cannot hide that unresolved alternative.
  Unrelated titles do not block a valid match.
- Coarse boundary imports check for contradictory unique numbered/named/section
  anchors, even where the positive alignment has too few anchors to succeed.
- Duration-pattern evidence requires at least three distinct duration groups on
  each side, beyond matching tolerance, as well as the existing coverage and
  independent-segment thresholds. Uniform generated tracks do not prove identity.
- Website ASIN extraction uses the final product-path segment, preventing a
  ten-character title slug such as Foundation from replacing the actual ASIN.

Regression coverage includes stale-file preservation, unresolved candidate recovery,
conflicting anchors, uniform tracks, valid varied-duration alignment with an extra
introduction, and both direct and title-prefixed product URLs.

Second review corrections:

- Reject incomplete chapter coverage, including missing audio at container edges
  or between children, and discrepancies with declared millisecond runtime
  (allowing at most 1 ms rounding). Partial trees cannot supply precise runtime.
- Recognize structural section roles on descriptive headings, so an introduction
  cannot be renamed to a titled foreword; matching titled introductions remain
  eligible for names-only enrichment.
- Treat composite disc/track numbers, bare chapter markers and number-only labels
  such as Chapter One Hundred as generic, while keeping descriptive suffixes.
- Merge incoming ISBNs into generic and length-specific tags, preserving existing
  values and deduplicating canonical equivalents across repeated writes.

Validation: 18 audible contract tests, 11 audible server tests (including the
observed response fixture), and 13 publication identity tests passed. The writer
regression verifies multiple ISBNs survive round-trip and repeated enrichment,
with audio fingerprint and permanent identity preserved.

## Native scheduling and durable jobs

Native storage commands now use the same bounded interactive/background ordering
as browser commands, on a dedicated serial worker for synchronous storage units.
Interactive commands overtake queued background commands; an eight-command burst
limit prevents background starvation. Cancelled callers do not start queued writes,
and a panicking command cannot stop subsequent work. Already running operations
remain non-preemptible. The submitting Tokio runtime context is retained.

`client_library::audible_jobs` adds persistent per-book discovery,
resolution and file-application states. Claims have expiring tokens; changed
file revisions, lookup inputs or policy invalidate older claims. Unavailable
responses respect retry deadlines. Supported responses retain candidate provenance
and resume file application after interruption, without being counted as applied.
Verified application records the new file revision and refreshed evidence, retaining
original lookup evidence for audit. Deleting audiobook metadata removes its job.

These storage APIs still need library discovery, provider dispatch, verified asset
publication/recovery, ISBN handoff and progress wiring. They do not enable Audible
requests or claim that files have been enriched. Browser file staging and complete
playback/sync integration remain required.

Release validation for this checkpoint: all four durable-job tests passed. The
platform suite passed 54 tests and exposed one scanner assertion that still
expected whole-file M4B identity. Updating that assertion to the current audio
identity contract made its focused release rerun pass. The five queue tests
include runtime-context preservation as well as priority, fairness, overload,
cancellation and panic recovery.

Current-worktree audit: publication-identity now derives M4B identity from audio
rather than embedding the original byte hash. Its earlier `enrich_m4b` API is no
longer present in the current source. The writer checkpoint above is historical;
verified chapter/identifier writing must be restored against the current identity
contract before connecting supported jobs to file publication.

## Writer restoration and native discovery

Restored verified M4B writing against audio-derived identity. The shared
`stage_m4b_enrichment` API accepts random-access storage, checks the raw planned
revision, copies into an empty stage and verifies audio identity, chapter names/
boundaries, covers and unrelated tags. Native publication additionally rechecks
source bytes/file identity and atomically replaces the original. No identity tag
is inserted into M4Bs. ISBN values are merged rather than overwritten.

The stock mp4ameta writer allocates entire moved ranges when metadata changes
container offsets. The small local vendor patch adds `write_with_source`:
staging reads those ranges from an independent original snapshot in 64 KiB
chunks. Existing in-place callers retain upstream behavior. The regression test
moves an 8 MiB range through a reader that rejects requests larger than 64 KiB.
This shared staging API still needs a browser worker/OPFS publication adapter;
compiling it for WASM does not prove responsive browser execution.

Native scanning now discovers durable Audible jobs after recording inspection,
including the actual file-byte revision. Existing files lacking a current-policy
job are reinspected. File-derived chapters/recording evidence refresh after tag
edits, while independently edited library bibliographic metadata stays intact.
Unchanged lookup evidence preserves previous results; changed policy/file inputs
invalidate them. Provider dispatch, application/recovery through the asset store,
ISBN handoff, status accounting and browser discovery remain unfinished.

Validation for writer restoration/discovery: 16 publication-identity release tests
passed, as did 25 native scanner tests. The expanded existing-M4B regression also
passed its focused release rerun: missing jobs backfill, unchanged no-match
results survive scans, policy changes rediscover work and tag edits preserve
book identity while refreshing the captured raw revision. The shared writer
built successfully for wasm32-unknown-unknown in release mode; publication-model
now enables UUID's JS randomness source for that target. No browser playback or
OPFS publication claim follows from this library-only build.


## Native dispatch and application

Verified writing now lives in `shared/publication-enrichment`, separate from the
concurrently refactored identity crate. Its shared staging API and small mp4ameta
vendor patch retain bounded moved-range copies; native writes support cancellation
between staged I/O operations and never insert an M4B identity tag.

Native enrichment schedules an independent active-library-scoped Audible activity.
It claims only visible live books under the current policy, calls the metadata
client outside storage commands, persists retryable failures, and applies supported
plans through the asset store. Already-completed local file copies are recognized
on recovery; all local placements are checked before any missing update is written.
An exclusive asset lease spans file publication and the referencing metadata commit.
Long verification renews the job claim without retaining a database connection.

Successful application refreshes embedded chapter metadata used for playback,
merges ASIN/ISBN identifiers with Audible provenance, invalidates stale Open Library
attempts when a new ISBN arrives, and forgets the old remote-asset fact so the new
file revision is scheduled for synchronization. Automatic Open Library candidate
queries defer books still in the initial Audible resolve/apply stages. Other ebooks
retain their independent activity. Existing settings counts now include Audible
applied, pending, review, no-match and unavailable states.

Remaining gates include browser worker/OPFS discovery and application, real
cross-device revision publication, broader recording fixtures and planner coverage,
and race validation when an older Open Library request overlaps a later Audible
retry. Native mocked integration is not evidence that those gates are complete.
