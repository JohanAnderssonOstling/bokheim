# Retained covers in downloads

Catalog images and embedded file artwork are separate. `catalog retain-covers`
updates card/detail images and their source evidence. After replacing a hosted
EPUB's cover, also run `embed-epub-cover.py` on a derivative file and publish it
under its new content address:

```sh
python3 servers/catalog/tools/embed-epub-cover.py original.epub updated.epub selected.jpg
```

Card geometry is baked into JPEGs by the shared `thumbnail` crate. Short covers,
including square and landscape artwork, move toward height/width 1.6 with at
most 7% total horizontal cropping (3.5% per side) and 20% vertical stretching.
Tall covers use up to 20% vertical compression before a centered vertical crop
to 1.6. Detail/master images retain the complete artwork. Publishing derives
versioned `card-v5-300` and `card-v5-600` files from retained detail images;
clients display these directly. Local card caches use the same policy version
and rebuild from their unmodified masters through thumbnail jobs.

The tool requires Pillow. It replaces the declared raster cover in its original
format, or adds a cover image and opening page with EPUB cover metadata when
there is no usable raster slot. It validates the resulting ZIP and preserves
the bytes of every non-cover resource. Originals are never overwritten. Its JSON
output records the changed resources, image dimensions and derivative SHA-256.

Hash the **complete resulting EPUB** with the same BLAKE3 algorithm as
`content-address`, retain the original file, and store the derivative as
`books/<new-hash>.epub`. Copy the curated `book` row to the new hash, move its
`book_source` and `source_entry.book_hash` references, then remove the old active
row in one SQLite transaction with foreign keys enabled. Preserve source
metadata, title, author, languages, navigation and format. Record old/new file
hashes and cover provenance in `source_entry.metadata.embedded_cover_update`.
The database must have a verified backup before this transaction.

Carry the selected card/detail images to the new hash. Upload and verify the
derivative files and images at the catalog origin, then publish and install a
new catalog snapshot. Never serve modified bytes at an old hash URL:
`library-catalog::download_book` verifies the complete downloaded file against
the catalog hash. Keep old URLs and files available for existing snapshots.

For downloadable MP3 audiobooks, the source archive is repacked for playback
without inserting catalog artwork. Tracks and source artwork remain unchanged.
The client fetches the selected catalog detail image independently of its
browsing cache and passes it to the same CPU thumbnail decoder used by regular
enrichment. `LibraryImport::refresh_external_catalog_audiobook_cover` writes the
master, browse and high-density card variants into the normal thumbnail cache,
replacing existing variants, marks the thumbnail ready and notifies the UI.
An invalid replacement leaves the old cached artwork intact.

New catalog audiobook imports call this replacement directly. Library file jobs
also refresh existing MP3 audiobooks with an unambiguous title, author and
language match to a catalog entry. Each library session refreshes a matched book
once; temporary failures remain eligible for retry. Thumbnail recovery keeps an
explicit replacement that arrived while file extraction was in flight.

Catalog artwork is never embedded in MP3 tags or added as a cover.jpg sidecar.
The source audiobook files are not changed. EPUB replacements continue to be
served as updated files in the published catalog snapshot.
