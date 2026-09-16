# Local copyright and cataloguing evidence

Publication inspection reads EPUB front/back matter before catalogue lookups. It follows copyright TOC links, including fragments within combined XHTML files; other TOC fragments and contents/prologue/introduction headings terminate the block. Copyright credits such as “Introduction © 2012” remain part of the block.

The scanner recognizes Copyright, ©, (c), numeric copyright entities, &copy;, cataloguing headings and EPUB copyright-page semantics. It preserves paragraph boundaries and inline text, excluding head/script/style. Self-closing scripts do not hide subsequent document text.

Limits: first/last eight spine documents, additional copyright TOC targets (32 documents total), 4 MiB streamed markup per document, 16 KiB opening search and bibliographic blocks, 32 accepted blocks. An explicit copyright fragment can start a new block beyond the opening search. The whole audiobook/book is not loaded into a text buffer. Unlabelled copyright text deep in body prose is not evidence.

ISBN markers have a bounded forward search for edition labels. A second regex pass searches the entire accepted block for unlabelled ISBN-10/ISBN-13 numbers with spaces or hyphens, rejecting longer number runs and invalid checksums; ISBN-13 must start with 978 or 979. Explicit ePDF references retain their PDF format. ISBN length labels such as `ISBN: 13` are accepted. Adjacent ISBN-10/ISBN-13 values may split at whitespace only when both complete values pass validation. Recognized printing sequences and numbers explicitly labelled as library control numbers are excluded from the unlabelled pass. Ordinary sentences starting with “References” do not terminate or reject the copyright block. Print/electronic scope remains attached to evidence. Related ISBNs support catalogue lookup; they are not silently promoted to the current electronic edition's identity.

LCC candidates can occur inline in cataloguing prose. They remain in inspection evidence even if unresolved. Promotion requires both a resolved subject and numeric coverage in the local taxonomy; a generic alphabetic prefix or a range spanning subclasses does not validate a class number. For example, Alexander Hamilton's printed E3002.6.H2C48 2004 is extracted but not promoted. This does not guess a corrected number. Numeric coverage is conservative local coverage, not a claim of official item-level validity. Rejected printed codes do not replace embedded subjects or add an LCC subject that suppresses further lookup.

`INSPECTION_VERSION` is 5. Evidence property serialization remains compatible with the existing `inspection:local:v2` schema and readers.

Validation:

```sh
cargo test --release -p publication-inspection --features pdfium --lib
cargo build --release -p publication-inspection --features pdfium --example inspect_local
cargo build --profile web-release -p publication-inspection --features pdfium --target wasm32-unknown-unknown
```

The release `inspect_local` example accepts `--keep-going` for a JSONL audit, returning an error record for each unreadable file. It reports local ISBNs, scoped reference ISBNs, declared LCC and conservatively usable LCC. It performs no network enrichment and writes neither books nor the library database.

Page extraction support is complete for the agreed scope. See [native OCR](native-ocr-inspection.md) for the native-only Tesseract fallback, bounds, deployment requirements, and WASM exclusion. Cataloguing continuation pages are accepted without repeating the heading, malformed ISBN-labelled metadata wrappers are handled, and labelled number groups can span plain line breaks.
