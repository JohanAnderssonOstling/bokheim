# Literature placement cleanup

Literary works, traditions, collections, criticism and literary writing now
have routes exclusively within Literature. Youth and Comics retain their
existing routes, as requested. This is a local curation change; no signed
taxonomy release or deployment was performed.

The cleanup reparented **40 existing subjects** and added **162 subjects** to
separate literary scopes from mixed language categories. Existing subject IDs
were preserved. The curated graph grew from 15,672 to 15,834 subjects.

## Reviewed scope

The audit examined the complete curated graph, literary BISAC selectors,
literature DDC selectors, and the recorded P-class literature/poetry headings
in the local LC reference. The manifest pins **228 literary scopes**, including
complete Cutter-ended intervals and the primary-school literature teaching
range. Source record IDs and original captions are recorded with the changes.

| Area | Result |
|---|---|
| Afro-Asiatic | Egyptian, Arabic, Hebrew, Akkadian, Sumerian, Coptic, Syriac, Yiddish and other recorded literary scopes are under Literature. Broad DDC 892/893 remain on the stable Afro-Asiatic subject; the narrower Hebrew author interval has its own child. |
| Indo-Iranian and Caucasian | Literary scopes are separated from grammar and philology, including Sanskrit, Persian, Hindi, Urdu, Armenian, Georgian and the documented regional language subdivisions. |
| Celtic and Romance | Irish-language, Welsh, Cornish, Breton, Romanian, Catalan, Occitan, Raeto-Romance and related literary intervals have literary destinations. |
| Germanic, Classical and Slavic | Hidden Latin, Gothic, Old English, Frisian and German literary intervals, plus the remaining Slavic/Baltic ranges, resolve within Literature. The Italian numeric umbrella is materialized from its recorded boundaries. |
| Uralic and Basque | Finnish, Hungarian and Basque literary subjects retain their existing IDs and move to Literature. Finnish/Hungarian fiction BISAC selectors move off the mixed language parents. Other documented Uralic literary scopes have separate subjects. |
| Asian, Pacific and Indigenous American | Literature subranges within broad language-family subjects have separate literary destinations, including complete source-recorded Cutter-ended scopes. |
| Literary studies and practice | Folk literature, literary biography, literary bibliographies, literary philosophy, Bible-as-literature study, writing fiction/nonfiction/poetry, music-related literary works and film/literature studies have literary routes. |

The word “literature” also described disciplinary writings rather than literary
works in several old headings. Music textbooks, technical documentation and
religious writings retain their disciplinary scopes with clearer labels.
Thematic subjects such as literary language, literary tourism, literature
depicted in art and screen genres retain their own disciplinary meaning.

Two boundary corrections were necessary during the audit:

- The Miao-Yao umbrella incorrectly ended at `PL407495`. Reference record
  390453 documents `PL4070..PL4074.95`. Correcting the decimal prevents unrelated
  language and literature codes from resolving to Miao-Yao.
- The Finnic umbrella `PH91.A1..PH98.5` combined language and literature, and
  its Cutter specificity outranked the separate literary interval. The
  language interval now ends at the last documented language class, `PH97`
  (dictionaries). `PH98..PH98.5` retains its documented literary boundaries.
  The complete original mixed heading remains an atomic selector.

The pinned Hebrew source-heading route follows its unchanged concept ID 13168
into Literature. Matcher version 144 refreshes derived book projections;
the taxonomy content revision also changes with the graph and selectors.

## Verification

- **642** literary boundary and midpoint probes pass using the rebuilt
  production matcher. Every reviewed literary scope resolves exclusively
  within Literature.
- **73,872** before/after classification probes cover every stored selector,
  structured range endpoints, incoming complete ranges, numeric midpoints and
  classifications from frozen installed catalog release **223**.
- **964** probes change destination; **zero** lose matches and **zero** gain
  previously absent matches. All changed destinations are within Literature
  except three synthetic probes involving the erroneous Miao-Yao upper bound,
  which now resolve to the broad Language fallback.
- Every stored literary BISAC and literature DDC selector has only Literature
  routes. Language-study controls and Youth/Comics controls pass.
- All **1,217** subjects reachable from the protected Youth and Comics roots
  retain their rows, selectors and all parent edges. No existing ID is removed.
- Migration replay matches the applied database exactly. SQLite integrity,
  foreign keys, cycle checks and duplicate sibling checks pass. Row comparison
  rejects edits outside the manifest.
- The subject-projection suite passes: **36 unit tests and four integration
  tests**. The specific-classification regression verifies that general
  Language and Literature fallbacks disappear when a supported specific
  literary classification exists.
- All **six** subject-browsing tests pass, including catalog sections,
  multiple routes and changes to taxonomy edges.

These coverage results apply to the recorded probes and frozen catalog. The
full Open Library classification cache was unavailable. The standalone viewer
was regenerated with `--without-usage` so historical counts are not displayed
as current counts.

## Reproducible artifacts

- [Reviewed migration manifest](../literature-cleanup-20261014.json)
- [Validation counts and database hashes](20261014-literature-cleanup-validation.json)
- [Every changed classification probe](20261014-literature-classification-changes.tsv)
- Migration tool: `servers/taxonomy/curation/apply-literature-cleanup.py`.
- Regression tests: `shared/subject-projection/tests/literature_placement.rs`.

The original curated snapshot and frozen catalog are retained in
`~/.cache/bokheim-taxonomy-curation/literature-20261014/`. The reference/master
taxonomies were not edited. Publication release identifiers are unchanged.

Preview or replay against the original snapshot (the migration deliberately
rejects an already-applied or incompatible baseline):

```sh
python3 servers/taxonomy/curation/apply-literature-cleanup.py BEFORE.sqlite3
python3 servers/taxonomy/curation/apply-literature-cleanup.py BEFORE.sqlite3 \
  --apply --backup ORIGINAL.sqlite3
cargo test -p subject-projection
python3 servers/taxonomy/curation/generate-viewer.py --without-usage
```
