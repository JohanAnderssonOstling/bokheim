# Wikidata identity: keys, candidate order, and what it is for

The point of matching against Wikidata is not bibliographic data — Open Library
is far larger and is the spine. It is **author identity**: a Wikidata hit yields
the author QID directly, and with it the portrait (`P18`), the descriptor, and
the Wikipedia sitelink. Everything else is gated behind
`remote_ids.wikidata`, which only 1.1% of Open Library authors carry.

## Keys, in order

1. **ISBN** — `P212` / `P957` on edition items. `wikidata_books.rs` already
   queries `SELECT book FROM isbn WHERE isbn13=?`, then `isbn_author` →
   `profile` (name, aliases, birth/death, identifiers, **image**).
2. **DOI** — `P356`. Already a first-class scheme here: `Scheme::Doi`,
   `canonical_doi()`, and PDF import extracts it from PRISM XMP
   (`publication-inspection/src/pdf.rs:273,290`).
3. **Title + author**, normalised — the last resort, for items carrying neither.
   Same shape as `edition_candidate_query` in `servers/metadata/src/identity.rs`.

An exact identifier wins on its own and needs no class check. If an ISBN or a
DOI resolves to a paper, that is a correct match and is kept — the identifier
said so.

## Candidate order: books first

Where several candidates tie, or where the match is fuzzy, rank by class:

| rank | `P31` | |
|---|---|---|
| 1 | `Q3331189` | version, edition or translation |
| 2 | `Q7725634` | literary work |
| 3 | `Q571`, `Q47461344` | book, written work |
| 4 | `Q13442814` | scholarly article — near-always DOI-bearing, so rarely reaches a fuzzy match at all |
| 5 | `Q191067` | article (journalism) |
| 6 | `Q1980247`, `Q13433827` | **chapter, encyclopedia article — parts of books** |
| 7 | everything else | |

This is a preference, not an exclusion. Papers stay in the corpus: they are
well identified, they carry authors and subjects, and a PDF library of papers
deserves enrichment as much as a shelf of books does.

**The one place class matters sharply** is fuzzy title+author matching, and the
risk is not where it first appears to be. Scholarly articles are the *safest*
of the article classes: 97% of them carry a DOI, so they are matched by
identifier and rarely reach the fuzzy path at all.

The hazards are `Q1980247` (chapter) and `Q13433827` (encyclopedia article).
Both are parts of books, both usually lack a DOI, and both have titles that
collide directly with book titles — a chapter titled "The Origins of
Totalitarianism" matching the book of that name is a false positive no paper
would produce. Restrict fuzzy matching to ranks 1–3, and exclude ranks 6–7 from
it outright, unless the local item looks like a paper itself (it has a DOI, or
it is a PDF with no ISBN).

## Coverage, measured

`~/server_ssh/wikidata-extracted-2026-08-12`, 45.4M entities, 2026-09-12.

| | count | share |
|---|---|---|
| entities with an author (`P50`) | 12,566 per 100k | 12.6% |
| — scholarly articles (`Q13442814`) | 7,201 per 100k | 57.3% of authored |
| — articles (`Q191067`) | 905 per 100k | 7.2% of authored |
| — encyclopedia articles (`Q13433827`) | 854 per 100k | 6.8% of authored |
| — chapters (`Q1980247`) | 431 per 100k | 3.4% of authored |
| — editions (`Q3331189`) | 839 per 100k | 6.7% of authored |
| — literary works (`Q7725634`) | 782 per 100k | 6.2% of authored |
| authored entities with a **DOI** | 9,199 per 100k | **73.2% of authored** |
| entities with an **ISBN** | 145,642 total | 0.32% of all |
| authored book-ish entities, extrapolated | ~700–750k | ~5× the ISBN set |
| Open Library `edition_isbn`, for scale | 37,083,822 | — |

So: Wikidata is ~0.4% of Open Library's ISBN reach, ~2% of its edition count,
and it is not a bibliographic replacement. It is an author-identity source with
three exact keys into it.

## Blocking work

`metadata-wikidata-core-20260909.sqlite.building` is 35 GB and
`wikidata-author-job-status.json` reads `"phase": "failed"` — `ingest-wikidata`
exited non-zero on 2026-09-09. The query code in `wikidata_books.rs` exists and
has nothing to query until that build completes.
