# Bilinguator acquisition

`ingest-bilinguator.py` inventories the visible catalog and sitemap, preserving
source responses and checksum receipts. It surveys each available translation's
copyright declaration using advertised bilingual pages. Reversed language pairs
are acquired once. Install `beautifulsoup4` in the Python environment first.

```sh
python3 servers/catalog/tools/ingest-bilinguator.py survey --output ACQUISITION_DIR
python3 servers/catalog/tools/ingest-bilinguator.py download --output ACQUISITION_DIR
catalog CURATION_DIR import-manifest ACQUISITION_DIR/import-manifest.json
```

The acquisition context is the operator-confirmed noncommercial Bokheim catalog.
CC0, CC BY, CC BY-SA, CC BY-NC, CC BY-NC-SA and CC BY-ND declarations retain their
specific versions and conditions. GFDL 1.3 is recognized only with the explicit
versioned source link. Public-domain declarations require a stated death year
more than 70 complete years before the current year. A life-plus-50 label alone
does not establish expiry for the Swedish catalog. These are checks on source
declarations, not an independent worldwide copyright determination.

`selection.json` records eligible pairs; `held.json` retains missing, ambiguous,
unsupported, and unexpired declarations. Unavailable works remain in
`survey.json`. No license is inferred for an unidentified translator. A Project
Gutenberg label alone does not establish non-US expiry or identify any additional
permission terms; it remains held for an individual source review.

The operator excludes works about Esperanto from this collection. The reviewed
work `manifesto_de_prago` (Prague Manifesto) is held for every language pairing,
including pairs without Esperanto. Editions of any work containing Esperanto
(`eo` or `epo`, including regional tags) are also held. Pairings of those works
in other languages remain eligible. Source evidence and previously acquired
files are retained.

`excluded-catalog-languages.json` freezes the additional 32 languages excluded
by the operator on 2026-10-30 because they lacked filter display names. An edition
is held if either language matches a listed code or ISO alias, including regional
tags. Supported-language pairings of the same work remain eligible. This list is
explicit; changing the UI's name table does not silently change acquisition.

An individually researched expiry review can be supplied to `download` with
`--rights-review REVIEW_JSON`. Each review identifies the work/language, named
source credit, death year, supporting authority URLs, and exact source-text
SHA-256. The tool checks that the text and credit match and that more than 70
complete years have elapsed. Such reviews remain visibly distinguished from
Bilinguator's own declarations in provenance and the downloadable credits page.
Use `--workers 1` through `--workers 4` for bounded acquisition concurrency.

EPUBs are generated through the source's advertised download operation. The
acquirer verifies archive integrity, work identity and both language tags before
retaining them in `originals/`. Copies in `attributed/` add a separate credits page
containing both source notices, source and license links, and full license texts.
Author metadata uses the source's English work-page credit; source creator names
remain in provenance. Supplied translator credits are also included as EPUB
contributors. All original resources other than the package metadata remain byte
identical, including literary text, images, styles, navigation and original
credits. The added page is in the reading order. Each translation retains its
own license; a collection is not assigned one blanket license.

Catalog descriptions also include the credits and license links. Full source
evidence and source/derivative checksums are supplied to `import-manifest` for
curation provenance. The published snapshot retains normalized source
identities; the download carries its complete notices independently of that
snapshot. `downloads.json` records individual outcomes. Reruns verify retained
file checksums and reuse successful acquisitions. The tool stops when less than
1 GiB of local storage remains. Import, publication and activation are separate
operations; the acquisition tool never modifies an existing catalog snapshot.


Checks:

```sh
python3 -m unittest discover -s servers/catalog/tests -p test_bilinguator.py
```
