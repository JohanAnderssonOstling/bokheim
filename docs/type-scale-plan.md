# Type scale

Decisions from the book-detail-page review. Nothing here is applied to
`apps/components/src/tokens.rs` yet.

## The anchor

Body text *is* the interface font size. `TEXT_BODY = 1rem`, and the default
preference moves from 16 to 18 so the page is pixel-identical to today.

Today no step is 1rem: the reader picks 16 and body renders at 18
(`TEXT_MD` = 1.125rem). The 15% uplift exists because Libertinus has a 0.429em
x-height against Noto Sans' 0.536em — a property of the font, so it belongs in
the default value of the preference, not smeared across six constants.

## The steps

Typed as `Rems`, so the unit is stated once, `/ 16.0` leaves 77 call sites, and
`px(TEXT_BODY)` stops compiling.

| Token | Rems | at 18 | step | carries |
|---|---|---|---|---|
| `TEXT_XS` | 0.667 | 12 | — | author line, contributor role, chapter duration, position line, subject chips |
| `TEXT_SMALL` | 0.80 | 14.4 | 1.20 | card blurb, button labels, supporting text that is read rather than scanned |
| `TEXT_BODY` | 1.00 | 18 | 1.25 | descriptions, card titles, breadcrumbs |
| `TEXT_H2` | 1.25 | 22.5 | 1.25 | block headings, shelf headings, a subtitle (muted) |
| `TEXT_H1` | 1.50 | 27 | 1.20 | what the page is |
| `TEXT_DISPLAY` | 1.875 | 33.75 | 1.25 | the page is about exactly one thing and this is it |

Line heights belong to the roles, not to call sites: display/h1 1.15, h2 1.25,
body 1.5, small 1.4, xs 1.0.

### Why both small steps, and why these values

Keeping two was decided against the book card, which sets title / blurb / author
at three distinct sizes; one `TEXT_SMALL` would flatten the bottom two.

The values move because six steps have to fit under the file's own rule that no
two steps are closer than a fifth. Today `TEXT_XS` 13 and `TEXT_SM` 15 are
**1.15** apart — the smallest step in the scale, on the pair carrying 56 of 82
uses, which is the exact defect the current scale's comment rejects the previous
scale for. At 0.667 / 0.80 the pair is 1.20 apart and `TEXT_SMALL` to
`TEXT_BODY` is 1.25.

The cost: `TEXT_XS` renders 12px at the default rather than 13. It lands on the
same size as `NAV_RAIL_LABEL_SIZE`, and it is only ever the quietest line in a
block. Preserving 13 exactly would mean 0.72, which is 1.11 from `TEXT_SMALL` —
worse than the 1.15 being fixed.

## Not steps on the scale

| Token | Rems | why |
|---|---|---|
| `NAV_RAIL_LABEL_SIZE` | 0.67 | measured against the small-cap height (0.483em), not the lowercase scale |
| `CONTROL_GLYPH_SIZE` | 1.25 | new. `reader_square_button` and the settings stepper borrow `TEXT_LG` to size a glyph, so changing a heading resizes two reader controls |

## Consequences for the `*_REM` geometry tokens

They are multiples of the same rem, so moving the default 16 → 18 means
restating each one. **Restate, do not divide** — these are floors and preferred
widths, and the nearest clean number is as correct as 1.125ths of the old one.

| Token | now | exact | take | px |
|---|---|---|---|---|
| `NAV_RAIL_SIZE_REM` | 5 | 4.444 | **4.5** | 81 (was 80) |
| `BOTTOM_NAV_HEIGHT_REM` | 4 | 3.556 | **3.5** | 63 (was 64) |
| `BOOK_CARD_MINIMUM_WIDTH_REM` | 10 | 8.889 | **9** | 162 (was 160) |
| `BOOK_CARD_PREFERRED_WIDTH_REM` | 12 | 10.667 | **10.75** | 193.5 (was 192) |
| `BOOK_CARD_MAXIMUM_WIDTH_REM` | 14 | 12.444 | **12.5** | 225 (was 224) |
| `BOOK_DESCRIPTION_OVERLAY_WIDTH_REM` | 34 | 30.22 | **30** | 540 (was 544) |

## Role mapping, old to new

| Old | New |
|---|---|
| `TEXT_XS` 13 | `TEXT_XS` 0.667 |
| `TEXT_SM` 15 | `TEXT_SMALL` 0.80 |
| `TEXT_MD` 18 | `TEXT_BODY` 1.00 |
| `TEXT_LG` 22 (headings) | `TEXT_H2` |
| `TEXT_LG` 22 (glyphs in `reader_square_button`, settings stepper) | `CONTROL_GLYPH_SIZE` |
| `TITLE_MD` 27 | `TEXT_H1` |
| `browser_page_title`'s 22 | `TEXT_H1` — it is a page title set one step below every other page title |
| `TITLE_LG` 34 | `TEXT_DISPLAY` |

The aliases (`BOOK_CARD_TITLE_SIZE`, `BOOK_CARD_AUTHOR_SIZE`,
`BOOK_CARD_DESCRIPTION_SIZE`, `BOOK_DESCRIPTION_OVERLAY_SIZE`,
`CAROUSEL_TITLE_SIZE`, `SHELF_SUBJECT_SIZE`) mostly stop earning their place
once the steps are named by role, and can name the role directly.
