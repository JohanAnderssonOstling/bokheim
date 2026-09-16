# Reading progress in the EPUB reader

Show elapsed progress through the book and through the current chapter, in a
small footer strip beneath the EPUB document.

## Why

Every reader surface computes reading position and almost none of them shows
it.

| Surface | Tracks | Displays |
|---|---|---|
| CBZ | `current_page` / `page_count` | Yes — `"{}/{}"`, via the search match-count component |
| PDF | `current_page` / `page_count` | Yes — page and percentage in the reader toolbar |
| EPUB | `fraction`, `location`/`total_locations`, `section`/`section_count` | No |

EPUB has the richest position data of the four and shows none of it. A reader
that cannot tell you how far through you are is missing the readout users want
most.

This plan covers **EPUB only**. PDF and mobile are left as follow-ups; both
already hold the data they would need.

## Scope decisions

- **Elapsed, not remaining.** "62% of chapter", not "38% left". Elapsed is the
  common convention and reads consistently with the book-level number beside
  it.
- **Chapter percentage, not pages left.** Pages-left would need a pagination
  API upstream — column semantics, viewport height, and a number that jumps on
  every resize. Chapter percentage is a pure glyph ratio: stable across font
  size, column width, zoom, and window size, and already computed upstream.
- **Footer, not toolbar.** The toolbar already carries history back/forward,
  Contents, Annotations, a `flex_1` title, audio, and settings. Progress is
  ambient information, not an action.
- **Very small type**, muted, in a thin strip.

## Upstream change (HtmlViewCore)

`html-view-core` is a separate repository, consumed as a git dependency
(`Cargo.toml:51`) with a local path override to the sibling checkout
(`Cargo.toml:80`). Local builds already resolve to the checkout, so this can be
developed before anything is published.

The chapter fraction is already computed and discarded:

```rust
// crates/html-view-navigate/src/navigate.rs, reading_progress()
let glyph_count = cx.doc.text().glyph_count().max(1) as f64;
self.progress_at(glyph.unwrap_or(0) as f64 / glyph_count)
```

That ratio is the position within the current document. The change is to return
it rather than only feed it to `progress_at`.

1. **`crates/html-view-types/src/lib.rs:274`** — add `doc_fraction: f32` to the
   `RendererEvent::ReadingProgress` variant.
2. **`crates/html-view-navigate/src/navigate.rs`** — have `reading_progress`
   return the document ratio alongside the existing triple.
3. **`crates/html-view-navigate/src/navigate.rs:268`** — include `doc_fraction`
   in the emitted event **and in the `emit_if_changed` dedup key**. The key is
   currently `(fraction, location, total_locations, current_doc)`; `fraction` is
   an `f32` over the whole book, so it can round to an unchanged value while the
   chapter ratio moves. Omitting it from the key would silently suppress footer
   updates inside long chapters.
4. **`crates/html-view-navigate/src/lib.rs:242`** — second match site on the
   variant; update for the new field.

Once published, bump the git rev in `Cargo.toml:51`.

## Downstream change (this repository)

1. **`apps/reader/src/epub/progress.rs`** — add `doc_fraction: f32` to
   `ProgressState`, extend `update()` to take and compare it. The existing
   tests assert that an identical report does not repaint or persist; extend
   them so a change in the chapter ratio alone still counts as movement.

2. **`apps/reader/src/epub/state.rs:399`** — the
   `RendererEvent::ReadingProgress` match arm destructures the event; take the
   new field and pass it through.

3. **`apps/components/src/reader.rs`** — add `reader_footer(theme)` and
   a small muted text child. Mirror `reader_toolbar`: `flex_none`, full width,
   `bg(theme.surface)`, but `border_t_1` instead of `border_b_1`. Type at
   roughly `11/16 rem` in `theme.text_muted`.

4. **`apps/reader/src/epub/render.rs`** — render the footer as the last
   child of the reader page, below the split, so it spans the sidebar as the
   toolbar does.

5. **`apps/reader/src/invalidation.rs`** — add a `ReaderFooter`
   component.

## Invalidation

The footer repaints far more often than the document — it changes on every page
turn, whereas the document layout does not. The toolbar already solved this by
being its own entity with its own model and equality-gated `set_model`
(`toolbar.rs`), so that a repaint there cannot pull the reader's whole render
along with it.

The footer must follow the same pattern: its own entity, its own small model
pushed in by the reader, its own `Component` for invalidation. Rendering it
inline in `ReaderView::render` from `self.progress` would repaint the entire
reader on every page turn and undo the split the toolbar exists to maintain.

## Copy

**`12% of book · 62% of chapter`**

Both percentages carry a word. Two bare percentages side by side do not say
which is which, and the strip is small enough that the labels cost little.

**No chapter number.** `display_title()` (`epub/state.rs:817`) already composes
`"{book_title} • {chapter}"` from `self.toc.active_title()`, and the toolbar
renders it. A `Chapter 4 of 21` in the footer would duplicate that, and would
derive from `section` — the spine index, which counts covers, front matter and
copyright pages as documents. The two chapter identities would disagree on
screen. The TOC title in the toolbar is the better source and it is already
displayed.

**Before the first report**, render the strip with empty text rather than
showing `0% of book`. `ProgressState::default` is a valid position, but a
resumed book would flash zero before the saved position restores. Rendering the
empty strip from the start keeps the layout from shifting.

## Kobo

Show the footer, at full `theme.text` contrast rather than `theme.text_muted`.

`request_reader_page_repaint(content_top)` sets a horizontal split
(`GPUI-Fork/crates/gpui_kobo/src/platform.rs:1199-1206`): above `split_y` is
presented as the toolbar region, below it runs to the bottom of the frame and
receives the fast reader waveform. A footer therefore lands in the lower region
and refreshes together with the page text on every turn, which is the correct
behaviour — there is no ghosting problem to design around.

The problem on Kobo is legibility, not refresh: small muted grey on a greyscale
panel is hard to read. Full-contrast text solves it, and vertical space is not
scarce on a Kobo panel.

## Note: the toolbar height is a magic number in three repositories

`58.0` appears in `components/src/reader.rs` (`reader_toolbar`),
`epub/render.rs` (`READER_TOOLBAR_HEIGHT`), `epub/state.rs`
(`request_reader_page_repaint(px(58.0))`), and
`GPUI-Fork/crates/gpui_kobo/src/platform.rs:1297`. This plan does not change
that, but the Kobo split geometry silently depends on the value matching, so
anyone altering toolbar height must update all four.

## Verification

- `cargo check` in both repositories.
- Extend the existing `progress.rs` tests for the new field.
- Manual: open an EPUB, page forward, confirm the chapter percentage advances
  smoothly and resets at each chapter boundary; change font size and column
  width and confirm both percentages stay put.
