# Bounded Figure Deferral

**Status:** Potential feature; not scheduled

## Goal

Reduce conspicuous blank space when a figure does not fit at the bottom of a
reader column. The paginator may defer the complete figure to the next column
and allow a small, safe amount of following prose to fill the remainder.

This is a book-mode pagination feature. It must not change continuous HTML
layout or web-compatible rendering.

## Non-negotiable invariants

- A `<figure>` and every associated `<figcaption>` form one placement unit.
- Captions must never be detached from their figure or reordered within it.
- Authored `break-before`, `break-after`, and `break-inside` rules remain
  authoritative.
- Forward and backward pagination must produce identical page boundaries and
  placements.
- Text selection, hit testing, links, annotations, decorations, and navigation
  anchors must follow the displayed positions.
- Deferral must not alter the document's continuous layout geometry.
- Web-compatible mode must retain ordinary document-order pagination.

## Deliberately bounded first version

- Defer at most one figure at a time.
- Defer it by at most one column.
- Admit only ordinary following prose into the vacated space.
- Do not move headings, tables, lists, code blocks, sidebars, forced-break
  content, semantic keep ranges, or another figure ahead of the deferred unit.
- If no eligible prose fits, preserve the current behavior and leave the
  remainder blank.
- Treat an oversized figure using the existing split fallback rather than
  repeatedly deferring it.

## Proposed paginator model

Introduce a pagination-only placement unit for the full figure bounds and a
single-slot deferred-placement queue. This is not a new layout box and does not
belong in `html-layout`.

When the paginator reaches a figure:

1. Resolve the complete figure range, including captions and decorations.
2. If it fits in the current column, place it normally.
3. If it fits in a fresh column but not the current remainder, tentatively
   enqueue it for the next column.
4. Scan following siblings for eligible prose that fits before the current
   column boundary.
5. Stop scanning at the first structural or authored boundary.
6. Record an explicit display-order projection for the admitted prose and the
   deferred figure.
7. Place the complete deferred figure at the next column top.

The resulting display sequence may be:

```text
document order: prose A -> figure + caption -> prose B
display order:  prose A -> prose B -> figure + caption
```

The projection must be stored with prepared-page data so reverse navigation
does not attempt to infer the reordering independently.

## Required implementation work

1. Add a semantic figure placement-unit descriptor in the paginator.
2. Add the bounded deferred-placement state to page preparation.
3. Represent projected display order independently of monotonic document Y.
4. Teach painting, selection, hit testing, links, annotations, and decoration
   projection to consume the recorded placement.
5. Reuse the same recorded placement for backward pagination and page caching.
6. Add rejection rules for ineligible fill content and authored breaks.
7. Add diagnostics showing why a figure was placed, deferred, or left in flow.

## Tests

- Compact figure and caption fit normally.
- Figure moves intact when no safe prose follows.
- One eligible paragraph fills the remainder before a deferred figure.
- A multiline caption remains attached.
- Top and bottom captions retain their authored order.
- Headings, tables, lists, code, sidebars, keeps, and forced breaks stop fill.
- A second figure is not allowed to pass the first.
- An oversized figure uses the existing split fallback.
- Selection, links, and annotation overlays match projected positions.
- Forward, backward, cached, and uncached pagination are identical.
- Resizing invalidates and deterministically rebuilds the projection.
- Web-compatible mode remains unchanged.
- An interactive GPUI comparison makes the whitespace reduction inspectable.

## Main risks

- Current pagination relies heavily on monotonic document-Y ordering.
- Reordered paint positions can desynchronize interaction geometry unless the
  projection is authoritative for every consumer.
- Over-deferral can make reading order surprising even when the visual result
  appears denser.
- A broadly generalized solution would become a float-placement engine; this
  proposal intentionally stops before that boundary.

## Acceptance criterion

Ship only if the bounded implementation visibly improves illustrated books
without producing unstable pages or surprising reading order. Otherwise retain
the current, predictable figure keep behavior.
