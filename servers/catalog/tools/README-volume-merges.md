# Combining EPUB volumes

The candidate survey also reports `component_groups` for volumes split into
numbered slices or essays. A complete slice range reconstructs its printed
volume; it does not establish completeness of the larger work. Essay numbers
without an explicit total still require a contents review. Files containing
multiple essays are counted once as files and separately as numbered essays.
An omitted heading represented by a styled paragraph or a plain text div can
be added with an explicit `plain_heading: true` navigation review. Its label
must match the original text exactly; containers and invented labels are rejected.
Printed inline headings can use `inline_heading: true` for span, italic, bold,
emphasis or strong elements. Check them against the original printed contents;
the exact source text and styling remain intact. Reviews can also retain an
original illustrator byline with the MARC `ill` role.
An explicit `navigation_target_repairs` review can repair an incorrect source
TOC link while retaining its original label. Bind the old entry to its source
SHA-256, `href`, and `label`; provide a `target_href`, exact `original_heading`,
and reason. The replacement must resolve to a unique original heading anchor
inside that source volume. Adding an anchor through a resource repair preserves
the original text and styling. Repeated NCX links to one target share a
`playOrder` while keeping distinct entry IDs and all labels.

`merge-epubs.py` creates a new EPUB3 from an explicitly selected, ordered set of
volumes. Titles alone do not establish series membership or completeness: check
the work, volume numbers, credited translators, language, and edition before
selecting files. The tool requires matching authors, contributors, and languages,
and rejects duplicate volumes. It does not merge a named series automatically.

Use `survey-volume-candidates.py --catalog DIR --output REPORT_JSON` to discover
hosted candidates across all catalog providers. It reads original package titles,
recognizes punctuation before stated totals and spelled-out volume numbers, and
keeps contributor and author variations together for edition review. Those
variations must still be explicitly verified and approved in the merger's review
JSON. Candidate status never authorizes a merge: a contiguous range without
extent evidence remains unverified. Bundled works and excerpts are reported
separately, existing EPUB2 inputs are included, and external audiobook volumes are not
scanned. Different languages remain separate groups.
Slash extents such as `Volume 3/19` are recognized. Michelet's date ranges before
the volume marker are grouped under *Histoire de France*, while the separately
edited *Moyen Âge* series remains a different candidate group.
English ordinal forms such as `Part the First` and `Part the Second` are also
recognized. Descriptive uses of “part,” such as “the part borne by,” remain
outside numbered-part discovery. A pair still needs work and extent review;
the presence of first and second parts alone does not establish completeness.
Numbered books and book ranges, including `Books 01 to 08` and `Books 37 to
the End`, are also discovered. Their ranges require explicit review of the
surviving text, epitomes, edition and translators; they do not imply that lost
ancient books are available. Book numbers remain distinct from volume numbers.
The abbreviation `v.` and German `Buch` markers are recognized too. Duplicate
selection retains these source scopes even when enrichment gives several
volumes the same displayed title.
Parenthesized positions such as `(1 de 2)` are supported without a volume
word. When a bare package title omits its volume, explicit original titlepage
labels can supply a candidate marker. A titlepage marker can also number a
larger series, so it still needs work and extent review. Duplicate selection
preserves the separately named Saunders essay collections as well.
Swedish ordinal parts such as `Första delen` and `Senare delen` are recognized.
Bare terminal numbers in Swedish books are provisional candidates only; they
require membership review and never supply a guessed total. Royal names and
independent sequels can also contain numbers, so inspect the actual work.
Compound Swedish numbering such as `VI:2` identifies a part of volume VI;
it does not establish that the entire sixth volume is present.

Gutenberg's Warren Commission hearing titles include changing positions in a
larger collection, such as `(02 of 26)`. Discovery groups those entries under
their shared hearings title and retains their stated 15-volume extent; the
separate exhibit series is outside that group. Original package titles remain
in the report, and completeness and edition still require review.

An explicit `--review REVIEW_JSON` can permit contributor variation within a
verified edition. It must identify the work, explain the edition evidence, and
bind every ordered source to its SHA-256 and exact creator, contributor, and
language lists. Authors and languages must still match. All differing contributor
credits and their roles are retained.
Edited works without an author field are supported when their nonempty editor
and contributor credits match across volumes. Creditless inputs remain rejected.

```sh
python3 servers/catalog/tools/merge-epubs.py volume-1.epub volume-2.epub \
  --title 'A Work' --output a-work.epub \
  --report merge-report.json
```

Use the original work title for the combined edition and a matching filename,
without volume numbers, ranges, or “combined volumes” suffixes. Volume identities
remain in component provenance and original source records, not the generated
edition's title or filename. Untitled sections retain their original § labels;
do not invent descriptive section titles.

The source TOCs are concatenated in volume order, without added volume headings.
Each source's internal chapter hierarchy remains intact. EPUB3 navigation and a
compatible NCX are generated from the same entries. Page navigation retains
volume labels to distinguish repeated page numbers. Reading order concatenates
the original spines after the combined contents page.

Each source occupies its own resource directory, preventing collisions between
chapter names, images, CSS, and footnote IDs. Content and resources remain byte
identical, including original title pages, navigation files, and Gutenberg
license text. Package IDs and manifest references are remapped; exactly one
navigation document and one primary cover image are designated. The result has
its own identifier. Source identifiers are recorded as provenance rather than
assigned as the combined edition's ISBN.

The merger checks manifest resources, reading order, local HTML/SVG/CSS links,
and fragment targets before exposing the output. Existing files cannot be
overwritten. The first valid source `dcterms:modified` timestamp is preserved in
volume order, so unchanged inputs produce identical output without a new clock
timestamp. Dates that refine another metadata element are skipped. If no source
has a valid publication-level timestamp, supply `--modified` explicitly. An
explicit timestamp override is included in the revision identity. Use EPUBCheck
for full conformance validation.

Some source TOCs omit explicit numbered sections in the text. Pass
`--expand-numbered-sections` to add paragraphs beginning `§ N.` when their numbers
form a unique increasing sequence. Entries stay within their original chapter,
in reading order. Schopenhauer's sections appear beneath the relevant Book,
alongside its Aspect heading. Missing anchors are added as `id` attributes
only: text, images, CSS, and existing links remain unchanged. The report lists
every added section and modified resource. This option does not invent headings
from arbitrary paragraphs or fill gaps in the source numbering.

Existing local EPUB2 books can also be merged directly into EPUB3. Their original
NCX supplies the exact navigation labels and hierarchy; content files stay byte
identical. Original author roles, sort names, and cover metadata are converted to
EPUB3 metadata. An intrinsic modification date or original packaging timestamp
is reused in UTC when EPUB2 has no `dcterms:modified`; the current clock and file
mtime are never substituted. This does not change Gutenberg intake: downloads
must still use the page's advertised EPUB3 files.

Supported inputs are ordinary EPUB2 or EPUB3 publications with one rendition and
left-to-right reading order. Encrypted or signed resources, fixed layout,
synchronized audio overlays, archive-root links, `xml:base`, and `srcset` require
a separate merge strategy and are rejected.

For retained catalog books, import the validated result through the actual Rust
reader inspector and optionally publish a local snapshot:

```sh
python3 servers/catalog/tools/import-merged-epub.py \
  --catalog /path/to/catalog --binary target/release/catalog \
  --report merge-report.json --publish
```

Inspection uses an isolated temporary inbox. The importer unions original raw
subjects, languages, and publishers, preserves contributor identities, and
retains a year only when all components agree. Each component's record, ISBNs,
source pages, and download observations are retained in `source_entry.metadata`.
Descriptions are retained too. Identical descriptions appear once; distinct
descriptions are associated with their original component labels. The pass puts
the retained catalog descriptions in the generated EPUB's `dc:description` and
the combined catalog record. Direct merges retain source package descriptions
unless `--description` supplies the retained catalog text. No replacement
summary is invented, and adding descriptions preserves the first valid source
modification timestamp.
Original records and files remain in the catalog. Publication installs a local
snapshot; it does not deploy files to the public catalog origin.
The importer creates its curation backup in a temporary directory beside the
requested backup, checks its integrity and catalog schema, and exposes it only
after completion. Existing unreadable or empty backups are rejected, so a retry
cannot silently reuse a partial backup left by a storage failure.
Use `--replace-merged PRIOR_HASH` to replace a previous generated edition of the
same ordered source volumes. Its EPUB file is retained; original volumes and
editions from other sources cannot be replaced through this option.
An explicitly identified replacement can also reuse archived originals when
their files remain retained. The importer binds them to the prior edition's
exact ordered hashes and file digests, reuses its archived component records,
and preserves its archival and alternate-source provenance. Changed source
bytes and concurrent provenance changes are rejected.

For existing combined entries, `retain-merged-descriptions.py --catalog DIR
--output AUDIT_DIR` restores descriptions from retained component records.
`--pages-manifest MANIFEST_JSON` also allows recovery of missing component
summaries from cached Gutenberg pages. It retains hidden continuation text and
the site's summary attribution, removes page controls, preserves existing
curated descriptions and all original component records, and writes a full
backup and restoration receipt. This changes catalog metadata; existing EPUB
assets and their historical merge reports remain intact. Publish and install a
new local snapshot afterward.

Reviewed source XHTML repairs are also supported through the review JSON. Each
repair must identify the source EPUB and member SHA-256, explain the correction,
and specify exact replacement text and occurrence counts. Repairs apply only to
the generated copy. The tool rejects any repair that changes document text.
The explicit `restore_empty_document_title` exception may fill an empty HTML
head title only with that source's exact package title; every other text node
must remain unchanged. The report distinguishes this metadata restoration from
repairs that preserve all text, and records preservation of reading text.
The report retains every applied correction. Run EPUBCheck before importing;
the catalog pass leaves outputs with errors or warnings unimported.
Reviewed `flattened_navigation` entries can also identify nested original
titlepage, volume, or editor wrappers. Their original targets and labels must be
unique; only those wrappers are removed, promoting their original children
without renaming chapters or changing the remaining hierarchy.
Reviewed `lifted_navigation` entries retain a heading's original link as a leaf
and promote its original children beside it. This can correct source heading
levels that accidentally place the book's chapters beneath a preface, without
losing that preface link or changing chapter labels and their internal hierarchy.
An entry cannot be selected for both removal and retention through lifting.

For an input whose EPUB navigation lists pages only, a reviewed
`heading_navigation` list can instead reuse original chapter headings or
subtitles. Every entry binds a source SHA-256, reading-order XHTML member,
`target_id`, exact whitespace-normalized `label`, and review `reason`. The merger
verifies that each target is a unique original heading paragraph or heading
element in the body, that its full text exactly matches the label, and that the
entries follow reading order. Plain text-only `div` headings are supported too,
preserving their exact text and original markup. Containers with descendant
elements remain rejected. Missing anchors can be added through the existing
text-preserving repair mechanism. Original navigation files and source text
remain retained; the generated navigation and NCX use the reviewed headings.

The initial verified set was Schopenhauer's *The World as Will and Idea*, volumes
1–3, Gutenberg IDs 38427, 40097, and 40868. Its combined edition has 22 top-level
TOC entries, 59 spine entries, and 85 manifest resources. EPUBCheck 5.4.0 returned
zero errors or warnings. It was installed in local catalog release 132.
The revised section-expanded edition replaced the generated book in release
133. It adds 70 explicit section entries from volume 1, for 162 TOC entries in
total. The four Books have 16, 12, 23, and 19 direct section entries respectively.
The source jumps from § 24 to § 26; no § 25 heading was invented. The revised
edition also passed EPUBCheck 5.4.0 without errors or warnings.

The 2026-10-13 catalog pass combined 73 source volumes into 27 books and installed
local release 141. This includes a refresh of the earlier Schopenhauer merge.
All imported merged EPUBs passed EPUBCheck 5.4.0 with zero errors or warnings.
Original source records, EPUB files, identifiers, and download observations were
verified against the pre-pass backup.

At the user's request, release 143 removed the 73 original volume entries from
the visible catalog, keeping all 27 validated combined books. The original EPUBs
and covers remain in storage and are hard-linked into
`volume-merge-pass-20261013/archived-originals/`. Its `archive.json` preserves
complete original book records and TOCs, source entries, download observations,
and author credits. The prior installed snapshot is retained there too.
Source evidence remains in curation with a null standalone book reference and
in the combined edition's component metadata. The combined evidence records
that originals are archived rather than still available as catalog entries.
Verification confirmed that all other hosted and external book records are
unchanged; the installed catalog contains 5,110 hosted books. Unmerged sets
remain visible.

The follow-up review merged another 80 volumes and parts into 25 catalog entries
in local release 146, including two separately verified Gutenberg editions of
Gibbon. Selection also consulted complete source EPUB titles (which retain
parenthesized totals omitted from some curated titles), prefaces, and library
edition records. Spinoza's four-part treatise was grouped across its two source
title spellings after checking translation and chapter coverage. Editor-only
Scottish Highlands volumes exposed and corrected an unnecessary author-field
requirement. All 25 outputs passed EPUBCheck with zero errors or warnings.
The 80 originals were archived with full records, navigation, source evidence,
download observations, and unchanged assets, then removed from the visible
catalog. The installed catalog has 5,055 hosted books; unrelated records and
external entries were verified unchanged against the preceding snapshot.

Audit directories are `volume-merge-pass-20261013-followup/`,
`volume-merge-pass-20261013-supplement/`,
`volume-merge-pass-20261013-editor-only/`, and
`volume-merge-pass-20261013-gibbon/`. The follow-up directory contains the
aggregate `final-verification.json`. Full curation backups for these later passes
are retained losslessly compressed, with decompressed hashes checked before
removing the uncompressed copies. Incomplete sets, unverified extents, isolated
excerpts, and the earlier invalid Reformation source remain separate.

The subsequent completeness review resolved all 27 previously unknown groups.
Eighteen already had gaps in their source volume numbering. Of the nine
contiguous groups, seven proved incomplete and two complete: Hallam's Ward,
Lock edition of *Introduction to the Literature of Europe* (two volumes) and
Taine's *The French Revolution* (three volumes in the Durand/Rom edition).
Their five originals were combined into two validated books and archived;
release 147 contains 5,052 hosted books. All other records were verified
unchanged. The review is saved in
`catalog/imports/volume-completeness-review-20261013/completeness-report.json`.
The resulting remaining review has 53 groups missing volumes, two excerpt
groups, one duplicate-number grouping, and the previously invalid source set.
Rousseau's two files are Books I and II of twelve, despite their “Volume” titles;
the two Guanzi files contain only the first ten chapters. This review does not
claim to verify isolated single-volume entries outside the 27 reviewed groups.

The missing-volume acquisition pass reviewed all 55 incomplete sets against
Gutenberg's official CSV catalog and retained book pages. It acquired only the
EPUB3 files advertised on those pages, using the official mirrors, and added
197 new files while reusing seven already retained. Four alternative-edition
candidates were excluded. Each of the 204 retained acquisitions has its actual
book-page download count, observation date, raw subjects, LoC codes, page ISBNs,
and original-publication/release-year evidence. Counts use `book_page` scope;
temporary importer bookshelf context is confined to isolated inspection and
never published as discovery evidence. No popularity ordinals were introduced.

After checking original edition extents and reading order, 38 complete sets
were merged, passed EPUBCheck without errors or warnings, and replaced 260
standalone entries. All originals and full records were archived. Source
timestamps, source/component ISBNs, raw subjects and counts were verified again
against the installed snapshot; release 154 was installed after the reader fix
described below. The audit directories are `missing-volumes-20261013/` and
`missing-volume-merges-20261013/`; the former contains
`completeness-after-acquisition.json` and `final-verification.json`.

Fourteen reviewed sets still have missing material not found in Gutenberg,
including the posthumous Supplement absent from the four retained principal
parts of the Summa. Two newly complete sets remain separate because their
contributor credits differ: Graetz's *History of the Jews* and Merle d'Aubigné's
Calvin Reformation history. Lucian's complete four-volume set also remains
separate: source volume IV has invalid zero-valued `colspan` attributes, and
EPUBCheck reproduced the merged-file errors in that original. These three
complete sets were not represented as failed or incomplete acquisitions.

Edition review distinguishes actual source structure from misleading volume
labels. Renaissance in Italy's last EPUB covers volumes VI–VII. Maspero's
volumes I–IX and Rappoport's X–XII continuation form separate authored works.
Parkman's source titlepages put the two Half-Century volumes before the bundled
Montcalm and Wolfe, although their Gutenberg page part numbers are reversed.
Montaigne's English text and Rousseau's Confessions are complete digital parts,
not nineteen or twelve original print volumes. Paine's four-part Gutenberg
selection does not purport to contain every item from Conway's printed
collected edition. These qualifications remain in the merge provenance.

Refreshing the app exposed an existing reader failure after catalog
deduplication: differing Litteraturbanken observations could refer to the same
retained book and prevented the entire snapshot from loading. The reader now
selects the strongest existing combined observation for display, using counts
and constituent percentiles to resolve ties deterministically. It keeps that
observation intact and preserves all source evidence; it does not sum counts.
All 18 catalog reader tests passed, including a regression checking reversed
source row order and unchanged snapshot evidence. A separate check through the
rebuilt Rust reader activated the actual release 154 with 7,277 catalog entries
and confirmed the merged Baxter, Montaigne and Rousseau books were present.

The expanded review included isolated volume entries and corrected Gutenberg
IDs previously attached to a different physical volume by title-based
deduplication. It imported 314 additional page-advertised EPUB3 files, retaining
actual download counts and page metadata. Distinct volume and excerpt scopes
now remain separate during deduplication even when enrichment gives them the
same title; Public Domain Library preference still applies within a scope.

This review produced 82 more validated combined books and archived 258 replaced
original entries. It includes the formerly blocked Graetz, Calvin Reformation,
Lucian and Lindsay sets. Explicit edition reviews preserve differing prefatory
and translation credits. Reviewed generated-copy repairs fix invalid source
table markup, generator metadata and a nested pagination anchor. One empty HTML
head title is restored from its exact source package title, with reading text
unchanged. All 82 imported outputs passed EPUBCheck without errors or warnings.
The Ancient Volcanoes of Great Britain remains separate because its source
EPUB lacks linked high-resolution map resources. Incomplete and unverified sets
also remain separate.

Descriptions were restored on all 96 previously generated catalog entries.
Cached Gutenberg pages supplied 139 missing component summaries; their wording
and attribution are retained with page hashes and source identities. The 78
merges generated after this fix also embed their retained descriptions in the
EPUB package, matching the catalog records. Historical EPUB files are unchanged.
Two newly acquired source pages have no summary; none is invented. Local release
162 contains 4,933 hosted and 2,423 external entries. The actual Rust reader
activated all 7,356 entries and the app was refreshed.

Audit directories for this review are `additional-missing-volumes-20261013/`,
`reviewed-volume-merges-20261013/`, `additional-volume-merges-20261013/`,
`additional-volume-markup-repairs-20261013/`,
`additional-volume-final-repairs-20261013/`, and
`description-retention-20261013/`. The acquisition directory contains the
aggregate `final-verification.json`. Original records, navigation, source
observations, identifiers and assets remain in the corresponding archives.

Selection required matching work titles, authors, contributors, and languages,
with volume numbers covering the stated total from titles or source frontmatter.
Isolated volume excerpts, already bundled editions, duplicate volume numbers,
missing volumes, and unknown totals were excluded. One otherwise complete set,
*A History of the Reformation*, was skipped because its invalid table markup was
also found in the original source EPUB. Franklin's sections restart their
numbering, so its original TOC was retained.

The resumable batch runner takes an explicitly reviewed selection with `groups`,
each containing a `title`, eligibility `status`, and ordered `volumes` with catalog
hashes. Reports and frozen survey code are retained in
`catalog/imports/volume-merge-pass-20261013/`.

```sh
python3 servers/catalog/tools/run-volume-merge-pass.py \
  --selection selection.json --catalog /path/to/catalog --output /path/to/audit \
  --binary target/release/catalog --epubcheck /path/to/epubcheck.jar
```

Batch inspection shares one pre-pass backup. EPUBCheck errors are quarantined
only when reproduced at the same resource in a source EPUB; other errors remain
failures. Installed snapshots link to their validated immutable release instead
of requiring a second full database copy.

```sh
python3 -m unittest discover -s servers/catalog/tests -p test_merge_epubs.py -v
```

Paired named subworks can be reviewed independently of a larger collected edition. For example, Swift’s two-volume Writings on Religion and the Church occupies volumes III–IV of his Prose Works; combining that complete subwork does not claim the larger series is complete. Emerson’s First and Second Series supply the complete paired Essays, while Turpin’s separately edited school selection remains distinct. Source scope checks protect that reviewed selection during later duplicate removal. Bundled titles with repeated markers, such as “Volume I and Volume II”, are excluded from single-volume discovery.

A reviewed `added_navigation` entry can restore an omitted source heading beneath an exact original TOC parent. Bind each entry to its source SHA256, reading-order XHTML member, existing anchor, exact printed label and reason. An anchor inside a heading is supported. With `adjacent_heading: true`, an existing numbered heading may target the immediately following original title heading; the tool checks both adjacency and literal title text. This restores navigation without changing source text or inventing titles. Hume’s 1826 Treatise and Dialogues reconstruction uses this for 90 section titles and twelve dialogue parts.
