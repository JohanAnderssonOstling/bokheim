# Bokheim taxonomy server

Read-only, unauthenticated HTTP access to the curated Bokheim unified taxonomy.
The service never imports raw LCC, DDC, or BISAC data: its only served state is
the reviewed `unified-taxonomy-v2.sqlite3` snapshot supplied at startup.

## Run

```sh
BOKHEIM_TAXONOMY_DATABASE=client/library-model/data/unified-taxonomy-v2.sqlite3 \
  cargo run -p taxonomy-server
```

The default listener is `127.0.0.1:8093`. Set `BOKHEIM_TAXONOMY_BIND` to change
it. A non-loopback plain-HTTP listener additionally requires
`BOKHEIM_TAXONOMY_ALLOW_PUBLIC_HTTP=true`; production should normally keep the
listener on loopback and terminate HTTPS in a reverse proxy.

No route accepts or requires credentials. Browser clients may access the public
API through permissive CORS.

## API

- `GET /health`
- `GET /v1/taxonomy` — release identifier and snapshot counts
- `GET /v1/roots` — initial visible roots
- `POST /v1/resolve` — batch resolution for all locally unsupported codes
- `GET /v1/resolve?system=lcc&code=DC256` — single-code diagnostic lookup
- `GET /v1/concepts/{concept_id}` — one concept and its ancestor closure
- `GET /v1/concepts/{concept_id}/children` — immediate children and required ancestors
- `GET /v1/snapshot` — complete immutable curated SQLite snapshot

Slices are ordered with parents before descendants so a client can transactionally
merge them into its local taxonomy. `ETag` identifies the exact taxonomy release;
the snapshot endpoint supports `If-None-Match` and long-lived immutable caching.

The production request is one bounded JSON batch for normal assignment operations:

```json
{"codes":[{"system":"lcc","code":"HF5601"},{"system":"ddc","code":"515"}]}
```

The response contains one result mapping per unique code and a single
deduplicated ancestor-closed concept union. Codes already supported by the
client must be filtered locally and are not sent.
Operations containing more than 512 unique missing codes, or whose encoded JSON
would exceed 64 KiB, are split into consecutive bounded requests. The client
combines them only when every response has the same taxonomy release ID.

The native client records classifications in `book_subject_code` before it
attempts unified assignment. Local-file imports, catalog imports, and
multi-book subject enrichment then collect the codes absent from the installed
taxonomy and make one request for the operation. Returned concepts are merged
transactionally into the device-local SQLite taxonomy, the in-process matcher
is replaced, and every open library recalculates its assignments. A network
failure never rolls back book metadata; the recorded codes remain available
for a later hydration attempt.

The browser client follows the same policy in its worker: exact local coverage
is checked in WASM, misses are batched, and returned concepts plus positive or
negative resolutions are persisted in browser SQLite. The overlay is restored
into WASM on demand. Rich metadata and remotely synchronized metadata both
trigger hydration, while taxonomy downtime never blocks metadata storage or
core library synchronization. A new server release invalidates old resolution
rows so previously negative codes can be checked again.
