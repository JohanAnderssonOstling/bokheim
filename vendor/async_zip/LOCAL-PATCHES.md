# async_zip local patch

Based on async_zip 0.0.19 from crates.io, under the MIT license.
The root Cargo patch selects this copy for EPUB reading.

The new read1 reader assumes ZIP64 extra fields always contain both sizes.
ZIP64 instead includes a value only when the corresponding ZIP32 header field
has its sentinel value. Valid entries extending just one size or just their
local-header offset fail with the upstream parser.

The patch in src/spec/extra.rs and src/spec/constructs.rs passes the header
sentinels into extra-field parsing, reads only the required values, and returns
an error for missing required values instead of unwrapping an absent offset.
Valid ZIP32 values retain precedence over redundant ZIP64 data.
CRC and decompressed-size validation remain enabled.

Upstream examples, tests, and fixtures are omitted from this copy.
The existing epub_provider ZIP64 compatibility and CRC checks cover this fix.
Replace the patch with an upstream release once it supports these cases.

The legacy EOCD signature search also uses an ordinary loop and slice comparison
in place of labelled loop exits, following this repository’s control-flow style.

Unused private header-writing and offset-validation helpers, and the unread
legacy combined-directory comment-length field, are removed. The existing
readers and parsed ZIP comment data retain their behavior.
