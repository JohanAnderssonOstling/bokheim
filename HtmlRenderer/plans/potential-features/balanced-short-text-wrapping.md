# Balanced Short-Text Wrapping

**Status:** Potential feature; deferred

## Goal

Improve the visual shape of multiline headings and other short display text by
choosing line breaks with more even line widths. This is distinct from normal
paragraph-wide optimal wrapping: a balanced heading should avoid a very short
final line without adding unnecessary lines or distorting spacing.

## Deliberately bounded scope

- Target headings and explicitly authored `text-wrap: balance` content.
- Consider only short blocks with a small maximum line count.
- Preserve natural glyph advances, word spacing, and letter spacing.
- Never balance ordinary body paragraphs automatically.
- Do not balance preformatted text or content containing tabs.
- Treat authored `<br>` boundaries as mandatory segment boundaries.
- Fall back to existing wrapping when no valid balanced solution exists.

## Proposed implementation

Reuse the existing compact Knuth--Plass breakpoint stream rather than adding a
second tokenizer. Add a short-text objective that minimizes the variance of
occupied line widths while applying strong penalties for:

1. Increasing the line count over ordinary wrapping.
2. Producing an orphaned final word.
3. Hyphenating a heading when a nearby word-boundary solution exists.
4. Creating adjacent lines with conspicuously different widths.

The balanced mode should return only breakpoint choices. Existing fragment,
selection, hit-testing, decoration, and painting paths remain unchanged.

## CSS and reader policy

- Add typed support for `text-wrap: balance` before claiming web-compatible
  behavior.
- Authored `text-wrap` remains authoritative in every rendering mode.
- Any automatic heading balancing is a separate book-optimized reader policy
  and must be independently switchable.
- `white-space`, overflow-wrap behavior, soft hyphens, and forced breaks remain
  authoritative.

## Invariants

- Source ranges and character-to-glyph mapping do not change.
- Balancing never changes shaped glyphs or their natural advances.
- A balanced result does not use more lines than the configured bound.
- Relayout at the same width is deterministic.
- Cached and uncached layout produce identical breakpoints.
- Continuous and paginated geometry consume the same balanced lines.

## Tests

- A two-line heading avoids a one-word final line.
- A three-line heading has materially lower line-width variance.
- Balancing does not add a line merely to equalize widths.
- Explicit `<br>` boundaries are preserved.
- Soft hyphens and ordinary word boundaries retain their authored semantics.
- Tabs and preserved-space modes use the existing fallback.
- Long headings and body paragraphs are not balanced automatically.
- RTL and mixed-direction text retain logical ranges and visual order.
- Resizing recomputes deterministic balanced breakpoints.
- Selection and hit testing follow the resulting lines.

## Main risks

- Over-balancing can make headings look artificially narrow.
- A width-variance objective can choose surprising breaks unless line-count
  and hyphen penalties dominate.
- Automatically targeting semantic headings changes book-mode layout and must
  not leak into web-compatible rendering.

## Acceptance criterion

Ship only when short headings improve visibly across narrow and wide reader
columns without increasing line counts unexpectedly or changing body-text
wrapping.
