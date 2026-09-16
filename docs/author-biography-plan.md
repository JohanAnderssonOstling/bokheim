# Author biographies: Wikipedia extract with attribution

Design for the biography block on the author detail page
(`docs/author-detail-page-design.html`). Nothing is implemented yet.

## The chain already exists

`client/metadata-client/src/author_pictures.rs:57` (`author_picture`) already
walks it for portraits:

1. `OL…A` → `https://openlibrary.org/authors/{id}.json` → `remote_ids.wikidata` → QID
2. QID → `https://www.wikidata.org/wiki/Special:EntityData/{qid}.json` → entity
3. entity → image claim → Commons `imageinfo` with
   `extmetadata` (`Artist`, `Credit`, `Attribution`, `LicenseShortName`, `LicenseUrl`)

A description reuses steps 1–2 and replaces step 3:

3. entity → `sitelinks.enwiki.title` →
   `https://en.wikipedia.org/api/rest_v1/page/summary/{title}`

One request returns everything needed:

| field | use |
|---|---|
| `extract` | the biography — plain-text lead, already trimmed |
| `description` | the Wikidata one-liner, e.g. "American political scientist" |
| `titles.normalized` | the article name, for the attribution line |
| `content_urls.desktop.page` | the link the attribution must carry |
| `revision`, `timestamp` | staleness, and what a refresh compares against |
| `lang` | which wiki the text came from |

`AuthorOrigins` gains a `wikipedia` origin beside `open_library`, `wikidata`,
`commons`, `uploads`, `thumbnails`, so the whole thing stays overridable in
tests exactly as the portrait path is.

## Attribution is a licence condition, not a nicety

Wikipedia text is CC BY-SA 4.0. Reuse requires naming the source, linking the
article (its history is the author list), naming and linking the licence, and
indicating that the text was modified — taking a lead extract is a modification.
Share-alike binds any derivative text.

The page therefore states, under the prose:

> Extract from the Wikipedia article **“Robert D. Putnam”**, by its
> contributors · **CC BY-SA 4.0**

with the title linking to `content_urls.desktop.page` and the licence to
`https://creativecommons.org/licenses/by-sa/4.0/`. Both are links, not text:
an attribution that cannot be followed does not satisfy the licence.

This also does honest work for the reader — it says the prose is not the
library's, which matters when the extract is thin or oddly weighted.

## Storage

A sidecar, mirroring `AuthorPictureCredit` rather than inventing a shape:

```
author_biography(
  author_id     INTEGER PRIMARY KEY,
  text          TEXT NOT NULL,
  descriptor    TEXT,            -- the Wikidata one-liner
  source        TEXT NOT NULL,   -- 'wikipedia' | 'openlibrary'
  source_url    TEXT NOT NULL,
  source_title  TEXT,            -- article title as normalised
  lang          TEXT,
  license       TEXT NOT NULL,
  license_url   TEXT NOT NULL,
  revision      INTEGER,
  fetched_at_ms INTEGER NOT NULL
)
```

Sidecar rather than a column on `author`: this is refreshable external data
that must never touch author identity, and a licence change or a takedown has
to be able to delete it cleanly. Local-only and refetchable — not synced.

## Coverage, measured on the dumps

Sampled from `~/server_ssh` on 2026-09-12.

| | coverage |
|---|---|
| OL authors with a biography | 45,997 of 10,019,155 — **0.46%** |
| OL authors with a `photos[]` entry | **0.4%** (sample of 200k dump records) |
| OL authors with a **wikidata** remote id | **1.1%** (109,734 in the imported DB; 1.1% in the dump sample) |
| OL authors with both a photo and a wikidata id | 0.1% |
| Wikidata humans with P18 image — notable Q-ids | 66.2% |
| Wikidata humans with P18 image — mid-range Q-ids | 5.5% |
| Wikidata humans with an English description | 99.4% notable / 61.3% mid-range |
| Wikidata humans with an enwiki article | 88.6% notable / 6.5% mid-range |

**The bottleneck is not Wikidata's richness — it is the bridge to it.** Only
1.1% of Open Library authors carry a `remote_ids.wikidata`, and every
Wikidata-derived thing (P18 portrait, enwiki sitelink, descriptor, extract) is
gated behind that one field, because `author_picture` resolves the QID from the
Open Library record. A Wikidata human is very likely to have a description, and
a notable one is likely to have a portrait and an article — but only one author
in ninety can be reached from Open Library in the first place.

That also explains the portrait path: Open Library's own photo covers ~0.4% of
authors, and the Wikidata fallback adds roughly `1.1% × P18-rate`, so it
contributes a fraction of a percent rather than the 66% the notable P18 figure
suggests.

### Widening the bridge

The share already holds a local Wikidata extraction — 45.4M entities,
5,102,021 humans, with labels, aliases, descriptions, sitelinks and claims. A
name+alias index over those humans, resolved against author credits the way
`match_author_resolutions` already resolves Open Library candidates, would
bypass `remote_ids.wikidata` entirely. Filtering to humans with an authored-work
link (the extraction counts 5,509,928 such links) keeps the index to plausible
authors rather than every person in Wikidata.

This is worth doing before, not after, the biography fetch: it lifts portraits,
descriptors and extracts at the same time, and all three are currently limited
by the same 1.1%.

## Source precedence

1. **Wikipedia extract** where an enwiki sitelink exists.
2. **A non-English sitelink** where the interface language matches, or where
   English is absent — mid-range humans have 6.5% enwiki against 19.5% with some
   wiki, so this roughly triples what is reachable once a QID is known.
3. **Open Library biography** otherwise — 45,997 of them, already served by
   `servers/metadata` via `load_author_biographies` and already shipped in
   `AuthorMetadata.biography`, where the client currently drops it.

Record which source won; the attribution line differs per source, and Open
Library's text is user-edited and needs a minimum-length and sanity filter
(real rows include `"uu no.20 tahun 2003"`).

## Where the fetch lives

Beside the portrait fetch, in `metadata-client`, with the same cache-with-credits
behaviour — the portrait path already has a test named
`wikimedia_fallback_keeps_credits_offline_and_upgrades_old_misses`, which is
exactly the semantics wanted here: keep what was fetched, keep its credits
offline, upgrade a previous miss when the network returns.

Two operational notes: Wikimedia expects a descriptive `User-Agent` on API
traffic, and a library import that resolves hundreds of authors at once should
pre-populate server-side from the dumps rather than fan out to the REST API per
client.

## What this does not do

Wikidata has no biographies. It has the one-line `description` and a sitelink;
the prose is Wikipedia's. The descriptor is worth taking on its own — near
universal for notable people, and it becomes the line under the author's name
in the hero.
