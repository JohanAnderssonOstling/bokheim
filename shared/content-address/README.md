# Book identities and content hashes

`ContentHash` is the BLAKE3 digest of the complete current byte stream. Hashing
never interprets or modifies a file. MP3 folders use the digest of their
reproducible archive, including track order and sidecars.

`BookId` identifies a logical book independently of its current local bytes:

- Existing content-addressed IDs retain their exact 32-byte encoding.
- Provider IDs use tag `01` and a digest of the scoped provider file identity.
- New local IDs use tag `02` and a digest of the library ID and first observed
  checksum. They remain stable when annotations are written into the local PDF.

Tagged IDs cannot be converted into `ContentHash`. Hexadecimal is used at text
boundaries; SQLite and binary transport retain the raw bytes and identity tag.
A legacy ID's `content_hash()` exposes its historical digest, which is not a
substitute for a separately recorded revision checksum.

The first successful upload fixes the canonical checksum and byte length.
Annotations already written into the PDF before that upload are included in
its canonical bytes. Later PDF writes update only the current local checksum,
local fingerprint, and local aliases. Annotation and reading-position records
continue using the stable book ID. The filesystem fingerprint is an inexpensive
mtime/size/inode observation, separate from the byte checksum.

Uploads address the book ID and supply `x-bokheim-content-hash` independently.
The server verifies the complete byte stream and refuses to replace an uploaded
revision. Downloads advertise the canonical checksum and verify bytes against
it. A PDF's canonical reader metadata describes that uploaded revision; local
writebacks never replace its server-side offsets or bytes.

Local aliases map historical byte checksums to stable book IDs. They are never
synchronized. Scanning and import resolve these aliases before assigning a new
identity. A reader resolves a byte-hash reference once when opening; its normal
annotation and progress queries continue using the stable ID.

PDF exporters can add the versioned `BokheimSourceIdentity` catalog dictionary
through `book_inspection::PdfSourceIdentity`. It contains the stable book ID
and, when known, its canonical checksum. This is a recovery hint rather than
proof of a file's contents. The resulting file is still hashed in full. Legacy
`BokheimContentHash` markers remain ordinary bytes and are ignored for identity.

Library schema 45 preserves existing IDs and records their known uploaded
checksums. Sync schema 14 binds each logical book to an immutable byte checksum.
Identical canonical bytes share physical storage and count once toward each
account’s quota, including across libraries. Existing uploads are backfilled.
Neither migration rewrites book files.
