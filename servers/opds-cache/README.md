# Bokheim OPDS cache server

This standalone public service crawls the curated anonymous catalogue registry. Most
sources are OPDS feeds; provider adapters may expose other public ebook catalogues
through the same neutral contract. It
does not share a process, database, credentials, or authentication code with
the synchronization server.

Public unauthenticated endpoints:

- `GET /health`
- `GET /api/opds/catalogs`
- `POST /api/opds/sources/{source-id}/page`
- `POST /api/opds/sources/{source-id}/search`
- `POST /api/opds/recommendations`
- `GET /api/opds/sources/{source-id}/cover?image_url={url}`
- `GET /api/web/catalogs`
- `GET /api/web/stats`
- `POST /api/web/sources/{source-id}/page`
- `POST /api/web/sources/{source-id}/search`
- `POST /api/web/recommendations`
- `POST /api/web/sources/{source-id}/acquire`

Structured requests and responses use
`application/vnd.bokheim+protobuf; version=13` with identity encoding. OPDS
upstreams remain XML or JSON, and covers are canonical JPEG thumbnails. The
`/api/web` façade uses the provider-neutral contract's JSON representation so
ordinary browsers do not need a second implementation of the protobuf codec.

The service stores complete catalog generations in SQLite. One task per catalog
crawls from its root and follows the current same-origin catalog graph. Catalogs
refresh concurrently on a fixed 24-hour cycle. Conditional upstream
requests reuse unchanged pages, while a successful crawl activates as one
transaction; failures retain the previous generation. Page and search HTTP
requests read SQLite only and never wait on an upstream catalog.

The cache database keeps queryable format, importability, upstream-cover,
crawl-status, timestamp, and active inventory fields alongside the
provider-neutral binary payloads. It supports only the current disposable cache
schema; an older cache is replaced and repopulated by the crawler at startup.
`GET /api/web/stats` reports process uptime, registry revision, metadata-service
configuration, active page/publication/importable EPUB counts, crawl state,
extraction queues, normalized failure categories, retry state, and at most
three redacted host/publication examples per category. The same bounded payload
is available to the operations collector from the separate loopback-only
listener on `127.0.0.1:8092` at `GET /internal/operations`. Raw errors and
acquisition URLs never leave the protected catalogue cache.

Search matches normalized contiguous substrings in title and author, returning
title matches first and distinct author matches second. Requests include the
client's supported languages. The response protocol identifies publications by
sequence so it sends payloads only for publications the client does not already
have at the current epoch.

The crawler also reads descriptions, covers, subjects, and ISBNs from bounded
temporary EPUB, MOBI, and PDF files where each format supports them. EPUB
origins that honor HTTP byte ranges transfer only the ZIP directory,
`container.xml`, package metadata, navigation document, and declared cover into
a sparse temporary file. Non-range origins, ZIP64 archives, and unusual cover
layouts automatically fall back to the bounded full-file path. Range capability
is remembered per origin so an incompatible host is not probed for every book. Raw
subject evidence is stored separately from deterministic
canonical assignments, using the same `library-model` projection as local
libraries. When a supported publication has an ISBN but no embedded subjects,
`BOKHEIM_METADATA_URL` enables batched LCC/DDC enrichment from the standalone
Open Library snapshot service. Recommendation requests contain at most 64
weighted canonical subject paths; the client derives this profile locally and
filters owned publications locally, so titles in a reader's library are not
sent to this public service.

Publication extraction is drained by independent persistent workers. Four
download workers stream directly to disk into a four-item bounded queue, while
one extraction worker parses metadata and artwork. This overlaps network waits
without allowing multiple ZIP/image decoders to raise the transient memory
ceiling. Temporary files are deleted after extraction. A restart
changes interrupted jobs back to pending and resumes them from the active
catalogue generations without resetting successful payloads or retrying failed
downloads in a tight loop. Completed crawls can still schedule a bounded retry
of failed jobs. Workers rotate their starting source, preventing a large provider
from holding all download capacity while smaller catalogues wait. Retryable
failures use 15-minute, one-hour, six-hour, and then daily backoff. Permanent
format, access, missing-resource, and safety-limit failures remain terminal
until their catalogue record or enrichment version changes.

Crawls enqueue discovered cover URLs after catalog activation. Download workers
fetch them eagerly and a separate worker pool creates 408 by 612 JPEG
thumbnails. An unchanged URL reuses its existing image. Native clients verify
and download acquisitions directly. Browser clients use the acquisition
endpoint because third-party book hosts commonly reject cross-origin browser
requests. The endpoint re-fetches the registered catalogue page, verifies the
publication identity, exact URL, free acquisition kind, and format, downloads
into a bounded temporary file, and streams it without adding it to the cache.
At most four upstream acquisitions run concurrently.

The registry contains only curated feeds. User-defined catalogs are not proxied,
and the acquisition endpoint accepts a structured catalogue action rather than
an arbitrary URL, so the service is not an open HTTP proxy. A configured
registry file is checked when the registry is requested; only a valid higher
revision is activated.

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
curl --header 'Accept: application/vnd.bokheim+protobuf; version=13' \
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
