# Metadata answers

`metadata-server` imports dumps, builds immutable SQLite answers, serves
`POST /v3/books`, and crawls LC/LibraryThing evidence for the next build.
The server reads one answer file and writes a small demand database. Its only
external provider is Audible; it neither reads LC/LT caches nor loads API keys.

```text
OL / LC MARC / Wikidata + provider.sqlite + demand.sqlite
    -> build -> answers-DATE.sqlite -> serve -> /v3/books
                     wanted -> crawl -> provider.sqlite -> next build
```

## Import and build

```sh
cargo build --release -p metadata-server
metadata-server import EDITIONS.txt.gz WORKS.txt.gz AUTHORS.txt.gz current.sqlite
metadata-server import descriptions WORKS.txt.gz descriptions.sqlite 2026-09-01
# Optional: the existing Python extraction/import tools produce LC MARC and
# Wikidata supplements. Fold Wikidata into a COPY of the imported snapshot:
cp current.sqlite authorities.sqlite
metadata-server import authorities current.sqlite wikidata.sqlite authorities.sqlite

BOKHEIM_METADATA_DESCRIPTION_DATABASE=descriptions.sqlite \
BOKHEIM_METADATA_DEMAND=/var/lib/bokheim-metadata/demand.sqlite \
BOKHEIM_LIBRARYTHING_RATINGS_DUMP=ol_dump_ratings.txt.gz \
metadata-server build authorities.sqlite /var/lib/bokheim-metadata-crawl/provider.sqlite answers-2026-09-30.sqlite
```

Optional environment variables may be omitted. Pass `-` for the provider input
on the first build. The builder accepts Open Library import schema 9 only and
validates the sidecar dump date. Inputs must be complete, immutable dump files;
provider results are read in one SQLite transaction while the crawler continues.

Every ISBN from the dumps, providers or demand is resolved without network I/O.
Provider results fill missing classification schemes while preserving existing LCC.
Answers use JSON BLOBs with explicit references to shared work descriptions;
edition keys and authority candidates preserve the existing scoring rules.
The builder streams ISBNs in batches and publishes OUT only after all tables,
manifest and integrity checks succeed. An existing OUT is never overwritten.
It prints ISBN count, answer JSON bytes, SQLite bytes, elapsed time and throughput
as JSON. Measure those on production before deciding whether to change encoding
or replace hot resolution rules with bulk SQL.

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

The server records valid ISBNs without LCC in `demand.sqlite`. The next builder
ranks demand ahead of other wanted ISBNs, then ranks the remainder by OL ratings.
Descriptions, author identity, ambiguity, provenance and per-book merge behavior
stay deterministic for the same file facts and answer file. Audible data remains
live. Cover artwork is resolved on the server: ISBN candidates are tried against
Open Library with a shared one-request-per-second limit, and selected Audible
artwork is fetched from Audible. Confirmed bytes persist in `demand.sqlite` and
`/v3/books` returns one reference fetched from `/v3/covers/{id}`. The policy is `books_v3`, prompting clients to refresh saved enrichment.
Clients otherwise re-ask after their 30-day answer lifetime.

Routes: `/v3/books`, `/v3/covers/{id}`, `/health`,
`/health/descriptions`, and `/internal/operations`. Operations reports the books
endpoint counters and processing totals; it has no duplicate service counters.

## Crawl

```sh
BOKHEIM_LIBRARYTHING_KEY_FILE=/path/to/key \
metadata-server crawl answers-current.sqlite provider.sqlite
# Process one bounded pass, useful for operational checks:
metadata-server crawl answers-current.sqlite provider.sqlite --once
```

The crawler reads only the manifest and ranked `wanted` table. It loads that
queue into a disk-backed temporary table when the snapshot changes, so a running
crawler sees newly activated builds without restarting. It queries LC first,
in batches of up to 50 ISBNs with persistent six-second request spacing. Only
exact MARC 020$a ISBN matches are accepted; related/cancelled 020$z ISBNs require
independent dump evidence and are not accepted by the crawler.

LibraryThing requires a completed LC result, including a miss or ambiguity, and
no LCC on that ISBN or another wanted ISBN of the same work. The same work gate
also avoids further LC work after a crawler result resolves it. Completed
positive, negative and ambiguous results persist. Failures remain retryable
with an hour backoff. LT reserves requests before sending, including failed
requests, with two-second spacing and a durable 1,000-request UTC daily quota.
No API key means an LC-only crawl.

The next build folds normalized results in; a running server never sees mutable
provider data. Crawl freshness therefore includes the next build and the client
refresh interval. An incremental build or shorter pending lifetime would be a
separate change.

`deploy/bokheim-metadata.service` runs `serve` with no provider environment file.
`deploy/bokheim-metadata-crawl.service` runs the independent crawler and reads
`~/.config/bokheim/metadata-crawl.env`, containing only the LT key-file setting.
Its `StateDirectory` owns provider.sqlite; the server's separate state directory
owns demand.sqlite. Re-run the provisioner to install both release templates.

For the one-time cache migration, use `tools/migrate-provider-caches.py` against
copies of the former LC/LT databases. It copies completed results and LT quota
state into a new provider database; the runtime has no cache-upgrade code.

## Verification

```sh
cargo test -p metadata-server -p metadata-contract
# Python extraction tests also require indexed_bzip2 and the system zstd command.
python3 -m unittest discover -s servers/metadata -p 'test_*.py'
```

Resolution fixtures build answer files, read them through the server, and compare
with the original resolver as a test-only oracle. HTTP, Audible and LC pagination
tests use local mock servers. Additional tests cover provider merges, description
sharing, demand and ratings ranking, read-only serving after source removal,
snapshot replacement, crawler ordering/backoff/work gates and durable quota.
Before production activation, compare `/v3/books` over representative production
file facts against the previous release and record full-build timing and size.
Production data is not part of the repository.
