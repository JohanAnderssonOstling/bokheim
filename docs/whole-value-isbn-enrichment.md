# Whole-value ISBN metadata enrichment

EPUB import now checks all OPF metadata values and attributes, plus the bounded NCX head, before page inspection. Validation accepts a whole ISBN with spaces/hyphens, or an explicit ISBN wrapper such as `urn:isbn:` or `urn|isbn|`. The legacy explicit `urn:uuid:isbn` wrapper remains supported. It does not search inside arbitrary values, filenames, UUIDs, subject identifiers or timestamps. ISBN-13 requires 978/979; zero placeholders are rejected.

This recovers all nine reviewed library cases in the cheap stage, including `dc:source`, `dtb:uid`, and `dtb:PrimaryID`. A valid metadata ISBN defers EPUB page extraction and OCR until its lookup completes. If that lookup provides no resolved LCC, the client continues with cached or bounded page inspection before title/author fallback. A resolved LCC completes subject lookup; BISAC alone does not. Native OCR is still a bounded fallback; WASM has no OCR. Existing page evidence is reused; completed client stages and ISBN responses are persisted.

The policy lives in `shared/publication-enrichment`, with file decoding in `shared/publication-inspection`. Client and catalog database/network scheduling stay in their respective host adapters.

Targeted release validation recovered all nine real EPUBs with empty page-evidence and OCR output, and rejected candidates in all ten actual false-positive files. The full library was not reinspected. Raw results: `tmp/books-enrichment-full-20260908/whole-value-implementation-validation.json`.
