# Bokheim catalog server

This is the standalone public service that curates OPDS catalog endpoints. It
does not share a process, state, database, credentials, or authentication code
with the Bokheim synchronization server.

The service exposes public unauthenticated endpoints:

- `GET /health`
- `GET /api/opds/catalogs`
- `GET /api/opds/sources/{source-id}/page?locator={url}`
- `POST /api/opds/sources/{source-id}/search`
- `GET /api/opds/search?q={query}`

All structured communication between Bokheim clients and this service uses the
versioned Bokheim bincode media type and adaptive zstd level-1 compression. This includes the registry,
normalized pages, source searches, and global search. The server stores the
exact compressed bytes for registries, normalized pages, source searches, and
global searches, so cache hits perform neither serialization nor compression
again. Prepared page and source-search entries share the five-minute lifetime
of the parsed OPDS cache and are bounded to 512 entries and 64 MiB. External
OPDS communication remains standards-compliant XML or JSON. Covers remain
canonical JPEG bytes because applying transport compression to an
already-compressed image adds CPU cost without a useful size reduction.

Search definitions are tagged contract values inside the binary payload. They
never contain a nested JSON token.

Public OPDS pages are fetched and normalized by this service, not by each app.
It keeps a bounded shared page cache, refreshes every public root periodically,
deduplicates concurrent work, and caches normalized global-search results. The
HTTP responses also permit cross-origin reads and are publicly cacheable.

The registry contains only anonymous curated feeds. User-defined catalogues are
not proxied, so the public service never receives arbitrary upstream URLs.

The service validates the complete registry before startup. When
`BOKHEIM_CATALOG_FILE` is configured, it checks that file on each request and
activates valid changes without a restart. A malformed replacement never
displaces the last valid registry.

## Local use

```sh
cargo run -p catalog-server
curl --compressed --header 'Accept: application/vnd.bokheim+bincode; version=5' --header 'Accept-Encoding: zstd' --output /dev/null http://127.0.0.1:8090/api/opds/catalogs
```

Configuration:

- `BOKHEIM_CATALOG_BIND`, default `127.0.0.1:8090`
- `BOKHEIM_CATALOG_FILE`, optional external registry path
- `BOKHEIM_CATALOG_REFRESH_SECONDS`, root refresh interval, default 900 and minimum 60
- `BOKHEIM_CATALOG_CACHE_FILE`, optional persistent normalized-page cache
- `BOKHEIM_CATALOG_ALLOW_PUBLIC_HTTP=true`, required for a non-loopback bind

Validate a candidate registry without starting the service:

```sh
cargo run -p catalog-server -- --check-catalog servers/catalog/opds.json
```

Every published change must increment `revision`. Production clients use
`https://catalogs.bokheim.se/api/opds/catalogs`, reject stale or malformed
responses, and retain their last valid atomic cache.

See [deploy/README.md](deploy/README.md) for an independent systemd and Caddy
deployment.
