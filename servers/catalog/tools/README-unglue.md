# Unglue.it acquisition

`ingest-unglue.py` prepares explicit category selections and downloads eligible
books after freshness review. Category links are supplied by the operator;
preparation never downloads books. No background crawl or publication is started.

```sh
python3 servers/catalog/tools/ingest-unglue.py prepare \
  'https://unglue.it/free/kw.Software/' \
  --output "$HOME/.local/share/bokheim/catalog/imports/unglue.it/software"
```

The public OPDS API supplies category discovery and pagination. Website download
pages supply all advertised freely available editions, including EPUBs and PDFs
omitted by the feed. Work-page HTML supplies bibliographic metadata,
faves, total downloads, downloads by format, identifiers, editions, descriptions,
keywords, and Open Library, LibraryThing, WorldCat, and Google Books links.
Advertised Unglue.it redirect links are retained as links; external provider
lookups are not required. Raw pages, feed responses, timestamps, delivery URLs,
byte counts and SHA-256 receipts remain beside the selection.

`selection.json` records every decision. `review.csv` provides a readable table
with publication years, faves, downloads, formats, identifiers and provider links.
Books need at least one verified fave.
Project Gutenberg origins are excluded. Discovery adds Unglue.it's `-gtbg`
filter where supported. Its live API rejects compound keyword facets, so keyword
categories use local provenance checks. Website collection/edition/download
evidence, Gutenberg/GITenberg URLs, and EPUB publisher/source/identifier metadata
block inclusion, even after manual approval. Delivery redirects are checked
before reading book bytes. Gutenberg references inside descriptions do not
alone establish origin.
Ubuntu 14.04 and earlier titles are skipped. Software and other age-sensitive
subjects published five or more years ago enter `review.json`; unknown dates
and version-specific software titles also enter review. `--review-age` adjusts
that threshold. Older fiction and classics are eligible without an age cutoff.
Missing fave counts or unavailable formats remain unresolved and cannot be
acquired merely by marking them included.

The work page's `Ungluers` tab uses the same fave count as its `Faved this Work`
label; campaign pages may show only the tab. Donation and download counts remain
separate. Older C/network programming fundamentals and general open-source,
architecture/design, conceptual and historical works can be approved after
review. Dated platform/framework/hardware guides and narrow older specialist
works can be excluded; publication age alone does not settle this review.

For reviewed books, set `decision` to `include` or `skip` in `review.json`, with
an optional `note`. Leave undecided books at `null`. Preparation preserves these
decisions when resumed. This is an operator review file: the agent should present
the titles, publication dates, links, and reasons to the user and record their
decisions, rather than asking the user to operate the script.

```sh
python3 servers/catalog/tools/ingest-unglue.py download \
  --output "$HOME/.local/share/bokheim/catalog/imports/unglue.it/software"
```

Downloads prefer EPUB and keep PDF-only books as PDFs. The current hosted-book
catalog importer supports EPUB; PDFs stay in acquisition storage and receive
source-linked catalog records through `link-manifest.json`. `downloads.json`
retains success/failure and review evidence.
Invalid book responses fail validation. Both commands resume using checked
cached evidence; use a new directory for a new selection or refreshed metadata.
Alternative advertised editions are tried after failed downloads. An operator
may also retain an independently verified publisher/author/institution mirror
with its URL, receipt, and `alternate_source` evidence. Failed downloads stay
distinct from curation exclusions.

The generated `import-manifest.json` supplies website metadata before enrichment.
All checksum-valid website ISBNs are attached to the book, including ISBNs from
multiple editions. Duplicate identifiers are removed. Edition associations,
DOIs and other identifiers, download counts and provider links remain in
`source_evidence`, which the manifest importer saves in `source_entry.metadata`
and publication carries into the catalog snapshot. Website titles, languages,
year, description, subjects and identifiers enter the enriched book record.
The source's `cn` declaration for Chinese is retained as evidence and normalized
to the BCP-47 language tag `zh` in book records.

```sh
catalog "$HOME/.local/share/bokheim/catalog" import-manifest \
  "$HOME/.local/share/bokheim/catalog/imports/unglue.it/software/import-manifest.json"
catalog "$HOME/.local/share/bokheim/catalog" link-manifest \
  "$HOME/.local/share/bokheim/catalog/imports/unglue.it/software/link-manifest.json"
```

Import and publication are separate operator steps. Preparation only considers
the supplied categories; manual decisions determine which older books download.

Checks:

```sh
python3 -m unittest discover -s servers/catalog/tests -p test_unglue.py
cargo test -p catalog manifest::tests
```
