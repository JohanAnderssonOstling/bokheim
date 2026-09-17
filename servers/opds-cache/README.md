# Bokheim OPDS cache server

This standalone public service crawls the curated anonymous catalogue registry. Most
sources are OPDS feeds; provider adapters may expose other public ebook catalogues
through the same neutral contract. It
does not share a process, database, credentials, or authentication code with
the synchronization server.

Public unauthenticated endpoints:

- `GET /health`
- `GET /api/opds/catalogs`
- `GET /api/catalog/sources`
- `POST /api/opds/sources/{source-id}/page`
- `POST /api/opds/sources/{source-id}/search`
- `POST /api/opds/subjects`
- `POST /api/opds/subject-books`
- `POST /api/opds/sources/{source-id}/acquire`
- `GET /api/opds/sources/{source-id}/cover?image_url={url}`
- `GET /api/web/catalogs`
- `GET /api/web/stats`
- `POST /api/web/sources/{source-id}/page`
- `POST /api/web/sources/{source-id}/search`
- `POST /api/web/subjects`
- `POST /api/web/sources/{source-id}/acquire`

Structured requests and responses use
`application/vnd.bokheim+protobuf; version=18` with identity encoding. OPDS
upstreams remain XML or JSON, and covers are canonical JPEG thumbnails. The
`/api/web` façade uses the provider-neutral contract's JSON representation so
ordinary browsers do not need a second implementation of the protobuf codec.
`/api/catalog/sources` is the authoritative provider-neutral native source
registry. It returns display metadata plus opaque root locators; provider URL
validation and OPDS endpoint configuration remain private to this service.

The service stores complete catalog generations in SQLite. On each fixed
24-hour cycle, distinct origins refresh concurrently while catalogues sharing
an origin remain sequential. Each source crawls from its root with at most four
overlapping page requests and follows the current same-origin catalogue graph.
SQLite writes remain serialized and batched. Conditional upstream
requests reuse unchanged pages, while a successful crawl activates as one
transaction; failures retain the previous generation. Page and search HTTP
requests read SQLite only and never wait on an upstream catalog.

Provider pagination is consumed during ingestion. Each discovered catalogue
page follows its same-origin continuation chain, deduplicates books in
provider order, and stages one complete logical page with no public pagination
links. Paginated logical pages are fully re-fetched on replacement crawls
because an HTTP validator for the first provider page cannot validate the
whole compacted collection.

The server store owns its complete SQLite pool and single version-4 schema, including canonical
catalogue rows, crawl state, queues, operational inventory, and serving
indexes. An incompatible cache is replaced and repopulated by the crawler at
startup; no client or shared crate initializes this database.
`GET /api/web/stats` reports process uptime, registry revision, metadata-service
configuration, active page/book/importable EPUB counts, crawl state,
extraction queues, normalized failure categories, retry state, and at most
three redacted host/book examples per category. The same bounded payload
is available to the operations collector from the separate loopback-only
listener on `127.0.0.1:8092` at `GET /internal/operations`. Raw errors and
acquisition URLs never leave the protected catalogue cache.

Search matches normalized contiguous substrings in title, author, and table of
contents, returning each as a distinct ordered section. It is always evaluated
against the active server snapshot; upstream OPDS search templates are neither
stored nor routed. Requests include the
client's supported languages. The response protocol identifies books by
sequence so it sends payloads only for books the client does not already
have at the current epoch. The authoritative page/search queries and native
delta assembly live in this server's `store` module. Clients persist exact
language-keyed layouts for offline display; no shared crate owns either SQLite
schema.

Subject browsing treats every crawled provider page as part of one logical
collection. The server is authoritative for subject search, source/language/
format filters, sorting, facets, totals, and result ranges. Native clients first
request the ordered IDs for the selected range, reuse immutable book
cards already cached for the returned epoch, and fetch only missing cards.
The browser JSON endpoint receives full cards directly; neither transport sends
a browse-mode flag. Provider pagination links remain crawler state and are never
exposed as subject locators.

The crawler also reads descriptions, covers, subjects, and ISBNs from bounded
temporary EPUB, MOBI, and PDF files where each format supports them. EPUB
origins that honor HTTP byte ranges transfer only the ZIP directory,
`container.xml`, package metadata, navigation document, and declared cover into
a sparse temporary file. Non-range origins, ZIP64 archives, and unusual cover
layouts automatically fall back to the bounded full-file path. Range capability
is remembered per origin so an incompatible host is not probed for every book. Raw
subject evidence is stored separately from deterministic
canonical assignments, using the shared `subject-projection` rules as local
libraries. When a supported book has an ISBN but no embedded subjects,
`BOKHEIM_METADATA_URL` enables batched LCC enrichment from the standalone
Open Library snapshot service. Subject browsing reads that canonical projection
without sending titles or other library contents to this public service.

Book extraction is drained by one persistent serial worker. It downloads
one book to a bounded temporary file, extracts its metadata and artwork,
commits the result, and then moves to the next source. Temporary files are
deleted after extraction. A restart
changes interrupted jobs back to pending and resumes them from the active
catalogue generations without resetting successful payloads or retrying failed
downloads in a tight loop. Completed crawls can still schedule a bounded retry
of failed jobs. Each pass visits every source before taking another book
from the same source. Retryable
failures use 15-minute, one-hour, six-hour, and then daily backoff. Permanent
format, access, missing-resource, and safety-limit failures remain terminal
until their catalogue record or enrichment version changes.

Crawls persist each HTTP(S) cover URL together with its catalogue source,
edition ID, and work ID in a durable queue; they never download the image. The
independent cover-download worker claims those rows and creates 408 by 612 JPEG
thumbnails, recording attempts, success, and failure against the same book.
Interrupted jobs return to pending after a restart, while an unchanged URL
reuses its existing image. Native clients verify
and download acquisitions directly. Browser clients use the acquisition
endpoint because third-party book hosts commonly reject cross-origin browser
requests. The endpoint re-fetches the registered catalogue page, verifies the
book identity, exact URL, free acquisition kind, and format, downloads
into a bounded temporary file, and streams it without adding it to the cache.
At most two interactive browser acquisitions run concurrently, independently
of the single background extraction download.

The registry contains only curated feeds. User-defined catalogs are not proxied,
and the acquisition endpoint accepts a structured catalogue action rather than
an arbitrary URL, so the service is not an open HTTP proxy. A configured
registry file is checked when the registry is requested; only a valid higher
revision is activated.

Catalogue crawling, cover downloads, book extraction, and catalogue
metadata backfill are independent workers. Every server start places all four
in the paused state;
the authenticated administrator dashboard can then resume, pause, or wake each
worker separately through the loopback operations API. Pauses take effect
between sources, books, or enrichment batches rather than interrupting
an in-flight unit of work.

Catalogue page traffic is throttled independently per upstream origin. Each
origin starts with four concurrent requests and a small request-start spacing,
grows cautiously after sustained low-latency success, and backs off on latency
spikes, transient HTTP/network failures, or rate limiting. `Retry-After` is
honored for both delta-seconds and HTTP-date values; rate-limited origins fall
back to one request at a time while other origins continue normally. The
current limit, pacing, and wait state are visible in the administrator source
inventory.

All catalogue, search-description, cover, and acquisition requests use the
server's public-only outbound policy. Every hostname is resolved before the
request, all returned addresses must be publicly routed, and the HTTP client is
pinned to those validated addresses. Redirects are followed manually and every
destination is resolved and validated again. Loopback, private, shared,
link-local, multicast, documentation, transition, and reserved IPv4/IPv6 ranges
are rejected. The separately configured `BOKHEIM_METADATA_URL` is trusted
deployment configuration and may intentionally point at the local metadata
service.

## Local use

```sh
cargo run -p catalog-server
curl --header 'Accept: application/vnd.bokheim+protobuf; version=18' \
  --output /dev/null http://127.0.0.1:8090/api/opds/catalogs
```

Configuration:

- `BOKHEIM_CATALOG_BIND`, default `127.0.0.1:8090`
- `BOKHEIM_CATALOG_FILE`, optional external registry path
- `BOKHEIM_CATALOG_CACHE_FILE`, optional SQLite cache path
- `BOKHEIM_CATALOG_COVER_CACHE_DIR`, optional JPEG cache directory
- `BOKHEIM_METADATA_URL`, optional base URL of the Open Library metadata
  service (for example `http://127.0.0.1:8091`)
- `BOKHEIM_CATALOG_ALLOW_PUBLIC_HTTP=true`, required for a non-loopback bind

Validate a candidate registry without starting the service:

```sh
cargo run -p catalog-server -- --check-catalog servers/opds-cache/opds.json
```

Every published registry change must increment `revision`. See
[deploy/README.md](deploy/README.md) for the independent systemd and Caddy
deployment.
