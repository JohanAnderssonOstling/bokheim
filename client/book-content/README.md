# book-content

Client-side EPUB, MOBI and FB2 parsing, declared metadata, navigation, cover resources,
and reading preparation. The crate has no HTML-engine dependency or feature gates.

`Book::open(format, reader_factory)` opens an asynchronous source supplied by
book-access. `Book::open_reader(format, reader)` is the synchronous entry point
for scanning, import, thumbnails, and tools. For EPUB and MOBI, synchronous opening reads only
container headers; asynchronous opening reads the ZIP directory and package or
the MOBI record directory and header record without buffering the whole source.
`prepare_reading()` consumes the container and prepares an immutable `ReadingBook`.
Declared metadata extraction runs only when requested by scanning/import.
`metadata()` returns one owned `DeclaredMetadata` structure for text formats.
The parsers retain declared fields, roles, refinements, and textual source
properties; book-inspection applies one library metadata projection. Redundant
singular publisher/language/date fields are replaced by their plural lists.

EPUB preparation eagerly reads container-local manifest CSS and XHTML/HTML,
including navigation and notes. The reader's synchronous resource provider serves
these bytes from memory. Undeclared media types become application/octet-stream,
external URLs are rejected, and embedded/obfuscated fonts are unsupported.
Images and SVG dependencies remain asynchronous. Preparation permits 4 MiB per
stylesheet, 24 MiB per document, and 64 MiB total prepared text.

EPUB uses async_zip with immutable directory metadata. Preparation reuses one
owned cursor; lazy resource reads use independent cursors. Synchronous inspection
owns one buffered cursor in `EpubSession`, reused for metadata, navigation,
cover extraction, and bounded document reads. It uses no source mutex.
The local ZIP64 patch is documented in vendor/async_zip/LOCAL-PATCHES.md at the workspace root.

MOBI opening reads metadata and the record directory without decoding text.
Cover candidates refer to bounded image records. Reading prepares virtual HTML
documents, then retains the reader factory and parsed record directory.
Text and compiled navigation are read from individual bounded records. HUFF/CDIC
loads the code tables and phrase dictionary once, then decodes text records using
the vendored `mobi::HuffmanDecoder`; the local patch is documented in
vendor/mobi/LOCAL-PATCHES.md. Reopenable sources never buffer the whole file.
Images use independent source cursors with the same 24 MiB limit as EPUB.
Cover and reading access share record mapping and bounds. Both formats permit 24 MiB per prepared document and
64 MiB total prepared text. Frontmatter inspection uses the same bounded text
decoder and trailer handling as reading.

`inspect_documents()` exposes a synchronous `DocumentInspection` view for both
formats: document URIs, navigation, and bounded `read_prefix()` access. EPUB
keeps HTML in the ZIP and reads only the requested prefix; MOBI decodes its text
and exposes the same virtual HTML and navigation as reading preparation.
Metadata-only opening does not prepare or decode reading text. There are no
EPUB-only inspection methods on `Book`; the optional metadata source path carries
the package filename when applicable.

Both formats share source ownership, the memory fallback, prepared resources,
navigation entries, and the HTML list navigation walker. MOBI navigation reads
compiled NCX indexes, falling back to a guide-declared inline contents page.
Original text byte offsets become anchors in the prepared documents; filepos
and named-anchor links are rewritten across virtual sections. Preparation exposes
navigation on `ReadingBook`; import persists it and the reader uses it when the
database has no contents entries. Inspection versions include the shared metadata projection so normal import/scanning
can refresh old cached results.

The HTML-engine adapter lives in apps/reader/crates/html/src/book.rs and is
shared by the reader and its command-line tools. Rendering, image scheduling,
decoding, and engine image caching stay outside book-content.

FB2 accepts plain XML and ZIP containers (`.fb2`, `.fbz`, `.fb2.zip`). Its bounded
XML parser handles declared encodings, maps sections and notes to stable HTML
documents and anchors, and decodes embedded base64 images on demand. The XML
input is limited to 96 MiB; prepared text and individual images retain the common
limits above. Source-print ISBNs remain distinct from ebook identifiers.

`comic::ComicBook` inspects CBZ directories and optional ComicInfo metadata without
decoding page images. Natural filename ordering defines page indices, including
ComicInfo cover and bookmark references. `into_reader()` retains independent
archive cursors for bounded asynchronous page reads. CBZ image decoding, cache
management, zoom, spreads and reader controls live in `apps/reader/src/comic/`.

Existing checks: `cargo test --release -p book-content --lib`.

Inspection consumes the common `Book` document and bounded-read API. A
synchronous, non-reopenable source used for reading preparation becomes immutable
bytes to provide independent lazy-image cursors, for either format. Its parsed
container metadata is reused. Interactive local and remote EPUB/MOBI sources
retain their reader factory instead of a full-file buffer. `toc_async()` supports
asynchronous inspection; `toc()` is its synchronous adapter for scanning/import.

`Book`, `ReadingBook`, and `BookReader` are the common public interfaces;
EPUB provider, inspection session, package, and ZIP ownership types stay internal.
Declared metadata types are exported from the crate root.
