# OPDS Store decisions

Date: 2026-08-09

## Ownership

Catalog support is split by responsibility:

- `shared/opds` implements direct external OPDS behavior: registry validation,
  OPDS 1/2 parsing, conditional requests, URL resolution, search templates, and
  verified acquisition downloads.
- `shared/catalog-model` and `shared/catalog-contract` own provider-neutral
  values and the client/cache-server protocol.
- `shared/catalog-core` owns behavior shared by both providers: normalization,
  SQLite storage, language filtering, OPDS-to-contract conversion, and cover
  thumbnails.
- `client/catalog-client` composes the internal cache provider with direct OPDS
  fallback and persists only catalog data the client has encountered.
- `servers/opds-cache` crawls curated external catalogs and serves its complete
  local SQLite snapshot. It never contacts an upstream catalog on a user search
  or page request.
- `client/backend` adds imports and a synchronized, library-owned relation from
  catalog source/publication identity to content hash, but contains no OPDS
  protocol implementation. Catalog URLs and cached metadata remain outside the
  library database.

The internal and external providers expose the same neutral catalog contract
without duplicating OPDS or persistence types.

## Storage and refresh

The client and cache server use the same SQLite schema and publication search
implementation. A catalog URL change invalidates that catalog's stored pages.
Existing caches from older implementations are ignored.

The cache server owns one crawl task per catalog. Catalogs are spread evenly
over a fixed 24-hour cycle. A crawl begins at the catalog root, follows the
current same-origin catalog graph, uses HTTP validators where available, and
activates the new generation atomically. A failed crawl leaves the previous
generation active. Pages that are no longer reachable disappear at the next
successful activation.

Each stored publication has a sequence number that changes only when its
normalized payload changes. Client requests include the active epoch and the
publication sequences they already know. The server returns page/search layout
plus only the changed or missing publication payloads. The server never keeps
per-client state and the client is not required to store the complete catalog.

## Search

Catalog and local-library searches share one text normalizer. Matching is
case- and diacritic-insensitive, collapses all whitespace runs to one space,
keeps punctuation significant, and requires the normalized query to be one
contiguous substring. For example, `harry   potter` matches `Harry Potter`, but
`har pot` does not.

A search returns two ordered sections from one query: title matches first, then
author matches that did not already match the title. There is no result-count
limit. Catalog search is served from SQLite and never calls an external OPDS
source on the request path.

Clients display local cached matches immediately while refreshing from the
selected provider. A successful refresh replaces the displayed results; a
refresh failure leaves the cached results in place without displaying a
"could not refresh" message. If the internal cache server is unavailable or
access policy later prevents its use, curated sources fall back to their direct
OPDS search endpoint. Custom sources are always direct.

## Languages

The client sends its supported language tags with internal search requests and
uses the same filter locally. A catalog with no overlap with the client's
languages is hidden before it is queried. A catalog declared multilingual is
kept, but publications whose language metadata has no client-supported match
are filtered out.

## Covers and downloads

Covers are keyed by their absolute image URL and stored as standard 408 by 612
JPEG thumbnails. An unchanged URL reuses the existing JPEG indefinitely. Each
server crawl hands its deduplicated URL list to download workers; completed
downloads are handed to a separate thumbnail worker pool. Catalog activation
does not wait for cover processing, and failed covers are retried on a later
crawl.

The catalog cache server stores metadata and covers only. It never stores or
serves books. Import always performs a fresh verification against the external
OPDS catalog on the client and then downloads the external acquisition. EPUB,
PDF, MOBI/AZW/AZW3, FB2, CBZ, WebPub, and M4B acquisitions are represented by
the neutral contract; actual import remains subject to the local library's
supported formats and acquisition policy.

## Transport and access

The internal service is public and unauthenticated today. Its structured API
uses the versioned Bokheim bincode media type with identity encoding only;
content compression and the former HTTP response-cache layer are not supported.
Registry responses retain normal HTTP ETag handling. External OPDS traffic
remains standards-compliant XML or JSON.
