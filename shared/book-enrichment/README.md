# Book enrichment

Portable, resumable policy shared by Bokheim clients and catalog ingestion. The crate owns subject stage order, evidence types, printed/resource ISBN verification and classification consensus. It depends on no file parser, OCR engine, database or network runtime.

`SubjectPipeline` requests these stages, in order:

1. Resolve metadata LCC/BISAC.
2. Look up metadata ISBNs.
3. Inspect pages if there is still no resolved LCC, reusing cached evidence.
4. Resolve page LCC/BISAC, then look up newly discovered ISBNs.
5. Search title/authors and look up any newly supplied ISBNs.

Only resolved LCC completes subject enrichment; BISAC is retained as partial evidence. Author enrichment runs independently even when local LCC already completes subjects. Successful ISBN requests are deduplicated across equivalent ISBN-10/13 values. Hosts call `finish_step` only after definitive responses, never after timeouts or omitted results, and persist the serialized state.

`LookupPlan` and `defer_initial_page_inspection` support cheap initial import: metadata ISBNs defer inspection pending lookup, rather than permanently skipping the fallback.

Format parsing owns metadata extraction. `book-model::metadata_isbn` validates whole values regardless of the declared scheme, permitting ISBN whitespace/hyphens and explicit ISBN wrappers. EPUB parsing checks OPF property text/attributes and the bounded NCX head. UUID/date/URL/filename substrings are excluded. This crate re-exports the validator for compatibility.

The client adapter is `client/backend/src/app/subject_pipeline.rs`. It batches network stages and short background database writes, runs page work through the native blocking/browser range-source worker, and caches ISBN responses for reuse by author enrichment. Progress and response caches are device-local SQLite data, not changes to source books. Native OCR remains bounded Tesseract; WASM has no OCR.

An ebook ASIN-to-ISBN network provider is not implemented here. The existing audiobook adapter remains ahead of general enrichment; Kindle ASINs must not be sent indiscriminately to Audible.

Descriptions use an independent client stage (`client/backend/src/app/description_enrichment.rs`) across all formats, including books whose subjects are already complete. It reuses the ISBN response cache, fills only empty database descriptions, and applies no description when available records conflict. It neither reads nor rewrites source files.
