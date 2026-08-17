# Open Library metadata server

This standalone server imports the Open Library editions, works, and authors
dumps into an immutable SQLite snapshot. It returns Library of Congress
Classification (LCC), Dewey Decimal Classification (DDC), and stable author
identifiers with edition/work provenance.

Snapshot schema v8 retains edition and work titles plus publisher and
publication year for the same ISBN-bearing editions. `POST
/v2/edition-identities` can therefore recover an ISBN from bibliographic
metadata without importing unrelated editions. Edition and work titles are
both considered, but an ISBN is selected only from an edition whose normalized
author and every supplied edition discriminator agree. Ambiguous responses
contain a bounded candidate list for local manual review.

Schema v8 also aggregates the public Open Library ratings and reading-log
dumps at work level. Enrichment responses expose only aggregate counts and
rating sums; patron-level activity rows are not retained. Schema v8 deliberately
does not contain resolved Wikidata entities. Schema v9 adds those entities and
their retained geographic relationships.

## Build a snapshot

The importer streams the gzip files and does not unpack them on disk:

```sh
cargo run --release -p metadata-server -- import \
  /home/johan/data/openlibrary/ol_dump_editions_latest.txt.gz \
  /home/johan/data/openlibrary/ol_dump_works_latest.txt.gz \
  /home/johan/data/openlibrary/ol_dump_authors_latest.txt.gz \
  /home/johan/data/openlibrary/classifications.sqlite
```

The output path must not already exist. The importer builds an adjacent
`.building` file, runs SQLite's integrity check, and renames it only after a
successful import. It retains only editions with a valid ISBN and the works and
authors reachable from those editions; unrelated Open Library records cannot be
queried by this service and are not stored. An optional final numeric argument
limits records from each dump and is intended only for sampling.

Use `import-popularity` with the editions, works, authors, ratings, reading-log,
and output paths to produce the production schema-v8 snapshot. The plain
`import` command remains useful for development snapshots without popularity.
To avoid rescanning Open Library, `add-popularity SOURCE.sqlite RATINGS.txt.gz
READING-LOG.txt.gz OUTPUT.sqlite` copies an immutable schema-v7/v8 bibliography
snapshot and streams only the aggregate activity dumps into the new output. It
also upgrades a completed legacy rich snapshot to schema v9 while preserving
its resolved entities.

### Rich place and person snapshot

The rich importer additionally consumes the Wikidata JSON dump:

```sh
cargo run --release -p metadata-server -- import-rich \
  ol_dump_editions_DATE.txt.gz \
  ol_dump_works_DATE.txt.gz \
  ol_dump_authors_DATE.txt.gz \
  wikidata-all.json.bz2 \
  ol_dump_ratings_DATE.txt.gz \
  ol_dump_reading-log_DATE.txt.gz \
  metadata-DATE.sqlite
```

`build-rich-snapshot.sh DATA_DIRECTORY OUTPUT.sqlite [MAX_RECORDS]` pins the
current dump URLs in `rich-snapshot.sources`, downloads every file with HTTP
range resumption, and starts the importer. Use a new data directory to select a
new monthly snapshot; an interrupted run continues using the pinned inputs.

The adjacent `.building` database is a durable checkpoint. Open Library phases
commit every 250,000 records; Wikidata commits every 20,000 and parses
5,000-record chunks. A bounded pipeline buffers compressed input, decompresses
ahead of the writer, and parses and filters records across the available CPU
cores. Wikidata's large typed records are immediately reduced to compact
retained rows before reaching SQLite. Dump rows first enter unindexed
append-only staging tables; each completed phase then sorts and deduplicates
them into the query schema in one separately checkpointed transaction. The
importer uses a 1 GiB SQLite page cache to reduce repeated B-tree writes.

Re-running the identical command validates each dump's canonical path, byte
size, modification time, record limit, and import mode, replays the compressed
stream to its last checkpoint, and resumes. Completed staging, resolution,
pruning, compaction, and integrity phases are also checkpointed. Do not replace
or touch dump files while a build is unfinished. A rich build produces schema
v9; its Open Library and popularity subset is schema v8. Because the popularity
aggregates add two pinned inputs, unfinished
older builds must be restarted from the source dumps.

Wikidata is decoded into typed records containing only labels, aliases, and the
selected claims used by the resolver. Place candidates must be unique and must
either have strong direct geographic evidence or belong through `P279` to a
retained geographic root. The finalized database contains only resolved
book-place/person entities and the `P131`/`P17`/`P30` ancestor closure. Build
coverage and retained graph counts are available in `metadata_metric`.

Wikidata/schema v9 is not part of the default production activation path. The rich build
may run to completion and is integrity-checked, but its activation supervisor
stops after validation unless `BOKHEIM_ACTIVATE_WIKIDATA_SNAPSHOT=true` is set
explicitly. The Open Library-only snapshot remains the production default.

Set `BOKHEIM_WIKIDATA_DECOMPRESSOR` to an `lbzip2`-compatible executable to
decode the bzip2 dump with two worker threads. The importer owns and validates
the child process and falls back to its built-in decoder when the variable is
unset.

`index-wikidata.py build DUMP.bz2 INDEX.json` creates a dump-specific bzip2
block map plus an entity-count-to-decoded-byte map at every 20,000-record
checkpoint. With `BOKHEIM_WIKIDATA_SEEKER` pointing to that executable and
`BOKHEIM_WIKIDATA_SEEK_INDEX` pointing to the completed index, a resumed scan
seeks directly to its committed record instead of replaying the compressed
stream. The helper validates the canonical path, size, modification time, and
edge checksum before emitting data. Missing configuration falls back to normal
sequential replay; a configured but invalid index fails closed.

The full Wikidata JSON dump is currently roughly 100 GB compressed. The helper
therefore requires 300 GB free by default to accommodate downloads, temporary
indexes, WAL, and `VACUUM`; override `BOKHEIM_METADATA_MIN_FREE_BYTES` only when
the filesystem has been sized separately.

## Run

```sh
BOKHEIM_METADATA_DATABASE=/home/johan/data/openlibrary/classifications.sqlite \
  target/release/metadata-server
```

Configuration:

- `BOKHEIM_METADATA_DATABASE` is required.
- `BOKHEIM_METADATA_BIND` defaults to `127.0.0.1:8091`.
- Binding plain HTTP to a non-loopback address additionally requires
  `BOKHEIM_METADATA_ALLOW_PUBLIC_HTTP=true`. Prefer a TLS reverse proxy.

## Endpoints

The ISBN lookup endpoints accept 1–256 ISBN-10 or ISBN-13 values:

- `GET /health` returns `ok` for service supervision.
- `GET /internal/operations` returns aggregate snapshot coverage, process-lifetime
  request/error/latency counters, and importer metrics for the authenticated
  operations dashboard. It contains no queried ISBNs or book metadata.
- `POST /v2/classifications` returns LCC/DDC metadata plus aggregate
  work popularity without loading author or entity metadata.
- `POST /v2/enrichment` returns classifications, ordered Open Library author
  identities, resolved places and people, and aggregate work popularity.

JSON is supported for diagnostics:

```sh
curl --fail \
  --header 'Content-Type: application/json' \
  --data '{"isbns":["0821338277","9780821338278"]}' \
  http://127.0.0.1:8091/v2/classifications
```

Production clients should use the independent, versioned binary contract from
the `metadata-contract` crate:

```text
Content-Type: application/vnd.bokheim.metadata+protobuf; version=2
```

ISBN-10 and ISBN-13 representations are normalized to the same ISBN-13 key.
One input can return multiple matches when Open Library maps it to multiple
works. Classifications state whether they came from the exact edition or its
work; authors likewise state edition/work provenance. The server never silently
resolves catalog ambiguity. Library clients only apply an author identity when
the ISBN resolves to exactly one work and the scanned credit matches exactly
one returned author after name normalization.

The SQLite database is opened read-only. Monthly refreshes should build a new,
dated snapshot and switch the configured path or symlink before restarting the
service.

## Production installation

Install the release binary and unit only after a complete snapshot has passed
the importer's integrity check:

```sh
cargo build --release -p metadata-server
sudo install -o root -g root -m 0755 target/release/metadata-server /usr/local/bin/metadata-server
sudo install -o root -g root -m 0644 servers/metadata/deploy/bokheim-metadata.service /etc/systemd/system/bokheim-metadata.service
ln -sfn /home/johan/data/openlibrary/metadata-YYYY-MM-DD.sqlite \
  /home/johan/data/openlibrary/current.sqlite.next
mv -Tf /home/johan/data/openlibrary/current.sqlite.next \
  /home/johan/data/openlibrary/current.sqlite
sudo systemctl daemon-reload
sudo systemctl enable --now bokheim-metadata.service
```

Production Caddy exposes only the bounded client endpoints at
`https://meta.bokheim.se` and proxies them directly to this loopback service.
Desktop clients therefore remain independent of catalogue availability. The
catalogue service may still use `BOKHEIM_METADATA_URL=http://127.0.0.1:8091`
for its own internal metadata needs.

### Passwordless deployments

Provision the deployment user once from a trusted checkout:

```sh
sudo servers/metadata/deploy/provision-metadata-catalog-deploy.sh johan
```

The provisioner installs a root-owned deployment program and grants only that
exact program passwordless elevation. Routine activations can then run
unattended with:

```sh
sudo -n /usr/local/sbin/bokheim-deploy-metadata-catalog
```

Do not make systemd units or deployment programs writable by the deployment
user. Changes to those privileged templates require rerunning the one-time
provisioner; staged binaries and data continue to use the passwordless
installer.
