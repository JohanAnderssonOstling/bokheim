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
- `GET /api/opds/sources/{source-id}/cover?image_url={url}`

Structured requests and responses use
`application/vnd.bokheim+bincode; version=11` with identity encoding. OPDS
upstreams remain XML or JSON, and covers are canonical JPEG thumbnails.

The service stores complete catalog generations in SQLite. One task per catalog
crawls from its root and follows the current same-origin catalog graph. Catalogs
are distributed evenly over a fixed 24-hour cycle. Conditional upstream
requests reuse unchanged pages, while a successful crawl activates as one
transaction; failures retain the previous generation. Page and search HTTP
requests read SQLite only and never wait on an upstream catalog.

Search matches normalized contiguous substrings in title and author, returning
title matches first and distinct author matches second. Requests include the
client's supported languages. The response protocol identifies publications by
sequence so it sends payloads only for publications the client does not already
have at the current epoch.

Crawls enqueue discovered cover URLs after catalog activation. Download workers
fetch them eagerly and a separate worker pool creates 408 by 612 JPEG
thumbnails. An unchanged URL reuses its existing image. Books are never cached
or served; clients verify and download acquisitions from the external catalog.

The registry contains only curated feeds. User-defined catalogs are not proxied,
so this service never accepts arbitrary upstream catalog URLs. A configured
registry file is checked when the registry is requested; only a valid higher
revision is activated.

## Local use

```sh
cargo run -p catalog-server
curl --header 'Accept: application/vnd.bokheim+bincode; version=11' \
  --output /dev/null http://127.0.0.1:8090/api/opds/catalogs
```

Configuration:

- `BOKHEIM_CATALOG_BIND`, default `127.0.0.1:8090`
- `BOKHEIM_CATALOG_FILE`, optional external registry path
- `BOKHEIM_CATALOG_CACHE_FILE`, optional SQLite cache path
- `BOKHEIM_CATALOG_COVER_CACHE_DIR`, optional JPEG cache directory
- `BOKHEIM_CATALOG_ALLOW_PUBLIC_HTTP=true`, required for a non-loopback bind

Validate a candidate registry without starting the service:

```sh
cargo run -p catalog-server -- --check-catalog servers/opds-cache/opds.json
```

Every published registry change must increment `revision`. See
[deploy/README.md](deploy/README.md) for the independent systemd and Caddy
deployment.
