# Content addresses

`ContentHash` stores a raw 32-byte BLAKE3 digest. Hexadecimal is used for text
boundaries such as filenames, URLs and JSON; SQLite uses BLOB and PostgreSQL
uses bytea. Binary storage and transport messages contain the raw digest.
The same crate provides streaming byte-hashing helpers for native and browser
readers. Hashing does not interpret file formats.

Book files use the BLAKE3 hash of their complete current bytes as identity.
PDF and M4B follow the same rule as EPUB and other single-file formats.
`hash_file` seeks to the start and hashes the file; `hash_reader` hashes from the
reader's current position. Neither parses formats nor modifies files.

External edits to metadata, covers, chapters or content change identity.
Renaming a file preserves its identity because its bytes are unchanged.
Existing `BokheimContentHash` PDF markers are ignored, including malformed
markers. They remain part of the bytes being hashed and are not removed.
Enrichment and reader annotations belong in the library database.

Every book has one content hash for library identity, storage and transfer
verification. Uploads verify bytes against the hash in the URL; downloads use
that same hash as their ETag. MP3 folders use the hash of their reproducible
ZIP archive, including track order and NFO, CUE and cover sidecars. Editing
any included bytes changes the folder's identity.

PDF entries created with embedded original-hash identities and M4B entries
created with audio fingerprints, and MP3 folders created with track-only hashes have different keys from the byte-hash scheme.
Those existing entries require migration or reimport before their assets can
be verified with this implementation. Clients and servers must use the same
identity scheme. This change does not rewrite existing files or library data.

This consolidation uses library schema 26, sync server schema 7 and transport
version 26, with binary storage version 15. Earlier library databases require migration or recreation. The
server schema upgrade rejects legacy identity-to-blob mappings that differ;
those books must be migrated or reimported first.

Book register keys, pagination cursors and browser worker hash results use
raw content hashes internally. Other register identifiers remain text IDs.
