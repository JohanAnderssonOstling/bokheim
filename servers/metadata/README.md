# Open Library classification server

This standalone server imports the Open Library editions and works dumps into
an immutable SQLite snapshot and exposes one batched lookup endpoint. It returns
Library of Congress Classification (LCC) and Dewey Decimal Classification (DDC)
notations with edition/work provenance.

## Build a snapshot

The importer streams the gzip files and does not unpack them on disk:

```sh
cargo run --release -p metadata-server -- import \
  /home/johan/data/openlibrary/ol_dump_editions_latest.txt.gz \
  /home/johan/data/openlibrary/ol_dump_works_latest.txt.gz \
  /home/johan/data/openlibrary/classifications.sqlite
```

The output path must not already exist. The importer builds an adjacent
`.building` file, runs SQLite's integrity check, and renames it only after a
successful import. An optional final numeric argument limits records from each
dump and is intended only for sampling.

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

## Endpoint

The only endpoint is `POST /v1/classifications`. A request contains 1–256
ISBN-10 or ISBN-13 values. JSON is supported for diagnostics:

```sh
curl --fail \
  --header 'Content-Type: application/json' \
  --data '{"isbns":["0821338277","9780821338278"]}' \
  http://127.0.0.1:8091/v1/classifications
```

Production clients should use the independent, versioned binary contract from
the `metadata-contract` crate:

```text
Content-Type: application/vnd.bokheim.metadata+bincode; version=1
```

ISBN-10 and ISBN-13 representations are normalized to the same ISBN-13 key.
One input can return multiple matches when Open Library maps it to multiple
works. Classifications state whether they came from the exact edition or its
work; the server never silently resolves catalog ambiguity.

The SQLite database is opened read-only. Monthly refreshes should build a new,
dated snapshot and switch the configured path or symlink before restarting the
service.

