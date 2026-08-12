# Repository and crate boundaries

Bokheim organizes Rust packages by runtime ownership rather than putting every
package below a generic `crates/` directory.

```text
apps/                 UI, visual renderers, platform applications, and tools
client/               headless client backend, local persistence, and replica code
servers/sync/          account, synchronization, and blob service
servers/opds-cache/    public curated OPDS cache service
shared/                portable code used across ownership boundaries
```

The main dependency shape is:

```text
apps ───────→ client ───────→ shared
  └────────────────────────→ shared

client document services ──→ apps/renderers

servers/sync ──────────────→ shared
servers/opds-cache ────────→ shared
```

Server-specific crates stay inside their owning server directory. The sync
server and OPDS cache server do not depend on each other.

`apps/renderers` is an application-owned processing subsystem rather than a
pure outer UI layer: the client backend uses its headless EPUB/PDF engines for
metadata and thumbnail work. UI crates remain consumers of `client` and do not
move into the backend ownership directory.

## Shared crates

- `content-address` owns `ContentHash`, the content-addressed identity required
  by both the library model and asset-transfer protocol.
- `catalog-model` owns provider-neutral catalog values and the shared search
  text normalizer.
- `catalog-contract` owns the application and internal cache-server catalog
  protocol, including sequenced publication payloads.
- `catalog-core` owns shared SQLite catalog storage, language filtering, cover
  thumbnails, and OPDS-to-contract conversion.
- `library-model` owns portable semantic book, publication, contributor,
  annotation, bookmark, and reading-position values. Publication identifier
  canonicalization and MARC types live here rather than in a catch-all
  algorithms crate.
- `sync-common` owns the opaque synchronization envelope understood by clients
  and the sync server: protocol IDs, cursors, request/response DTOs, mutation
  kinds, limits, and server-visible blob declarations. It does not decode
  library payload values.
- `account-contract` and `app-contract` own transport-neutral DTOs implemented
  by servers and consumed by applications.
- `opds` owns direct external OPDS parsing, conditional requests, downloads,
  and registry validation.
- `wire` owns the common binary encoding helpers.
- `test-support` owns fixtures shared by client and server tests.

## Client crates

- `backend` (`epub-svelte`) owns SQLite, native paths, scanning, local file
  materialization, session persistence, and composition for application UIs.
- `catalog-client` composes the internal curated cache provider with direct OPDS
  fallback and the client's partial SQLite catalog cache.
- `library-replica` owns the client-replica schema for synchronized library
  payloads, local mutation coalescing, conflict keys, and conversion to and from
  `sync-common::WireMutation`. The sync server never links it.
- `sync-client` owns authentication, HTTP/WebSocket transport, asset transfer,
  and protocol-response validation. It does not interpret library payloads.
- `library-registry`, `remote-reader`, `book-metadata`, `thumbnail`, and
  `audiobook-library` are headless client services composed by the backend and
  applications.

Filesystem-relative paths, scanners, SQLite projections, and local placement
rules remain private client-backend details. They are not part of
`library-model` or the client/server protocol.

## Applications and renderers

Application binaries live directly under `apps/`. Shared application UI is
under `apps/ui/`, and visual and document rendering is under `apps/renderers/`.
The analysis tooling is maintained as the separate sibling `DataAnalysis`
project rather than as an application in this workspace.

## Servers

- `servers/sync` is a separate Cargo workspace. It owns accounts, synchronized
  state storage, blobs, thumbnails, HTTP endpoints, and PostgreSQL adapters.
- `servers/opds-cache` serves the curated public OPDS registry and cached
  catalogue data. It owns no synchronization or account state.
