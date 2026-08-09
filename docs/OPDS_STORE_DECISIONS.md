# OPDS Store decisions

Date: 2026-08-01

## Product model

Store is a fifth navigation destination in both the desktop and mobile clients. An OPDS catalog is presented as a read-only remote library: users choose a source, navigate folders with breadcrumbs, search when the source advertises search support, page through results, and add free EPUBs to the active local library.

The first version imports books into the library root. Choosing an import folder, loans, DRM, and purchases are intentionally deferred. OPDS catalogs are anonymous; authenticated feeds are not part of the product model.

## Built-in sources

The catalog registry, cache persistence, revision policy, and device-local custom endpoint store live in `BokheimOpds`; clients do not duplicate URLs or source metadata. `Backend` only composes that store with catalog-server fallback and local-library import.

| Source | Endpoint | Languages |
| --- | --- | --- |
| Project Gutenberg | `https://www.gutenberg.org/ebooks/search.opds/` | Multilingual |
| Open Library | `https://openlibrary.org/opds/` | Multilingual |
| Unglue.it | `https://unglue.it/api/opds/` | Multilingual |
| textos.info | `https://www.textos.info/catalogo.atom` | Spanish |
| Ebooks Gratuits | `https://www.ebooksgratuits.com/opds/index.php` | French |
| Office of the Historian | `https://history.state.gov/api/v1/catalog` | English |
| Gallica | `https://gallica.bnf.fr/opds` | French |

## Shared contract and correctness boundary

Catalog fetching, OPDS 1.2 Atom parsing, OPDS 2.0 JSON parsing, URL resolution, search-template expansion, acquisition classification, downloads, and EPUB import all live in the Rust backend. The mobile client calls typed Tauri commands generated with tauri-specta; desktop calls the same `AppCore` operations directly.

The backend exposes `OpdsPublication.free_epub` as the only acquisition that the clients may present as an immediate free download. It is present only for an EPUB whose OPDS acquisition relation is open-access or generic. Buy, borrow, preview/sample, and subscribe relations remain visible in parsed data but never become an Add-to-library action. The Rust importer independently enforces the same rule at its command boundary, so a forged client request still cannot import a paid or borrowed acquisition as though it were free. This keeps the policy identical in both UIs and makes adding another acquisition kind a Rust-only decision.

Downloads go to an application-owned temporary directory, are size-limited, and pass through the normal Rust EPUB importer before becoming library books. Partial and invalid downloads are removed. Catalog and acquisition URLs must be absolute HTTP(S) URLs without embedded credentials, responses have size and idle-time limits, and HTTP errors are surfaced per catalog without affecting the local library.

## UI behavior

- Source failures are isolated and offer retry; one unavailable provider does not disable Store.
- The root catalog's search descriptor remains available while navigating so search works from subfolders.
- OPDS groups are flattened into the matching folder and publication views so both OPDS versions have the same interaction model.
- Previous and next links use the same catalog loader and navigation history.
- An Add-to-library button is shown only when `free_epub` is present. It is disabled until a local library exists and transitions through Adding and Added states.
- A successful mobile import invalidates home, author, and folder caches; a successful desktop import advances the shared library refresh signal.

## Source references

- Project Gutenberg offline catalog documentation: <https://www.gutenberg.org/ebooks/offline_catalogs.html>
- OPDS 2.0 specification: <https://specs.opds.io/opds-2.0>
- Office of the Historian catalog API: <https://history.state.gov/developer/catalog>
- Gallica API documentation: <https://api.bnf.fr/fr/node/2640>
- Unglue.it API help: <https://unglue.it/api/help>

The seven built-in endpoints were live-fetched and parsed by the ignored integration test `open_built_in_catalogs_are_live_and_parseable` on 2026-08-01.
