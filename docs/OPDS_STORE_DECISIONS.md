# OPDS Store decisions

Date: 2026-08-09

## Ownership

Catalog support is split by responsibility:

- `servers/opds-cache/crates/opds` implements direct external OPDS behavior: registry validation,
  OPDS 1/2 parsing, conditional requests, URL resolution, search templates, and
  verified acquisition downloads.
- `servers/opds-cache/crates/litteraturbanken` implements Litteraturbanken's public catalogue API,
  provider model, URL construction, and download request requirements. A thin
  adapter in `servers/opds-cache/crates/opds` presents it through the existing catalogue pipeline.
- `shared/catalog-model` owns provider-neutral values, while
  `shared/catalog-protocol` owns only the client/cache-server page and search
  protocol.
- `shared/catalog-core` owns provider-neutral catalogue models and low-level
  SQLite storage primitives, plus language filtering and cover caching. It has
  no OPDS routing, authoritative page/search/subject queries, response
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
  locators used only in server requests. One client-owned store and SQLite
  connection serialize registry persistence, page/search materialization, and
  partial subject-card caching. Those tables and behaviors live exclusively in
  `client/catalog-client`; the shared schema contains no client-value table.
  OPDS endpoint identities, origin canonicalization, and provider URL policy
  remain server responsibilities.
- `servers/opds-cache` crawls curated external catalogs and serves its complete
  local SQLite snapshot. Its `store` module owns crawl generations, extraction
  and cover queues, backfills, process controls, operational inventory, and
  materialized index refreshes. Shared cache code exposes only the low-level
  transaction primitives needed by that server-owned layer. The server never
  contacts an upstream catalog on a user search or page request.
- `client/backend` adds imports and a synchronized, library-owned relation from
  catalog source/publication identity to content hash, but contains no OPDS
  protocol implementation. Catalog URLs and cached metadata remain outside the
  library database.

The internal and external providers expose the same neutral catalog contract
without duplicating OPDS or persistence types.

## Storage and refresh

The client and cache server use the same base SQLite catalogue schema, but not
the same directional behavior. Authoritative page/search SQL and native delta
assembly live in `servers/opds-cache`; offline page/search resolution and
response materialization live in `client/catalog-client`. `catalog-core`
exposes only the neutral row and transaction primitives both sides need. The
server layers a separately versioned schema over the base for worker queues,
process controls, enrichment state, operational review data, and materialized
serving indexes. Opening a client cache never creates those server tables. A
catalog URL change invalidates that catalog's stored pages. Existing caches
from older base-schema implementations are ignored.

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
deduplicate Gutenberg payloads, and the neutral shared schema stores no
Gutenberg-specific presentation flag.

Provider-native popularity extraction is also server-owned. The OPDS adapter
interprets Gutenberg download counts and Litteraturbanken interaction tags,
then passes an opaque numeric ordering score into neutral shared storage. The
richer reading-log and rating popularity value is a server-local model.

Crawl write preparation is server-owned as well. Encoding page batches,
precomputing publication hashes and normalized inventory, assigning provider
popularity, and staging authoritative generations all live in the server store;
the shared cache exposes only neutral row primitives and the client's
server-sequenced payload write.

Replacement-generation carry-forward is server policy. Copying enriched
subjects, extracted table-of-contents terms, and popularity observations from
the active generation into a staged crawl is implemented by the server crawl
store rather than the shared cache.

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
