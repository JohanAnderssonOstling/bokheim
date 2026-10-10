# Local MOBI patches

Vendored from mobi 0.8.0 (MIT), upstream vv9k/mobi-rs commit
`17fb427c6600fb2022bc1f223c3283e049972532`. The original license is in LICENSE.

The crate root exposes `HuffmanDecoder` and `HuffmanError`.
`HuffmanDecoder::new(records, dictionary_limit)` owns the HUFF/CDIC tables and
phrase dictionary; `decode_into(record, output, output_limit)` decodes one
record without requiring an original-file buffer. The caller removes MOBI
record trailers and handles text encoding.

The decoder checks table/phrase bounds, supports short input and partial final
codes, bounds dictionary storage and recursive phrase expansion, reports cyclic
references as errors, and removes upstream debug printing. The original
whole-book decompression function delegates to the same decoder.

book-content uses async source reads for headers, text, navigation, and images.
Uncompressed, PalmDOC, and HUFF/CDIC text use the same bounded record loop and
encoding conversion. Non-seekable frontmatter inspection retains its existing
16 MiB input limit and uses the same decoder for a 128 KiB text prefix.
