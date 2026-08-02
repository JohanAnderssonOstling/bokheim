# Link Preview Plan

Desktop-only feature: previewing the target of in-book links in a floating
popover, without navigating away or touching history.

## Scope

- **Previewable** = the link resolves inside the book.
- **Not previewable** = external links (`http(s)`, `mailto`, `tel`, `ftp`) and
  broken internal anchors.
- **Desktop only.** No `MobileApp/src-tauri` changes; no long-press path.

## How links work today (the foundation)

The machinery already exists:

- **Glyph→href map**: `Document.link_glyph_hrefs: FxHashMap<u32, u16>`
  (`document/mod.rs:175`), populated during box building.
  `link_href_for_glyph(glyph_idx)` returns the interned href.
- **Hit testing**: `hit_test_glyph(pos) -> Option<u32>` and `hit_test_image(pos)`
  (`renderer/selection.rs:305,364`).
- **Resolution + classification**: `resolve_href_target(href) -> Option<(usize, Option<String>)>`
  (`nav.rs:63`) — same-doc `#id`, cross-doc `path#frag`, bare cross-doc `path`,
  or `None` for external/unresolvable.
- **Target → position**: `jump_to_id(fragment)` (`nav.rs:188`) maps an anchor id
  to a layout `y`. The same lookup can locate target content without navigating.
- **`epub:type` is already in the DOM**: `build_dom_element` stores all
  namespaced attributes, so `epub:type="noteref"` is retrievable via
  `attr_in_any_namespace("type")` — just never read today.

A preview is: **hit-test → classify → extract target content → show popover,
without touching navigation/history.**

## Classification: internal vs external

Falls out of existing code:

```
link_is_previewable(glyph) =
    link_href_for_glyph(glyph)
        .and_then(resolve_href_target)
        .is_some()
```

No `epub:type` needed at the decision layer.

## What slice of the target to show

`epub:type` and the target node's tag pick a tight preview unit instead of a
blunt "first N lines". All cases reduce to **a vertical band of the target
document's layout**.

| Target | Preview unit |
|---|---|
| Fragment → note (`aside`/`li`, `epub:type` footnote/endnote/rearnote) | that note's box subtree |
| Fragment → heading | heading + content up to the next heading (capped) |
| Fragment → figure/table/blockquote/equation | that element's box |
| Fragment → anything else | the block containing the anchor (+ following block) |
| No fragment (whole-doc link) | that spine doc's start — title/first heading + opening paragraphs |

## Rendering approach — reuse the existing layout

Render a **clipped vertical band of the target doc's existing layout** into the
popover, rather than building a subtree extractor + mini-renderer:

1. Get the target `LoadedDocument`. Endnotes commonly live in a separate
   `notes.xhtml`, so cross-doc is the *common* case → add an **LRU cache of
   `LoadedDocument` keyed by `doc_index`** so hovering doesn't rebuild a chapter
   each time.
2. Map fragment → target DOM node → its **layout box rect** (top/bottom),
   extending the `jump_to_id` id→y lookup to also return the box bottom. Gives a
   precise band `[box_top, box_bottom]`, capped at a max height.
3. Paint that band at the reader's column width into the popover, reusing the
   existing per-line painter with a y-offset. Popover width = column width
   avoids any relayout.

Reuses the layout engine, the painter, and the anchor→position lookup.

## Trigger (desktop)

- Pointer **dwell (~400 ms)** over a link glyph → `hit_test_glyph` →
  `link_is_previewable` → show popover.
- Dismiss on pointer-leave.
- Plain click still routes to existing `handle_link_click` → navigate.

## Components & edit sites

**HtmlRenderer**
- `nav.rs` / new `renderer/preview.rs`:
  - `link_is_previewable(glyph_idx) -> bool`
  - `preview_target(glyph_idx) -> Option<PreviewTarget>` →
    `{ doc_index, band: Range<f64> }` (or `DocStart`)
  - `render_preview_band(target, width) -> <paintable>` reusing the painter
- Loaded-doc **LRU cache** (new small struct held by the renderer).
- Extend the id→node→box lookup behind `jump_to_id` to expose the target box's
  vertical extent.

**UI (`UI/src/epub_reader.rs`)**
- Pointer dwell → `hit_test_glyph` → `link_is_previewable` → request band →
  floating popover anchored near the link rect (clamped to viewport).
- Popover **"Go to ↗"** action calls the real `navigate_to_href` — the *only*
  path that pushes history; previews never touch history.
- Dismiss on pointer-leave.

## Edge cases

- **Broken internal anchor** (id not found): treat as non-previewable.
- **Target already on screen**: previewable per the rule, but suppressed as
  low-value (toggleable).
- **Preview-within-preview** (a note containing its own links): disabled in v1;
  "Go to" navigates instead.
- **Footnote backlink** (note → its referent): allowed, harmless.

## Phasing

1. Internal-link detection + LRU doc cache + band lookup + band rendering, wired
   to hover-dwell with a plain "first lines of the target box" preview.
2. Tight preview-unit selection by node type (note/figure/heading).
3. Polish: max-height with fade, "Go to ↗".
