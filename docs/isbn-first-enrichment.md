# ISBN-first subject and author enrichment

Inspection runs during import/scanning and saves accepted bibliographic page evidence. Enrichment consumes that saved evidence; it does not reopen the book for each provider request.

The automatic pipeline uses these decisions:

1. A local LCC subject completes subject enrichment. No subject ISBN request or title/author subject search is scheduled for that book.
2. Otherwise, look up checksum-valid ISBN references from inspected copyright/cataloguing sections, including print and unspecified formats. Page references take priority over embedded ISBNs; embedded ISBNs are used when no accepted page references exist.
3. A usable LCC result completes subject lookup. An ISBN record with no usable LCC code, as well as an ISBN miss, permits title/author fallback. Missing responses and request errors retain retry behavior.
4. Books without ISBNs or local LCC use title/author search. A reference supplied during that pass can be looked up before the pass completes.
5. Author enrichment is independent of subject completion. It queries all ISBNs from the preferred evidence source (copyright/cataloguing references first, otherwise embedded ISBNs). A missing ISBN result retries; conflicting authoritative author lists remain ambiguous. A miss for one ISBN does not hide a successful second ISBN.

Inspected ISBN references are projected as `book_publication_value.kind='enrichment_isbn'` from the original inspection properties. They are not inserted into `book_identifier` as claims about the file’s edition. Subject provenance records the reference ISBN. Raw book bytes remain unchanged.

Only accepted local bibliographic sections contribute reference ISBNs. Embedded title/author values do not gate extraction: a bounded copyright/cataloguing block can supply ISBN/LCC even when those values are missing or wrong. Printed LCC replaces conflicting embedded LCC. Checks against references, promotional sections, malformed evidence and invalid ISBN checksums remain in force. This change does not reimport or backfill old libraries.

The normal bounded background database queue and batched metadata writes remain in use; provider requests execute outside write transactions.

Previous validation (before the page-priority change; optimized release): 18 backend enrichment tests passed, including direct local LCC with independent multi-ISBN author lookup, printed ISBN subject lookup without title search, and ISBN hits without classifications falling back to title/author search. Eight related-ISBN policy tests passed. The backend WASM library release build passed. The broader database/inspection run earlier in this change passed 159 database tests and 26 inspection tests (one ignored).

Page-priority validation (2026-09-07, optimized release): 160 database tests and 28 inspection tests passed (one ignored). Reinspection of the previous 208 no-match files found usable ISBNs in 96, including 10 with direct LCC; 44 additional files acquired usable evidence. 206 EPUB/PDF files were reinspected; the two AZW/AZW3 files retain their prior no-code result because this page-extraction change does not add inspection support for those formats. This is extraction coverage, not a fresh catalogue matching result.

ISBN line-boundary fix: ISBN parsing no longer consumes digits on the next printing-number line. Verified against the actual Locke EPUB (0192803948); 29 release inspection tests passed, one ignored. Reinspection now yields ISBN/LCC evidence in 98 of the original 208 no-match files (10 with LCC), leaving 110 without accepted codes. Catalogue lookup has not been rerun.

Hyphenated ISBN recovery: line breaks immediately around explicit hyphens are accepted, including en dashes, Unicode hyphens and nonbreaking hyphens. Unhyphenated line breaks still terminate the number, and checksum validation remains mandatory. Verified on the actual Atheism PDF (0192804243) and Locke EPUB (0192803948); 30 release inspection tests passed, one ignored.

ISBN anchor search now scans up to 128 characters after each ISBN marker for the first checksum-valid number, stopping at the next marker. Labels need not be adjacent to the marker; scope is retained from preceding text or a following parenthesized label. Small whitespace gaps within the marker (such as PDF-extracted IS BN) are supported. Verified both UX for Developers ISBNs (9781484242261 and 9781484242278) and retained Atheism/Locke recovery. All 32 release inspection tests passed, one ignored.
