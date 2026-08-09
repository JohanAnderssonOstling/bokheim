# Plan 06 — Structural pseudo-classes and `::first-letter` / `::first-line`

## Goal
Support the selector features common in real EPUB CSS that currently never
match: structural pseudo-classes (`:first-child`, `:last-child`, `:nth-child`,
`:not`, …) and the typographic pseudo-elements `::first-letter` / `::first-line`
(drop caps, styled opening lines).

## Current behavior (the gap)
In `src/style/selectors_dom.rs::component_matches_dom`:
- Only `:link`/`:any-link` are handled (`pseudo_matches_dom:150`); every other
  `NonTSPseudoClass` and all structural `Component::Nth`/`Negation` fall through
  the final `_ => false` (`:107`).
- `Component::PseudoElement(_) => false` (`:105`) — so `::first-letter` etc.
  never match.

Drop caps via `::first-letter { float:left }` won't work even though floats are
now implemented, because the pseudo-element box is never generated.

## Split: 6a (matcher-only, do first) and 6b (box/line synthesis)

---

## 6a — Structural pseudo-classes
Pure matching logic; needs sibling position info from `Document`.

### Components to handle (parcel_selectors)
- `Component::Nth(NthSelectorData { nth_type, a, b, .. })`:
  - `Child` → match against index among **element** siblings (1-based).
  - `LastChild` → index from the end.
  - `OnlyChild` → single element sibling.
  - `Col`/typed variants below.
- `Component::Nth` with `OfType` semantics (`:nth-of-type`, `:first-of-type`,
  `:last-of-type`, `:only-of-type`) → index among siblings **with the same tag**.
- `Component::Negation(selectors)` → `!any(selector matches)`; reuse
  `match_from_position_dom` for each inner selector.
- `Component::Is(list)` / `Component::Where(list)` → `any(matches)` (Where has
  zero specificity — already handled by lightningcss `specificity()` so no extra
  work).
- `:root` → element is the document root.

### Document support needed
Add cheap helpers (most already partially exist via `previous_element_sibling`):
- `element_sibling_index(node) -> usize` (count preceding element siblings).
- `element_sibling_count(parent) -> usize`.
- of-type variants taking a tag.
An+B test: `(index - b) % a == 0 && (index - b) / a >= 0` with `a==0` →
`index == b`.

### Steps
1. `selectors_dom.rs`: implement the `Nth`/`Negation`/`Is`/`Where` arms in
   `component_matches_dom` (or a sibling `structural_matches`).
2. `document/dom.rs`: add the sibling-index/count helpers.
3. The bloom-filter pre-check (`selector_might_match_with_filter`) is unaffected
   (structural pseudos carry no tag/class/id ancestor constraints).

### Tests
- `p:first-child` matches only the first `<p>` among element siblings.
- `li:nth-child(2n)` matches even items.
- `tr:nth-child(odd)` striping.
- `p:not(.note)` excludes class `note`.

---

## 6b — `::first-letter` / `::first-line`
Larger: needs synthetic style + box/line generation, not just matching.

### Matching
- In `component_matches_dom`, recognize `Component::PseudoElement(pe)` and, when
  the selector otherwise matches the originating element, record the
  pseudo-element style separately rather than folding it into the element style.
- During `compute_style_for_dom_element`, accumulate a second `WorkingStyle` for
  matched `::first-letter` / `::first-line` rules and stash it on the element
  (e.g. `Document::dom_pseudo_styles: FxHashMap<(DomNodeId, Pseudo), StyleIndices>`).

### `::first-letter` generation (box_builder)
- When building inline content for a block whose element has a `::first-letter`
  style, split the first typographic letter (leading punctuation + first letter
  per CSS) of the first text run into its **own inline box** carrying the merged
  (element + pseudo) style.
- Float support already exists, so `::first-letter { float:left; font-size:3em }`
  then flows as a drop cap through the float path.

### `::first-line` generation
- Cannot be a box (the line is only known after breaking). Apply at
  `emit_lines`/`write_line_fragments` time: for the first emitted line of the
  block, override the per-glyph style (color, font-style, weight,
  letter/word-spacing, small-caps) with the `::first-line` style.
- Only the properties that apply to `::first-line` (font/color/spacing/decoration)
  — not box-model — so this is a glyph-style overlay on line 0.

### Steps
1. Style plumbing: pseudo-style map keyed by `(node, pseudo)`.
2. `::first-letter`: first-letter span detection + synthetic inline box in
   `box_builder.rs::build_inline_children` / `process_text_node`.
3. `::first-line`: first-line style overlay in `inline.rs` emit phase.

### Tests
- `p::first-letter { font-size:3em; float:left }` → first glyph laid out as a
  larger floated box; following text wraps beside it.
- `p::first-line { font-variant:small-caps }` → only line 0 small-caps.

## Effort / risk
6a: medium, self-contained, high value (striping, first-child spacing). 6b:
larger — `::first-letter` touches box generation; `::first-line` touches inline
emission. Recommend shipping 6a first, then `::first-letter` (drop caps payoff),
then `::first-line`.
