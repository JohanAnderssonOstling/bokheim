# Backend crate boundaries

Bokheim uses one portable application model with separate runtime adapters. The
native Tauri/desktop backend and the web backend reuse the crates in
this document instead of depending on each other.

```text
                    bokheim-algorithms
                           |
                     sync-common

                         sync-common
                        /     |      \
          bokheim-domain      |       projection
                 |            |
       bokheim-app-contract    |       bokheim-sync-client
                 \            |              /
                  \           |             /
                   native Backend or WebServer

          storage-sqlite -> native Backend / WebServer

sync-common -> bokheim-opds        -> native Backend / catalog server / WebServer
           \-> bokheim-sync-client -> native Backend / WebServer

thumbnail -> native Backend
book-metadata -> native Backend
bokheim-library-files -> native Backend
library-registry -> native Backend
remote-reader -> native Backend
bokheim-identifiers -> domain / sync / storage / native Backend
```

## Reusable crates

- `bokheim-identifiers`: dependency-free ownership of stable library, book,
  directory, file, replica, and content identities. Higher layers may re-export
  these types but do not define them.
- `bokheim-algorithms`: deterministic, allocation-light algorithms and the typed
  canonical `ReadingPosition` value that compile for native targets and
  `wasm32`. Reader-position parsing, validation, and wire serialization live
  here so every runtime applies the same rules.
- `bokheim-domain`: validated reading, path, library-value, and book-location
  types. It has no database, filesystem, networking, UI, or async runtime.
- `bokheim-app-contract`: transport-neutral frontend DTOs, preferences,
  download/transfer state, account state, and reader annotation/bookmark data.
- `bokheim-sync-client`: validated server origins and sessions, authentication,
  library catalogue calls, sync push/pull calls, asset-existence calls, and
  verified book asset transfer calls, and protocol response validation. It never
  persists a session or opens a database.
- `projection`: deterministic, storage-independent conflict projection.
  It converts directory intents into a parent-before-child live projection.
- `bokheim-opds`: OPDS discovery, parsing, searching, download, and registry
  validation. It has no dependency on either application backend or sync server.
- `storage-sqlite`: shared SQLite connection safety settings, web
  migrations, transactional state persistence, and opaque encrypted-session
  record storage. Both runtime adapters reuse it; it has no HTTP, Tauri,
  scanner, renderer, or sync-transport dependency.
- `thumbnail`: pure EPUB, CBZ, M4B, and optional PDF cover extraction,
  resizing, and JPEG encoding. It has no database, application state, library
  path, synchronization, or UI dependency.
- `book-metadata`: typed EPUB, generic publication, PDF fallback, and
  M4B metadata inspection. It has no database, application state, library path,
  synchronization, or UI dependency.
- `bokheim-library-files`: canonical library-root ownership, typed path
  resolution, verified installation, and guarded removal of managed files. It
  has no database, application state, synchronization transport, or UI dependency.
- `library-registry`: validated library entries, manifests, discovery,
  attachment, and atomic device-local registry persistence. It has no database,
  synchronization, networking, scanner, renderer, or UI dependency.
- `remote-reader`: authenticated HTTP range reading, sparse on-disk
  caching, offline reopening, and cache eviction for remote books. It has no
  database, application state, scanner, renderer, or UI dependency.

## Runtime adapters

- `Backend` (`epub-svelte`) is the native adapter. It owns SQLite, native paths,
  scanning, local file materialization, secure session-file persistence, and
  composition for Tauri and desktop frontends.
- `bokheim-catalog-server` serves the public curated OPDS registry. It is a
  separate process with no synchronization state or credentials.
- `sync-server` owns accounts, synchronized event storage, blobs, and
  thumbnails. Its account component is the single identity authority for native,
  mobile, and web clients, including login, session validation, refresh, and
  revocation. It deliberately has no OPDS code, route, registry, or dependency.
- `bokheim-web-server` is the web adapter. It owns authenticated HTTP handlers,
  authenticated routing to one SQLite database per user, encrypted durable web sessions,
  content-addressed book files, CSRF/origin enforcement, static frontend
  hosting, and OPDS proxying. It talks to `sync-server` through
  `bokheim-sync-client` and does not import the native `Backend` crate.
