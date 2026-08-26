# Library of Congress Classification structural snapshot

The detailed development-time LCC structure is derived from the Library of
Congress MDSConnect Classification 2016 retrospective snapshot. The Library
provides the snapshot as MARC 21 UTF-8 and MARCXML at:

https://www.loc.gov/cds/products/MDSConnect-classification.html

Run the repository importer from the workspace root:

```sh
python3 scripts/import-lcc-mdsconnect.py
```

The importer verifies the upstream archive by SHA-256 and creates
`shared/subject-projection/data/lcc-mdsconnect-2016.sqlite3`. The generated database
is intentionally ignored: the compressed upstream MARCXML is about 31 MiB and
expands to about 713 MiB. It can be recreated from the public source whenever
taxonomy curation or mapping needs the complete structural records.

The snapshot includes classification-number spans, captions, caption and
summary hierarchies, number hierarchies, and table references. It is a richer
curation source than `lcc-2024-outline.csv`, but it is older. The outline
remains the small current runtime source until selected snapshot structure is
reviewed and promoted into the unified taxonomy.

MDSConnect describes these records as open-access and intended primarily for
research, education, and development. The Library of Congress should be
attributed as the source of the original records.
