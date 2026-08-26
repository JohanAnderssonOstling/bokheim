# OPDS Store decisions

Date: 2026-08-09

## Ownership

Catalog support is split by responsibility:

- `shared/opds` implements direct external OPDS behavior: registry validation,
  OPDS 1/2 parsing, conditional requests, URL resolution, search templates, and
  verified acquisition downloads.
- `shared/litteraturbanken` implements Litteraturbanken's public catalogue API,
  provider model, URL construction, and download request requirements. A thin
  adapter in `shared/opds` presents it through the existing catalogue pipeline.
- `shared/catalog-model` and `shared/catalog-contract` own provider-neutral
  values and the client/cache-server protocol.
- `shared/catalog-core` owns behavior shared by both providers: normalization,
  SQLite storage, language filtering, OPDS-to-contract conversion, and cover
  thumbnails.
- `client/catalog-client` talks only to the catalogue server for curated source
  discovery, pages, searches, covers, and acquisitions. Its SQLite cache holds
  only catalogue data the client has encountered and remains available for
  offline page and search display. The server registry is authoritative: before
  the first registry sync the client exposes no sources, and it never merges in
  bundled source definitions as a fallback. GPUI refreshes the registry in the
  background at startup; a first-run subject refresh waits for that registry if
  none is cached. Cover and acquisition authorization remains server-side, so
  stale subject cards are not rejected against a temporarily empty local registry.
  The active client registry persists the provider-neutral `CatalogSource`
  records returned by `/api/catalog/sources`, together with opaque root
  locators used only in server requests. OPDS endpoint identities, origin
  canonicalization, and provider URL policy remain server responsibilities.
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

Provider page compaction applies only to catalogue-page navigation. During
ingestion the server follows an upstream continuation chain and stores it as
one complete logical page; clients download and cache that page without a
catalogue range or provider-pagination contract.
Subject navigation instead synchronizes the complete descendant collection for
one opaque subject locator in bounded transport chunks. Native and browser
clients atomically replace that locator's membership in local SQLite, reuse
unchanged publication payloads only when both epoch and revision match, and
remove memberships no longer returned by the server. Search, source/language/
format filters, facets, sorting, and UI ranges are then evaluated locally; they
are not part of the network cache identity.

A synchronized subject collection is reused without a request for five minutes.
After that freshness window, the client fetches only the collection manifest's
epoch and revision. It downloads and atomically replaces the collection only
when that version changed. The server does not expose a subject endpoint that
accepts client query, filter, sort, or range state.

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

Clients can display local cached matches immediately while refreshing from the
catalogue server. A successful refresh replaces the displayed results; a
refresh failure leaves any cached results in place. The native Store does not
contact external OPDS endpoints directly. Catalog sources are exclusively
managed by the catalogue server registry; device-local sources are not
supported.

## Languages

The Store exposes a persisted list of included language tags and sends that
selection with internal search requests while applying the same filter locally.
An empty selection means all languages. All catalogue providers remain visible,
and an explicitly selected single-language catalogue remains browsable.
Catalogue-wide searches skip sources with no language overlap. A catalogue
declared multilingual is filtered publication by publication.

Litteraturbanken exposes provider-owned facets through the neutral catalogue
contract for sort order, publication language, author gender, and modernized or
original spelling. These controls are constructed in the Litteraturbanken crate
and rendered by the Store's existing facet UI.

## Covers and downloads

Covers are keyed by their absolute image URL and stored as standard 408 by 612
JPEG thumbnails. An unchanged URL reuses the existing JPEG indefinitely. Each
server crawl hands its deduplicated URL list to download workers; completed
downloads are handed to a separate thumbnail worker pool. Catalog activation
does not wait for cover processing, and failed covers are retried on a later
crawl.

The catalog cache server persistently stores metadata and covers only. Native
and browser imports use public, bounded acquisition endpoints: the server
re-fetches the registered page, validates the source, publication, exact
download URL, free acquisition kind, and format, then streams a temporary
download to the client without retaining it. The endpoint cannot proxy an
arbitrary URL and limits concurrent upstream acquisitions. EPUB,
PDF, MOBI/AZW/AZW3, and M4B acquisitions are represented by
the neutral contract; actual import remains subject to the local library's
supported formats and acquisition policy.

## Transport and access

The internal service is public and unauthenticated today. Native structured
requests use the versioned Bokheim protobuf media type with identity encoding
only. Browsers use a JSON façade and explicit CORS preflight support. Content
compression and the former HTTP response-cache layer are not supported.
Registry responses retain normal HTTP ETag handling. Only the catalogue server
contacts external catalogue providers; that upstream OPDS traffic remains
standards-compliant XML or JSON.
