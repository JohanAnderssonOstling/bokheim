# Metadata answers

`metadata-server` imports dumps, builds immutable SQLite answers, serves
`POST /v3/books`, and can fold archived provider evidence into builds.
The server reads one answer file and writes a small demand database. Its only
external provider is Audible; it neither reads LC/LT caches nor loads API keys.

## Direct work lookup

Work lookup searches title and author together, including works without
classifications. Build its composite search index once from the immutable import
file used for the answers, then configure that source:

```sh
metadata-server index-work-search /absolute/path/authorities.sqlite
export BOKHEIM_METADATA_WORKS=/absolute/path/authorities.sqlite
```

The command creates `authorities.sqlite.work-search.sqlite3` beside the canonical
source path. It normalizes author credits offline and indexes each title with
compatible author search keys. Requests retrieve candidate work IDs directly by
both fields before loading titles or author metadata. Common titles therefore do
not load thousands of unrelated authors. Initials, reordered names, omitted given
names, coauthors and reviewed aliases remain eligible; the existing full-name,
qualifier and ambiguity checks still decide identity. Compact hashed keys only
retrieve candidates: original title keys and complete author credits are checked
before accepting a match. The 4,096 candidate ceiling applies after title/author
filtering. Queries requiring more than 512 author search keys abstain as ambiguous
with truncation.

The search artifact is required when a work source is configured. Startup checks
its version and the source file size and modification time; a missing or stale
index is rejected. Rebuild the artifact for changed sources or search-key rules.
Index construction is offline, leaves the source read-only and does not rebuild
ISBN answers. Work lookup never searches edition records.

Configure this environment variable for `serve` or `evaluate-subjects`; the import
schema must be 9 or 10 and its dump date must match the answer snapshot. Keep the
source file immutable while it is served. Source imports retain works referenced
by retained editions; lookup covers those imported works, not the entire Open
Library work catalogue. All source connections are read-only and use bounded
SQLite caches.

`POST /v3/works` accepts title-and-author queries and returns work IDs:

```json
{"queries":[{"query_id":"0","title":"Emma","authors":["Jane Austen"]}]}
```

Each result has `query_id`, `status`, `matched`, `candidates`, and
`candidates_truncated`. Status is `matched`, `ambiguous`, `no_match`, or
`insufficient_metadata`. Matches contain `open_library_work_id`, `title`, and
`authors`. Missing authors do not establish a work identity. Duplicate work
records and conflicting full author names remain ambiguous; lookup does not
invent an ISBN. A title/author search exceeding 4,096 candidates abstains with truncation;
at most 64 review candidates are returned. Requests contain 1–128 queries.
Without a configured work source, this endpoint returns HTTP 503.

The same lookup is available without running the HTTP server:

```sh
metadata-server lookup-works authorities.sqlite request.json
```

With the work source configured, enrichment carries the verified work IDs into
every subsequent query. A publisher and year first get a separate edition lookup
within those works; a unique compatible edition retains exact LCC priority.
An unambiguous work can then supply its own declared LCC.

Build the compact edition indexes once for the immutable imported source:

```sh
metadata-server index-work-editions authorities.sqlite
```

This creates `authorities.sqlite.work-editions-v2.sqlite3` beside the canonical
source path. Work lookup opens it automatically, validates the source size and
modification time, and reports `work_editions_indexed` in operations. A stale
index is rejected. Build a new index for each changed source; no answer rebuild
is needed. The source stays read-only. Index construction is offline and never
runs inside a lookup. Classified editions are indexed directly by work, with
their titles, subtitles, author credits, canonical ISBNs, years and LCC codes
stored together. Consensus reads this covering evidence instead of joining the
large import tables for each sampled edition. Full title/author checks still
run on the stored evidence. Publishers are read separately only for exact
publisher/year identification. Version 1 artifacts remain available to older
binaries; build the version 2 artifact before upgrading to enable indexed access.
Publication candidates are indexed by work and year before publishers are read.
At most 128 year candidates are checked; a larger set abstains from selecting an
exact edition. Without this artifact, requests inspect at most 512 edition
references across all verified works. A truncated reference set cannot establish
an exact publisher/year identity and may reduce subject recovery.

When work codes are absent or several compatible work IDs remain, enrichment
reads directly classified editions only from those work IDs. It uses at most
32 distinct ISBN/edition observations and examines at most 128 classified
candidate records to allow for duplicate ISBNs and incompatible titles, authors
or volumes. These budgets apply across all matching works, not separately to each.
Selection uses indexed work/edition order in pages of 16. It is a deterministic
bounded evidence sample, not a random population estimate. Unclassified edition
bodies are not loaded. Duplicate ISBNs vote once; missing ISBNs use edition IDs.
More than 128 compatible work IDs causes abstention.

The weighted voting rule runs over the selected observations. Partial inputs with
fewer than eight eligible classified observations abstain. Sample metadata records
the examined count, classification limit and truncation; subject labels say
`weighted edition sample support` when evidence was truncated. Samples cannot
select an ISBN or establish a unique edition. If work evidence abstains, the
existing bounded LC/Wikidata authority lookup can still recover classifications.
Neither outcome restarts the broad edition-title search.

Unmatched work titles and unresolved author conflicts never trigger a broad
edition-title search. Edition records without a verified work are excluded from
title/author recovery; translated edition titles absent from the work index also
abstain. Independent LC/Wikidata authority lookup remains available. Without a
configured work source, enrichment uses ISBN answers and independent authorities
only. ISBN lookups still retain exact edition classifications, including editions
without a work link. Policy `books_v34` removes the unscoped edition fallback.
Policy `books_v35` adds indexed edition access and bounded reference scans.
Policy `books_v36` requires indexed title/author candidates for work lookup.
Policy `books_v37` adds covering edition evidence and reuses author counts during
voting. With a work source configured, the consensus taxonomy is prepared during
startup before the service accepts requests, rather than on its first fallback.
Alternative title readings run only after the primary lookup lacks supported
subjects, one at a time, stopping at the first supported alternative.

```text
OL / LC MARC / Wikidata + optional archived provider.sqlite + demand.sqlite
    -> build -> answers-DATE.sqlite -> serve -> /v3/books
```

## Import and build

```sh
cargo build --release -p metadata-server
metadata-server import EDITIONS.txt.gz WORKS.txt.gz AUTHORS.txt.gz current.sqlite
metadata-server import descriptions WORKS.txt.gz descriptions.sqlite 2026-09-01
# Existing immutable catalogs can gain edition descriptions without a reimport:
metadata-server import-edition-descriptions EDITIONS.txt.gz current.sqlite
# Optional: the existing Python extraction/import tools produce LC MARC and
# Wikidata supplements. Fold Wikidata into a COPY of the imported snapshot:
cp current.sqlite authorities.sqlite
metadata-server import authorities current.sqlite wikidata.sqlite authorities.sqlite

BOKHEIM_METADATA_DESCRIPTION_DATABASE=descriptions.sqlite \
BOKHEIM_METADATA_DEMAND=/var/lib/bokheim-metadata/demand.sqlite \
metadata-server build authorities.sqlite - answers-2026-09-30.sqlite
```

Optional environment variables may be omitted. Pass `-` to omit archived provider
input. The builder accepts Open Library import schemas 9, 10 and 11 and
validates the sidecar dump date. Inputs must be complete, immutable dump files;
archived provider results are read in one SQLite transaction.

Schema 11 retains valid edition descriptions independently of ISBN/LCC admission.
For existing catalogs,
`import-edition-descriptions` reads the matching edition dump into
`CANONICAL_SNAPSHOT_PATH.edition-descriptions.sqlite3`. This resumable import
publishes the supplement atomically after an integrity check and pins it to the
catalog's size and modification time. It stores only nonempty descriptions with
ISBN and work indexes; editions with several distinct work links are eligible
only for exact-ISBN lookup. Keep the supplement beside its catalog when deploying
and restart the server after publication. A stale supplement is rejected at
startup. The answer builder also reads this supplement and includes its identity
when deciding whether previous answers can be reused.

Policy `books_v38` adds verified title/author work descriptions. Policy `books_v39`
adds edition descriptions when those works have no description: an exact ISBN is
preferred, followed by editions linked to the verified work. Different nonempty
descriptions abstain rather than selecting an arbitrary edition. An exact ISBN
can supply a description without a work match. This fallback performs indexed
local reads; it neither searches unrelated edition titles nor calls a provider.
Existing file descriptions still take precedence when the client applies
enrichment. Previously completed books need enrichment requested again to gain
new descriptions.

Policy `books_v40` honors the per-book `needs_description` request flag. Clients
set it from whether the local book needs a description, without uploading its
existing description. When false, the server skips shared ISBN-description reads
and work/edition-description fallback and does not copy an Audible description.
Other enrichment still runs. Omitted flags default to true for older clients.

Every ISBN from the dumps, providers or demand is resolved without network I/O.
ISBN lookup does not infer LCC by matching titles and authors across separate
Open Library works.
Provider results fill missing classification schemes while preserving existing LCC.
Answers use JSON BLOBs with explicit references to shared work descriptions;
edition keys and authority candidates preserve the existing scoring rules.
The builder resolves 4,096 ISBNs per batch using indexed bulk queries, including
LC metadata and provider evidence. Exact-edition LCC stays authoritative.
Work-only LCC uses distinct classified edition records: each record votes once
per top-level subtree, regardless of call-number variants. The denominator
includes every retained edition with a parseable numeric LCC, including unmapped
numbers; bare subclasses and unclassified editions do not vote. A primary subtree
requires at least two records, 90% support and no secondary subtree above 10%.
Imported numeric work codes may replace sibling variants only when all map below
the root into that corroborated subtree. Otherwise the resolver retains supported
sibling codes and compatible imported codes, preserving substantial secondary
branches and unknown codes. With no corroborated primary subtree, a sibling code
still needs two record votes, or the sole classified record; imported work codes
remain available. Sparse evidence is not discarded for failing the percentage
test. No raw work-code count limits eligibility, and later merges preserve LCC
rather than re-filtering it by counts of code variants. ISBN answers and title
candidates use the same policy. This agreement check does not prove semantic
correctness, and retained edition records are not necessarily independent
cataloging decisions. Existing published answers require a new build.
The builder reuses
prepared SQL statements and shared-description lookups and commits once per
batch with SQLite's FULL durability setting.
Selected work classifications, descriptions and BISAC share a build-local
cache, bounded to 32 MiB of estimated allocations, including negative results.
Edition indexing enumerates exact/work evidence owners and editions whose
lowest ISBN has a classified answer, then loads their bibliographic fields.
If title-author authority evidence is present, it conservatively visits all
ISBN editions. Empty authority tables are skipped.
OUT is published only after all tables, manifest and integrity checks succeed.
An existing OUT is never overwritten.
Import schema 10 retains editions with numeric LCC even when they have no valid
ISBN. Older imports discarded their edition identity before title indexing.
To recover those records from the same pinned dumps without replacing an
existing source or resolving its ISBN answers again:

```sh
metadata-server import-subject-editions EDITIONS.txt.gz WORKS.txt.gz AUTHORS.txt.gz subject-editions.sqlite
metadata-server extend-edition-index subject-editions.sqlite answers-current.sqlite answers-with-subject-editions.sqlite --limit 8192
# Extend the measured batch to all remaining eligible editions:
metadata-server extend-edition-index subject-editions.sqlite answers-with-subject-editions.sqlite answers-complete.sqlite
```

The import scans each compressed dump once, retains classified editions without
ISBNs and their referenced works/authors, and checkpoints for resumption. Use
complete dumps from the original dump date. An optional final record limit is
for sampling only; such a limited import is not a complete source snapshot.
The index command can also extend a full schema-10 source directly. It publishes
only additions in a cumulative overlay, retaining the original answer base.
Existing ISBN answers and authority indexes stay unchanged. Its optional
`--limit` bounds visited editions; omitted, it processes all eligible additions.
The JSON report separates source selection and indexing time and says whether
all eligible editions were visited. Indexing still validates numeric LCC and
uses normal title/author agreement and classification corroboration. Candidates
without ISBNs keep edition/work IDs in subject evidence; their compatibility
field `canonical_isbn13` is empty, and enrichment never attaches an empty ISBN
or collapses distinct editions on that empty value. Lookup code must be updated
before serving an index containing SQL NULL ISBNs. Build recipe 4 records the
new candidate rules; earlier recipes require migration or a fresh build for
normal provider updates.

The JSON report includes ISBN count, answer JSON bytes, SQLite bytes, elapsed
time and throughput, plus separate timings for preparation, lookup, providers,
writing, edition indexing and authority indexing. Progress
logs include edition counts and lookup, provider and write times so production
measurements can guide further encoding or query changes.

To update a compatible snapshot after provider or demand changes:

```sh
metadata-server build authorities.sqlite provider.sqlite answers-next.sqlite \
    --previous answers-current.sqlite
```

Use the same optional description and demand environment variables.
Full builds retain compact classification summaries, exact provider evidence,
demand counts and immutable input identities for this comparison. Provider
additions, edits and deletions are detected even if their timestamps are unchanged.
The updater refreshes affected ISBNs and edition candidates and records demand
count changes without resolving unchanged metadata. Authority data stays fixed
with the imported input.

To apply the work-classification rule directly to an existing full database:

```sh
metadata-server reclassify authorities.sqlite provider.sqlite answers-current.sqlite
```

This uses SQL updates in one WAL transaction. It checks work-derived LCC,
writes changed answers, updates affected edition search entries, and folds
pending provider/demand evidence into the same transaction.
Descriptions, authors, non-LCC classifications and unchanged answer
bytes are preserved. The whole transaction rolls back on failure. Server
readers use ordinary SQLite `mode=ro`, so existing connections observe
committed changes.
Older servers using `immutable=1` must be replaced or stopped before maintenance.
The deployed systemd services have `ProtectHome=read-only`. Run maintenance
outside that sandbox. For the initial switch from DELETE journaling to WAL,
stop long-lived readers, initialize readable WAL/SHM files through a writable
maintenance connection, and start the read-only services while that connection
remains open. Subsequent WAL transactions can run alongside service readers.
The report separates `reclassified_rows` and `reclassified_editions` from full
metadata resolution in `resolved_rows`.

Reclassification requires current recipe bookkeeping and the exact recorded
immutable inputs. Incremental builds use a fresh full build when their previous
inputs or recipe are incompatible.



With identical inputs, OUT reuses the previous immutable file without resolving
ISBNs or copying it. Changed builds publish a small cumulative update database
with an embedded manifest pointing to the original immutable base. Readers merge
changed rows with the base through indexed views; deletion markers hide removed
base rows. Subsequent updates copy only the previous update database. Integrity
checks cover the update file, without copying or scanning the full base.
The server reads both full snapshots and update snapshots.

Keep the base at its recorded absolute path for as long as any update refers to
it. Its size and modification time are checked when opening an update, and the
base is opened with SQLite `mode=ro`. Bases referenced by overlays remain pinned;
in-place maintenance operates on full databases rather than overlays. The manifest
is published atomically inside the update database;
there is always one base and one cumulative update, with no growing layer chain.
The report's `file_bytes` measures the published file alone; an update also
depends on its retained base. As cumulative changes grow, run a fresh full build
without `--previous` to establish a new base, then update that base incrementally.
An older answer file without build state, changed imported/description
files, an unsupported builder recipe change, or more than 100,000 changed/affected ISBNs
falls back to a full build. Immutable file identity includes canonical path,
size and modification time, plus change time on Unix. Bump `RECIPE_VERSION` in
`src/build/incremental.rs` when resolution or candidate rules change.
The report's `mode`, `resolved_rows` and `reused_rows` distinguish these paths.

## Serve and activate

```sh
BOKHEIM_METADATA_ANSWERS=answers-2026-09-30.sqlite \
BOKHEIM_METADATA_DEMAND=demand.sqlite \
metadata-server serve
```

`BOKHEIM_METADATA_BIND` defaults to `127.0.0.1:8091`. A non-loopback bind requires
`BOKHEIM_METADATA_ALLOW_PUBLIC_HTTP=true`; production uses a TLS reverse proxy.
The answer file opens read-only, validates its manifest and tables at startup,
and remains pinned for the lifetime of the process. To activate a completed
build, atomically replace the `answers-current.sqlite` symlink and restart
`bokheim-metadata.service`. Existing readers continue using their previous file.
Keep the previous dated file for rollback.

Code-only deployments reuse `answers-current.sqlite`; they do not run a build:

```sh
servers/metadata/deploy/deploy-code.sh /path/to/metadata-server /path/to/taxonomy.sqlite3
```

Run this on the production host after uploading the compiled server. It shares
the release lock and invokes the existing activation helper with its health,
book endpoint and CORS checks.

The service template enables work lookup through
`/home/johan/data/openlibrary/works-current.sqlite`; point this symlink at the
immutable import with the matching dump date and built work indexes. The
activation helper installs the runtime taxonomy alongside the server and rolls
both back together on failure. After updating these deployment scripts or the
service template, run `provision-metadata-deploy.sh` as root once to refresh the
installed helper and template before deploying.

Subject enrichment policy `books_v26` prefers exact-edition LCC, then the
matched work's explicitly declared LCC, then sibling-edition evidence. A
record with any `manual:*` subject assignment or exclusion keeps its subjects;
automatic subject classification and heading recovery are skipped. Other
enrichment, such as author identities and covers, can still run. A
non-manual record has old borrowed classifications (including LCC, BISAC and DDC) and previous automatic subtree decisions
removed before lookup, so those saved results cannot prevent reevaluation or
accumulate stale assignments. A
title/author match checks the matching works' declared codes before pooling
candidate edition classifications. Publisher and year must both agree for a
title match to prefer a uniquely identified edition over work codes; the EPUB
creation year alone does not establish an edition. A work match supplies subjects without
attaching an arbitrary edition ISBN. Declared-work subjects retain the Open
Library work URL in their identifier and use the distinct source
`openlibrary:metadata:declared-work`. Cached ISBN answers apply the same priority
when read, preserving exact-edition LCC and replacing pooled work codes when
declared work codes exist. When no declared work code exists, inherited LCC in
old ISBN answers cannot bypass the sibling consensus lookup. Cached pooled work
classifications in other schemes are also discarded; exact-edition codes remain.

Policy `books_v27` and subject matcher version 149 enforce LCC before BISAC.
A classification that resolves to a non-root LCC subject suppresses automatic BISAC
projection, including imported BISAC codes, inferred genres, and previously
materialized heading mappings. Supported sibling LCC consensus counts as LCC
for this decision even though it is stored as a `unified` subtree assignment.
Reviewed raw-heading mappings and DDC are also fallback evidence and do not
add navigation subjects when usable LCC exists. LCC codes that cannot resolve to navigation do not
block fallback. Original source headings and classification codes remain in
the record; explicit manual navigation assignments remain effective. This
shared rule applies to enrichment, catalog publication, and library reprojection.

The sibling fallback counts only LCC recorded directly on matching editions.
Titles use the existing classification title-compatibility function, including
whole-word title fragments when subtitles differ or are absent. Structural
qualifiers remain protected; fragment matches also check edition, revision and
translation markers. Consensus
does not introduce a stricter full-title requirement. A main-title-only edition
is checked explicitly, so long subtitles cannot exhaust the fragment limit.
Subject retrieval and voting use the same title-aware author checks. Repeated
short/full credits retain the most informative name; conflicting explicit full
names still cause abstention. Verified work links carry alternative and translated
titles into sibling voting, while each edition must still pass author and
volume/adaptation checks. Lookup reads the compatible siblings of identified works
before counting support, including editions under titles with or without an initial
"The". Only the original title candidates can establish an edition identity.
Colon and spaced-slash subtitles supply an explicit main-title lookup before
bounded fragments, so long descriptive titles cannot hide the main title.
Short titles use the same checks and support threshold, with no minimum title length.
Candidates are counted before year/publisher ranking, work/code
deduplication and display truncation; a truncated input search cannot vote.
Duplicate ISBN records vote once, while editions without ISBNs use their OL
edition IDs. Each full call number is mapped to taxonomy subjects before votes
propagate through their ancestors. Each edition has one vote, split equally
across its distinct deepest subjects. Equivalent codes and redundant parent
assignments do not multiply a vote. Each leaf's share also contributes to its
ancestors, allowing subtree support to combine related subjects. Unmapped numeric
codes consume a share without supporting a mapped subject; unclassified records
and inherited work unions never vote. A subject needs 20% of the total edition
vote budget, with no minimum number of supporting editions. An edition with
eight subjects contributes 1/8 to each before averaging across editions.
Retain the deepest qualifying concepts independently in each branch, omitting
redundant parents and top-level roots. If none qualifies, retain the subject with
the highest weighted support, or all subjects tied for that support, again
omitting redundant parents. With no mapped evidence, abstain. Exact ISBN
classifications and a uniquely identified publisher/year edition retain priority.
This agreement estimates support, not independent cataloging decisions.
Subjects use `metadata:edition-subtree-consensus` with the `unified` authority,
store stable `concept:<id>` codes, retain donor edition URLs, and include weighted percentages, donor counts, and
any highest-support fallback in their names. The API also exposes the weighted
support fraction and whether the fallback was used. Each supported subject carries
its `concept_id`; its `path` is display data. Older responses without an ID are
still resolved through their legacy path when materialized.
They never use the manual-assignment source or attach a donor ISBN to the book.

Policy `books_v31` uses concept IDs and canonical navigation ancestry for subject
corroboration, root support, and similarity grouping. Display labels and path
separators do not establish subject identity or ancestry. The runtime keeps its
existing navigation graph cache as ID edges and roots, materializing display paths
only for returned subjects. Projection policy version 153 recomputes local assignments
independently of prepared matcher format version 151. Existing caches labeled 151,
152, or 153 share that format and remain readable without rebuilding.

New answer builds include the separate declared-work lookup in `authority_title`.
Before activating this policy over an existing full answer database, populate it
from the original imported dump:

```sh
metadata-server index-work-classifications core-2026-07-31.sqlite answers-current.sqlite
```

This transaction replaces only the dedicated `openlibrary_work` index rows.
ISBN answers, edition candidates, and other authority records remain intact.
The command requires matching dump dates and a full database rather than an
overlay. It can run beside ordinary read-only WAL service connections. Future
answer builds use recipe 5 and read declared work codes before consulting siblings.

`servers/catalog/tools/rerun-title-author-subjects.py prepare` stages catalog
entries with title/author classification provenance. It removes borrowed codes in
every supported classification scheme
from the requests so previous enrichment cannot stop the new lookup. Its `apply`
command accepts complete corrected results through `books_v26` and changes only subjects in the
staged catalog; original records and responses are retained for review. It
checks request ISBNs against original EPUB identifiers or source metadata,
so ISBNs inferred by an earlier title match cannot establish an exact edition.
By default, entries without replacement classification evidence keep their
prior subjects for review. With `prepare --without-manual`, every entry carrying
any `manual:*` subject control is excluded, and identities and assignments are
exported to `manual-assignments.json` and `.csv`. In this explicit consensus
mode, old borrowed classifications that have no qualifying replacement are
withdrawn; original source subjects and stronger classification sources remain.
Use `--selection-from /path/to/previous/selection.json` to reevaluate the same
entries after a policy change, including entries whose subjects were withdrawn.
The staged result must be reviewed and published before changing the catalog.

Subject enrichment policy `books_v12` used the refreshed record-supported work
classifications and preserves LCC selections through later merges. It also
keeps looking past broad LCC subclasses
such as `B`, `DA` and `BF`. A completed classification must have supported
numeric coverage and resolve beyond the subclass's fallback taxonomy routes.
ISBN and title/author lookup can therefore refine source classifications while
retaining their original LCC codes and raw headings. Unsupported numbers and
numeric codes that still resolve only to a broad parent do not end lookup.
The client subject matcher version is 137: a verified numbered classification
takes precedence over automatic fallback navigation assignments in the same
LCC family, including general numbers that map only to the subclass parent.
Independent branches and explicit manual assignments are preserved.

Title lookup recognizes reviewed author aliases in
`shared/metadata-contract/data/reviewed-author-aliases.json` and explicit Latin
aliases attached to names in another script.
Author lookup keys ignore honorifics at any position and dated biographical
phrases such as `active 6th century`, `born ca. 1851` and `fl. 1695-1711`.
These rules preserve displayed credits, name initials and generational suffixes.
Full religious titles (`Saint`, `Father`, `Sister`, `Brother`, `Rabbi`, `Swami`)
and `Mahatma` are ignored wherever they occur. Abbreviated `St.` is handled
through reviewed aliases for Augustine of Hippo and Benedict of Nursia;
it remains part of surnames such as `St. John`. `Sr.` still means `Senior`.
`General` and `Baronet` are also ignored. Additional abbreviations
(`Gen.`, `Lt.`, `Maj.`, `Sgt.`, `Adm.`, `Brig.`, `Bt.`, `Bart.`, `Fr.`)
require a printed period so given names such as `Gen` and `Bart` remain intact.
Parenthesized contributor roles such as `(Translator)` and `(Editor)` are
excluded from lookup keys; other parenthesized names and aliases are retained.
Verified parenthetical initial expansions such as `Stanton H. (Stanton Henry)
King` produce a full-name lookup spelling, preserving the stated name as a
constraint. Short/full credits can omit given names only when a shared full
given name, surname and complete/main-title agreement support the match.
Conflicting stated names, initials and suffixes remain distinct; conflicting
donors are checked before classification voting and work deduplication.
Shortened-name matches recover classifications without attaching a donor ISBN.
A sole donor must supply its own edition classification; inherited work codes
need multiple matching edition records.
Conjoined personal credits inherit a shared surname (`Grace and Harold
Johnson`), and identified collective credits can omit the leading `the`
(`the Three Initiates`). These rules change lookup evidence, not displayed
credits, and use the existing title index.
Biographical labels are removed only when followed by a recognized date or
century expression. Title matching checks structural qualifiers in the full
title, including subtitles. Gutenberg identifiers in either
`/ebooks/NUMBER` or legacy `/NUMBER` form suppress edition-year ranking and
automatic print-ISBN attachment; the source's displayed year is preserved.
Editions sharing a work keep representatives of differing normalized code
sets, so the first indexed edition cannot hide other subject evidence.

Broad title/author searches without publisher or year corroborate numeric LCC
using distinct exact-edition records before collapsing candidates. Large
searches (at least ten classified editions) require two votes per subject route,
or a 90% dominant root/route with every secondary branch at most 10%. Sparse
searches preserve exact classifications and keep inherited routes only where
edition evidence supports them. Without mapped edition evidence, inherited
work codes spanning unrelated routes remain unresolved. This limits leaks from
translations, adaptations and erroneous cataloging; agreement remains a
heuristic rather than proof that a classification describes the local file.

Earlier policies added supported BISAC subjects from reviewed source headings
alongside numeric LCC. Policy `books_v27` supersedes that behavior: source-heading
and BISAC navigation is used only when usable LCC is absent. Only `dc:subject` and `catalog:*` headings with
no authority or LCSH authority qualify. Full headings must agree; history and
criticism qualifiers are not stripped. Reviewed literary criticism headings
map to criticism. Explicit final LCSH form subdivisions (`-- Fiction`,
`-- Juvenile fiction`, `-- Biography`, `-- Poetry`, `-- Humor`) can provide a
broad form category when no more specific reviewed mapping exists. Short-story
headings use the curated Short Stories category through the shared reviewed
rules below. Vague labels such as `Nonfiction` remain unresolved.
Christian apologetics additionally requires an explicit Christianity heading;
library-specific digital preservation and archive mappings require library
context. Original source headings remain.

Policy `books_v13` additionally uses the 24 approved exact mappings in
`shared/subject-projection/data/reviewed-source-headings.json`. This same table
is part of ordinary subject projection, including new imports and library
reprojection; it is not a catalog-only correction. Each complete raw heading
selects one existing concept ID. Destination paths in the rule file are optional
authoring annotations; current navigation paths come from the active taxonomy.
Only case and whitespace
are normalized: punctuation, dates and subdivisions remain significant.
Duplicate normalized inputs and IDs absent from the active navigation graph
remain unresolved. Competing subdivision mappings use taxonomy ancestry to
retain a single most-specific concept, leaving unrelated candidates unresolved.
Unreviewed headings and unsupported raw LCC codes are not
given guessed destinations, and no taxonomy categories are added.
Enrichment retains the original headings and returns derived `unified`
subjects with `metadata:source-heading:` provenance and `concept:<id>` codes.
The matcher validates these derived subjects against the approved heading and ID,
and continues to accept older path-based codes when they resolve to that ID.
The catalog's read-only `catalog DIR recover-source-headings` command uses the
same subject constructor for backfills. It reads one JSON subject array per
line and returns the approved derived subjects, without editing the catalog
or contacting metadata providers.

Policy `books_v17` and matcher version 142 add 44 reviewed English exact
headings from the catalog, for 91 scoped rules in the shared table. Each new
rule requires its reviewed source (`dc:subject` or `catalog:unglue.it`);
derived subjects must retain that original source to qualify. The additions
include literary genres, forms and clear topical headings. No Swedish aliases
or new taxonomy categories are added. Explicit LCC classifications continue
to suppress raw-heading inference. When a reviewed heading names an ancestor
of another supported assignment, only the more specific route is projected;
explicit manual assignments remain visible.

Policy `books_v18` and matcher version 143 also use the reviewed broad LCC
mappings in `20261014_route_broad_catalog_lcc.sql`: `D` selects History, `CT`
selects Biography, and `PZ` selects Literature. The broad `PZ1..PZ90` range
also selects Literature instead of the excluded Genres umbrella. Bare `PJ`
and `PL` select Language without guessing a specific language. Existing
numbered selectors and narrower ranges are retained. These are taxonomy
selector changes; original catalog subject declarations are preserved.

For repeatable evaluation against the existing answers, without deploying or
calling artwork/recording providers:

```sh
BOKHEIM_METADATA_DEMAND=/path/to/scratch/demand.sqlite \
metadata-server evaluate-subjects answers-current.sqlite request.json > results.json
```

The request uses the `/v3/books` JSON shape. Every book must have
`needs_cover: false` and `recording: null`; the command processes internal
batches of eight through the same enrichment implementation. Answers open
read-only; use a separate scratch demand database. Measure projected subject
placement rather than `found`, which can also mean other metadata was found.

The server records valid ISBNs without LCC in `demand.sqlite`. Builds include
these ISBNs even when they are absent from the imported dumps.
Descriptions, author identity, ambiguity, provenance and per-book merge behavior
stay deterministic for the same file facts and answer file. Audible discovery
uses live requests with a six-hour bounded memory response cache. Valid Audnexus
book responses and verified accurate chapter timelines persist permanently in
the `audnexus_response` table in `BOKHEIM_METADATA_DEMAND` (normally `demand.sqlite`).
The full URL identifies a response, including its ASIN, region and endpoint.
There is no expiry, row limit, byte budget or eviction for these disk records.
Cache hits bypass network pacing and recheck the recording identity and chapter
validation before use. Invalid book responses, inaccurate or incomplete chapter
timelines, and failed requests are not saved.
Only requested responses are loaded into memory; the cache survives service
restarts and answer snapshot upgrades. Include this database in backups.
SQLite operations run on blocking workers. Artwork lookup also reuses saved
Audnexus book metadata. No new environment variable or external database is needed.

Completed supported recording decisions also persist permanently in
`recording_match` in the same database. The key includes the full recording
request (title, credits, ASINs, duration and chapter evidence), provider origins
and `RECORDING_MATCH_POLICY`. The full serialized input is checked alongside its
hash. Bump that policy when recording selection, parsing or chapter-plan rules
change. A changed recording never reuses a different input's decision. Only
supported decisions are saved; unavailable, ambiguous and unmatched requests
remain eligible for discovery. Cached decisions return the original response
without provider discovery or an outbound lookup slot. Concurrent identical
lookups coalesce and recheck the cache after waiting.

Uncached audiobook requests wait for one of two shared discovery slots rather
than immediately failing because a batch contains more than two audiobooks.
Discovery has a 45-second deadline after acquiring a slot; queueing does not
consume that provider budget. Network requests retain their eight-second
timeout and are paced independently per provider origin. Each recording's chapter
lookup is attempted once per discovery request across all fallback phases;
failed chapter lookups remain retryable on later requests. The client still
receives a single complete enrichment response.

The client submits enrichment requests concurrently without a client-side
concurrency cap. Ordinary books retain the eight-book API payload size;
audiobooks are submitted individually so recording discovery cannot hold up
other books in the same response. Completed requests are persisted immediately,
and newly discovered files join the running queue while earlier requests are
in flight. The metadata connection has an eight-second connect timeout; the
server owns provider deadlines so time spent queueing does not cancel valid
lookups. Missing-cover recovery also runs alongside metadata enrichment.

For older clients and catalog tools, the server can resolve cover artwork:
ISBN candidates are tried against
Open Library with a shared one-request-per-second limit, and selected Audible
artwork is fetched from Audible. Confirmed bytes persist in `demand.sqlite` and
`/v3/books` returns one reference fetched from `/v3/covers/{id}`. The policy is
`books_v37`. Successful client answers are immutable.
Library clients send `needs_cover: false` for every book and look up/download
missing artwork directly on the device: Open Library by ISBN or title/author,
and Audible by ASIN when recording artwork is needed. Bibliographic and recording enrichment still use `/v3/books`. Existing embedded or downloaded
art is retained. Direct browser image downloads remain subject to the provider's
CORS headers.
Older clients and catalog tools can still request server-side covers. Omitted
`needs_cover` flags default to true for compatibility.
Cached artwork presence is checked without loading image bytes. Confirmed cover
misses persist for 24 hours; transient provider failures are not negative-cached.
Concurrent requests for the same cover coalesce. Download starts retain the shared
one-request-per-second pacing, while up to four bodies can download concurrently.
The pacing lock is released before the HTTP request. A shared HTTP client reuses
connections, limits individual requests to eight seconds, and each provider cover
lookup has a 12-second budget including queue time and alternative ISBNs. Cover
SQLite reads and writes run outside the asynchronous worker threads. Current
library clients continue retrieving artwork separately from metadata.

Routes: `/v3/books`, `/v3/covers/{id}`, `/health`,
`/health/descriptions`, and `/internal/operations`. Operations reports the books
endpoint counters and processing totals; it has no duplicate service counters.

## Retired metadata crawler

The LC/LibraryThing crawler and its `crawl` command have been removed. New builds
omit the `wanted` queue and its indexes. Existing full and overlay answer files
remain readable; their legacy queue tables are ignored.

To retire an existing installation and update its deployment helper:

```sh
sudo bash servers/metadata/deploy/remove-crawler.sh
```

The script disables and removes `bokheim-metadata-crawl.service` and its release
template. It leaves the metadata API running and preserves
`/var/lib/bokheim-metadata-crawl/provider.sqlite` and existing credentials.
Archived results can still be passed as the optional provider input to `build`
or `reclassify`. The provisioner also performs this cleanup, and deployments
install and start only `bokheim-metadata.service`.

## Verification

```sh
cargo test -p metadata-server -p metadata-contract
# Python extraction tests also require indexed_bzip2 and the system zstd command.
python3 -m unittest discover -s servers/metadata -p 'test_*.py'
```

Resolution fixtures build answer files, read them through the server, and compare
with the original resolver as a test-only oracle. HTTP and Audible tests use
local mock servers. Additional tests cover archived provider merges, description
sharing, demand updates, read-only serving after source removal and snapshot
replacement.
Before production activation, compare `/v3/books` over representative production
file facts against the previous release and record full-build timing and size.
Production data is not part of the repository.
