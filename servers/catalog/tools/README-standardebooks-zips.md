# Import Standard Ebooks subject ZIPs

```sh
cargo build --release -p catalog
python3 servers/catalog/tools/ingest-standardebooks-zips.py \
  --catalog ~/.local/share/bokheim/catalog \
  --downloads ~/Hem/Downloads \
  --output ~/.local/share/bokheim/catalog/audits/standardebooks-zips-20261014 \
  --binary "$PWD/target/release/catalog"
```

The importer reads EPUB members of every ZIP whose name contains `se-ebooks`
directly into memory. It deduplicates overlapping subject archives by SHA-256,
then sends each distinct book through the catalog's normal inspection and
metadata enrichment. Only the permanent content-addressed catalog EPUB is
written; there is no extracted inbox or temporary EPUB tree.

Acquisition manifests accept optional `archive_member`, `download`, and
`preserve_covers` fields. `download: true` fetches `file_url` into memory and
uses `file` as the inspection filename, without writing a temporary EPUB.
Standard Ebooks downloads must contain the requested canonical source URL in
their package identifiers before they can be imported. Pass an explicit list
to the import command with `--manifest acquisition-manifest.json`.
Standard Ebooks' advertised download URL first returns a download-started page;
use the EPUB URL from its automatic redirect, with `?source=download`, as
`file_url` when preparing a direct-download manifest.

With cover preservation enabled, enrichment requests no cover and any
returned cover is ignored. Exact title/author replacements reuse existing
catalog artwork; new entries use their embedded artwork. The EPUB bytes remain
unchanged. Existing cover files are checked by SHA-256 before and after local
publication.

The output directory contains a verified compressed SQLite backup, inventory,
source archive/member observations, resumable progress, logs, replacement plan,
and receipt. Replacements use `deduplicate-catalog.py`, restricted to imported
exact format/title/displayed-author groups and matching source volume scope.
Every incoming edition, every previously imported Standard Ebooks edition, and
every Public Domain Library edition is retained.
Only pre-existing entries from other sources are removed. Other formats, unrelated duplicates,
separate source volumes, and original book files are retained. A successful run
publishes and installs a new local snapshot.

Resume an interrupted run with the same output directory. The importer stops
before starting another book if available disk space falls below 100 MiB.
It also saves progress and stops immediately on HTTP 429, without retrying that
request or starting another download. Resume only after the source's limit has
reset. Standard Ebooks' anonymous download handler checks both a 30-second
window and a six-hour window; its current long-window threshold is 100 downloads.

`standardebooks-popularity.py --pages DIRECTORY --output ranking.json` validates
a cached, complete popularity listing fetched with `per-page=48`. It records
canonical URLs, titles, authors, page hashes, observation time, and ordinal ranks.
Pass that file to the import command with `--popularity-only ranking.json` after
the import completes to retain the observations and publish a ranked snapshot.
The source's download counts are not available in the listing. The compact
catalog uses reciprocal rank as an internal positive sorting/aggregation score;
it never labels the ordinal as a download count. Source order is Public Domain
Library, Standard Ebooks, then the other sources in their established order.

`audit-standardebooks-missing.py --catalog CATALOG --ranking ranking.json
--output REPORT_DIRECTORY` compares the finished catalog with those same pages
and writes JSON and CSV reports. Exact source URLs establish edition presence;
normalized title/author matches identify other catalog EPUB editions separately.

Published snapshots also store the resolved, most specific subject assignments
in `book_concept`, alongside their taxonomy/matcher revision in
`subject_projection`. Original subject metadata remains in `book_subject`.
An assigned ancestor is omitted only when another assignment descends from it;
assignments on separate branches remain. Clients use these stored rows when the
revision matches, and rebuild them once when the taxonomy changes. Library book
projections apply the same rule when writing assignments. Parent counts include
descendants through the existing subject tree; browsing does not prune rows.

Reviewed replacements can retain all original classification evidence while
excluding selected navigation placements. Append a subject with source
`manual:subject:exclude`, authority `unified`, and the exact taxonomy path in
`code`. Add the replacement through the existing `manual:subject` convention.
The exclusion applies only to that concept on that book, including when an LCC
code also resolves to other branches. Exclusions are applied during projection
and publication; the resulting `book_concept` rows are saved in the snapshot.
Ancestor counts still include any retained descendants of an excluded parent.

Adding a hosted catalog book preserves the full curated record, including manual
assignments and exclusions, in the library's synchronized enrichment answer.
Direct file imports and filesystem discovery also reuse that record when the
actual content hash and media format match a hosted catalog edition. The answer
is saved in the import transaction before enrichment can run. Other files follow
ordinary enrichment; source-linked entries are not matched by title and author.
Reimporting an already answered library book retains its immutable answer.

For a catalog-only build against a frozen, previously published taxonomy, set
`BOKHEIM_BUILD_TAXONOMY_SQLITE` to its absolute SQLite path. The default remains
the repository taxonomy. The override receives the same integrity, relationship,
and matcher validation as the default and avoids incorporating concurrent
taxonomy edits into a subject-only catalog release.

Existing personal libraries can be refreshed without starting the UI:

```sh
cargo run --release -p library-database --example refresh-subject-assignments -- /path/to/library.db
```
