# PDFium range loading and bibliographic inspection

PDF-capable Bokheim builds now use the same PDFium character geometry for reading and ISBN/LCC page inspection. The first and last 20 pages are inspected without rendering. Each accepted page remains separate, retains its physical page number, and is capped at 128 KiB of text. Existing copyright/cataloguing checks and ISBN checksum validation still apply.

The PDFium WASM wrapper retains a seekable source instead of calling `read_to_end`. Its source is served by the existing worker range reader; PDFium still allocates parsing/rendering memory and copies requested blocks between heaps. This removes mandatory full-source buffering, not all PDF memory use.

The vendored `pdfium-render` 0.9.3 patch enables reader loading on WASM, installs the read trampoline for the PDFium instance, retains each reader for document lifetime, and frees temporary access descriptors. Its character-box pointer order is fixed centrally, replacing the UI-only JavaScript adapter. See `vendor/pdfium-render/LOCAL-PATCHES.md`.

`lopdf` is used for embedded Info/XMP metadata only. If it cannot read the optional metadata, PDFium can still supply page evidence. The inspection crate's `pdfium` feature is enabled by the backend's `pdf` feature and the OPDS cache. Minimal builds without PDF support retain embedded metadata inspection. No book files are rewritten, and this change does not automatically reimport existing database entries.

## Verification

- Release browser reader test: a virtual 64 MiB PDF backed by a tiny fixture, with short reads and a strict 1 MiB total-read budget. Rendered both pages, searched/selected text, reused sessions, and tested cancellation, repeated disposal, initialization failure and recovery.
- Production CPU-worker browser test: its first PDF inspection request lazily initializes PDFium, recovers ISBN 0192854577 from positioned copyright text, does not merge the next printing-number line, retries a failed download, and repeats cleanly. Test: `client/backend/scripts/test-pdf-inspection.mjs` (uses the same bundle configuration as the PDF-thumbnail test).
- Actual local files: Gandhi 0192854577, Atheism 0192804243, UX for Developers 9781484242261 and 9781484242278.
- Optimized release native PDF-reader and publication-inspection tests; metadata-only inspection tests also pass.

Run PDFium inspection probes with:

```sh
cargo run --release -p publication-inspection --features pdfium --example inspect_local -- '<book.pdf>'
```

Browser reader test uses `apps/web-gpui/scripts/test-pdf-worker.sh`, which now builds release artifacts. Set `PLAYWRIGHT_MODULE` and `CHROMIUM_PATH` if the tools are not installed in the default locations.
