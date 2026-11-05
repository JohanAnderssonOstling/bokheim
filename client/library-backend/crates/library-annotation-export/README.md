# Local annotation export workflow

This service exports the annotated books in one library. It has no UI and does
not make network requests. `LibraryClient` exposes the annotation snapshot and one export call. Native Settings and library-switcher menus share the dialog
in `apps/browser/src/universal/annotation_export.rs`.

The dialog offers Markdown, Foliate JSON, Readest JSON, and writing into existing PDFs. It has
one editable destination field, initially including the library name, and always
uses `Replace` for existing export files. File review, overwrite choices, format
descriptions, and a pre-export file-count footer are intentionally omitted.
Export runs once and reports per-file failures. The dialog can close while the
accepted operation finishes, and completed exports can open their output folder. The file-writing dialog is native-only; the browser build retains
the portable snapshot/writer APIs without showing unsupported filesystem actions.

The application stores a device-local default export folder. Its initial value
is the platform Documents folder plus `Bokheim annotations`. Use
`app.default_annotation_export_folder()` and
`app.set_default_annotation_export_folder(path)` to read or change it.
`library_backend::annotation_export_destination(base, library_name)` appends a
portable library folder name. The resulting destination is independently editable.

## Calling the workflow

1. Build `AnnotationExportOptions` with a final destination, selected formats,
   and a conflict policy (`KeepBoth`, `Replace`, or `Skip`). The safe default
   policy is `KeepBoth`.
2. Call `library.export_annotations(options)`. The result contains each actual
   output path, success, skip, or failure. One bad book does not prevent the other
   destinations from completing. Closing the dialog leaves an accepted export
   running to completion.

For callers that already have a snapshot, the portable `plan` function exposes
filenames. Non-PDF destinations can be changed with `options.overrides`; PDF
outputs always refer to existing local placements.

Execution takes a fresh, consistent database snapshot. Changes to annotations
made after that snapshot appear in a later export. Database transactions never
span filesystem work. Large file parsing and writing run off the library actor.

## Output

Recurring annotations are exported once, by their originating book, at the
passage where they were explicitly created. Chapter, book, and library scope
do not move that passage or expand the export into every matching occurrence.
Inline notes inherited from another book never enter the receiving book's
export. The interoperable highlight uses the original location and the latest shared
note and formatting.

Each Markdown file is named `Title.md` and follows its placement's library folder
structure. Distinct books with the same title receive numbered filenames. YAML
frontmatter contains bibliographic metadata, library/book IDs, known canonical
and local content hashes, source format, and an export timestamp. Values use
JSON-compatible YAML flow scalars and collections, preserving quotes and line
breaks. TOC ancestors become headings; highlights become multiline blockquotes
in italics, with the user's note immediately below in ordinary Markdown and an
`[Book title — Chapter](bokheim://open?…)` source link underneath. PDF source
labels use the page number. The separate `Copy link` actions in the
annotation list and text selection UI use this same formatter; selection copies
omit the note and do not create an annotation. Ordinary `Copy Markdown` and
Ctrl/Cmd+Shift+C retain the renderer's Markdown copy without a source link.
Quotes are escaped as literal text and keep their paragraph
breaks. Source links contain the configured library and stable book identities,
the annotation ID when applicable, and a fallback EPUB/PDF location, so deleted
or unsynchronized annotations can still resolve to their passage. Opening a link
requires that library and book on the receiving device. Annotations without a
known TOC location are retained under `Other annotations`.

Foliate output is `Title.foliate.json`, using its
[JSON import contract](https://github.com/johnfactotum/foliate/blob/gtk4/src/annotations.js).
It includes the EPUB package's original identifier when the local book is
available, falling back to the first `dc:identifier` if the package's
`unique-identifier` reference is missing or broken. ASCII whitespace is stripped
and collapsed as in Foliate. If no nonempty identifier exists, it uses
`foliate:` plus the MD5 of the first 10,000,000 source bytes, matching
[Foliate's fallback](https://github.com/johnfactotum/foliate/blob/gtk4/src/book-viewer.js).
This export identifier never replaces the library book ID or canonical hash.
Annotation locations are full publication CFIs without Bokheim's
storage envelope. Older reader-index CFIs are converted using the actual
publication map, including filtered spine items; the conversion never edits or
synchronizes the saved annotation. A legacy location without local EPUB bytes
is reported as a failure instead of exporting an incorrect address. Foliate may
ask for identifier confirmation when the source identifier is unavailable.

Readest output is `Title.readest.json`, using its version 1
[annotation import contract](https://github.com/readest/readest/blob/main/apps/readest-app/src/services/annotation/providers/readest.ts).
It exports EPUB highlights and notes, retaining annotation IDs, full publication
CFIs, RGB colors, supported styles, and millisecond timestamps. The optional
Readest MD5 hashes are omitted; Bokheim identifiers and content hashes are not
interchangeable with them. Import the JSON into the open EPUB through Readest's
annotation import dialog. Legacy CFIs receive the same local-only conversion as
Foliate. Full-CFI exports work without local EPUB bytes. PDF rectangle anchors
cannot be represented as Readest's text-layer CFIs, matching the EPUB scope of
our existing Readest annotation importer. Strikethrough annotations are exported
as underlines for Readest; the saved annotation retains its original style.

PDF output reconciles Bokheim annotations in the existing local file. Native
annotations belonging to other readers are retained. Repeated exports replace
Bokheim marks, and deleted marks are removed. Geometry handles inherited crop
boxes and page rotation; appearance streams make marks visible outside Bokheim.
The PDF retains its source identity and known canonical hash. A recovered,
unconfirmed provenance hint is preserved rather than erased.

PDF writes take an exclusive book lease, stage a neighboring file, and atomically
replace it. Missing, busy, encrypted, or read-only PDFs produce per-file failures. There is no automatic
download. Metadata-only Markdown export still works when library files are
unavailable.

## Local ownership

New Markdown/Foliate/Readest files must be outside the library, including destinations
that use symlinks into it. They never pass through import or upload machinery.
The workflow updates a PDF's local hash, fingerprint, and hash aliases. An
already uploaded canonical revision remains unchanged. Before a first upload,
the pending upload follows the new local bytes, so the first uploaded revision
includes annotations already written locally. This is the existing canonical
identity policy; no separate annotation-export upload is created.

PDFs run before Markdown when both are selected, so frontmatter records the
resulting local PDF hash. Failures leave incomplete temporary files unpublished. Existing output files are only replaced when the caller
explicitly selects `Replace`.

## Preserving external annotations

Replacing Foliate or Readest output updates only annotations explicitly identified
as Bokheim exports and present in the library's annotation history. External
annotations remain in the destination unchanged and never enter the library or
its sync queue. Bookmarks, other native note types, and reader settings are retained.
If an external annotation uses the same Foliate location or Readest ID as an
exported annotation, that destination fails without being replaced.

Export does not inspect running reader processes or repeatedly reread destinations
to detect concurrent external edits. Reader-library import remains the explicit
way to bring external annotations into Bokheim. PDF export likewise leaves foreign
marks in place and updates only Bokheim-owned marks.
