# OPDS Store decisions

Date: 2026-08-09

## Ownership

Catalog support is split by responsibility:

- `servers/opds-cache/crates/opds` implements direct external OPDS behavior: registry validation,
  OPDS 1/2 parsing, conditional requests, URL resolution, and verified
  acquisition downloads. Upstream search descriptions and templates are not
  part of the application contract.
- `servers/opds-cache/crates/litteraturbanken` implements Litteraturbanken's public catalogue API,
  provider model, URL construction, and download request requirements. A thin
  adapter in `servers/opds-cache/crates/opds` presents it through the existing catalogue pipeline.
- `shared/catalog-model` owns provider-neutral values and language filtering, while
  `shared/catalog-protocol` owns only the client/cache-server page and search
  protocol.
- `shared/catalog-core` owns only the shared cover cache and its cover-specific
  errors. It has no SQLite schema, connection, conversion helper, language
  policy, OPDS routing, authoritative page/search/subject queries, response
  materialization, or server job/operations types.
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
  locators used only in server requests. One client-owned store, schema, and
  SQLite connection serialize registry persistence, exact language-keyed
  page/search layouts, materialization, and partial subject-card caching. Those
  tables and behaviors live exclusively in
  `client/catalog-client`; no shared schema is installed.
  OPDS endpoint identities, origin canonicalization, and provider URL policy
  remain server responsibilities.
- `servers/opds-cache` crawls curated external catalogs and serves its complete
  local SQLite snapshot. Its `store` module owns crawl generations, extraction
  and cover queues, backfills, process controls, operational inventory, and
  materialized index refreshes, its complete single-version SQLite schema, and
  its connection pool. The server never
  contacts an upstream catalog on a user search or page request.
- `client/backend` adds imports and a synchronized, library-owned relation from
  catalog source/publication identity to content hash, but contains no OPDS
  protocol implementation. Catalog URLs and cached metadata remain outside the
  library database.

Every curated upstream is adapted into the same neutral server snapshot without
exposing provider routing or persistence types to clients.

## Storage and refresh

The client and cache server own independent SQLite schemas and connection
management. Authoritative page/search SQL and native delta assembly live in
`servers/opds-cache`; offline page/search resolution and response
materialization live in `client/catalog-client`. `catalog-core` owns neither
schema nor pool. Opening a client cache creates only partial client payload,
page, search, registry, and subject-card tables; the server schema contains all
canonical rows, crawl state, queues, enrichment state, operational review data,
and serving indexes. A catalogue URL or server epoch change invalidates that
catalogue's client-cached pages and searches. The client stores the server's
exact layouts per supported-language set instead of reconstructing membership
from a flat publication cache. The current client schema is version 3;
disposable rows from older layouts are discarded while registry and subject-card
state are retained. The server schema is version 4. An incompatible server
store is disposable and rebuilt from upstream catalogues rather than maintained
through parallel schema versions.

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

Project Gutenberg description cleanup, work identity, and illustrated-edition
preference are server ingestion policy. Ordinary pages are canonicalized before
they enter the authoritative snapshot, and subject indexing applies the same
preference across the complete source generation. Clients neither rewrite nor
deduplicate Gutenberg payloads, and the client schema stores no
Gutenberg-specific presentation flag.

Provider-native popularity extraction is also server-owned. The OPDS adapter
interprets Gutenberg download counts and Litteraturbanken interaction tags,
then passes an opaque numeric ordering score into server storage. The
richer reading-log and rating popularity value is a server-local model.

Crawl write preparation is server-owned as well. Encoding page batches,
precomputing publication hashes and normalized inventory, assigning provider
popularity, and staging authoritative generations all live in the server store.
The client owns its separate server-sequenced payload writer.

Replacement-generation carry-forward is server policy. Copying enriched
subjects, extracted table-of-contents terms, and popularity observations from
the active generation into a staged crawl is implemented by the server crawl
store rather than any shared cache layer.

Subject navigation is server-authoritative. Each request includes its opaque
subject locator, search text, source/language/format filters, sort order, and
display range. The server evaluates that complete query against one SQLite
snapshot and returns ordered entry IDs, navigation, facets, totals, and range
metadata together. Native clients retain only immutable publication-card
payloads for the current server epoch and fetch missing cards by returned ID;
they do not synchronize collection membership or reproduce subject queries in
client SQLite. Browser clients receive the same authoritative result with the
cards included directly.

The native subject endpoint returns only that authoritative index. A separate
bounded payload endpoint resolves missing immutable cards. The browser JSON
endpoint returns the full card response directly; transport strategy is not a
client-provided browse flag. Subject query, navigation, facet, ordering, and
range SQL lives exclusively in the catalogue server. The publication-card
cache table and its resolver live exclusively in `client/catalog-client`.

## Search

Catalog and local-library searches share one text normalizer. Matching is
case- and diacritic-insensitive, collapses all whitespace runs to one space,
keeps punctuation significant, and requires the normalized query to be one
contiguous substring. For example, `harry   potter` matches `Harry Potter`, but
`har pot` does not.

A search returns three ordered sections from one query: title matches first,
then author matches that did not already match the title, then table-of-contents
matches that appeared in neither earlier section. There is no result-count
limit. Catalog search is served from the authoritative SQLite snapshot and
never calls an external OPDS source on the request path. No upstream
provider-search route exists.

Clients can display local cached matches immediately while refreshing from the
catalogue server. A successful refresh replaces the displayed results; a
refresh failure leaves any cached results in place. The native Store does not
contact external OPDS endpoints directly. Catalog sources are exclusively
managed by the catalogue server registry; device-local sources are not
supported.

## Languages

The client exposes a persisted list of included language tags and sends that
selection with subject and catalogue search requests for server-side filtering.
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
