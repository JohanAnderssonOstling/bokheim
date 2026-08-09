# Authoritative Text Shaping Plan

## Objective

Make one shaping result authoritative for HTML line breaking, interaction geometry, and painting. Remove the current full-document exact-line refinement loop and the duplicate shaping performed when visible lines are painted.

This is a general HTML renderer change. It must improve both desktop and Kobo without adding Kobo-specific behavior to the renderer.

## Baseline

The Kobo profile from 2026-08-03 measured the following for the tested EPUB chapter:

- Book open to first presented reader frame: 6,170 ms.
- Initial pipeline update: 2,352 ms.
- Initial document shaping: 870 ms.
- Initial exact-line refinement: 1,013 ms.
- First viewport relayout: 2,299 ms.
- Exact-line refinement inside that relayout: 2,125 ms.
- Refinement result in both cases: `geometry_changed=false`.
- Pipeline retained memory: approximately 3.7 to 3.9 MB.

The immediate preference replay was already removed by passing `RendererInitialConfig` into the first pipeline update. The remaining duplication is inside text shaping and viewport layout.

## Current problem

The current renderer has multiple geometry authorities:

1. `shape_glyph_run` produces glyph metrics and a `ShapedTextRun` for logical single-style runs.
2. Layout chooses line boundaries from the resulting scalar advances and cluster boundaries.
3. `refine_exact_line_geometry` shapes every chosen line through the backend again.
4. Refinement writes the new advances into `ExactLineGeometry` and may repeat layout up to four times.
5. `prepare_visible_shaped_lines` shapes visible text again to create backend `TextRunId` resources for painting.

In the GPUI backend, `shape_text_run` internally creates a native shaped line, converts it to geometry, and truncates the retained native run. The native resource is reconstructed later for painting.

## Required invariants

- Layout and paint must use geometry produced by the same font backend.
- Line breaking must never split a shaping cluster.
- Caret placement, selection, hit testing, links, annotations, and painted glyph positions must agree.
- Font fallback, ligatures, kerning, bidirectional text, and complex scripts must remain correct.
- Letter and word spacing must affect line breaking exactly once.
- Justification and alignment must be post-break placement and must not feed back into intrinsic shaping widths.
- Shaping resources must participate in the existing begin, commit, and rollback transaction.
- A width-only relayout must reuse document shaping.
- Painting an unchanged page must not invoke the text shaper.

## Target model

Introduce an authoritative shaped-run record owned by `ShapedDocument`. The final names can change, but the record needs to represent:

```rust
struct AuthoritativeShapedRun {
    source_range: Range<u32>,
    style_key: ShapingStyleKey,
    backend_run: TextRunId,
    caret_stops: Arc<[f32]>,
    advances: Arc<[f32]>,
    cluster_boundaries: Arc<[bool]>,
}
```

The backend run identifier is meaningful only for the `GlyphShaper` that produced it. Its lifetime must therefore be committed or rolled back together with the shaped document cache.

Line layout should retain lightweight fragments that reference these records:

```rust
struct ShapedRunFragment {
    run: AuthoritativeShapedRunId,
    source_range: Range<u32>,
    origin_x: f32,
    placements: FragmentPlacements,
}
```

Painting should consume these fragments instead of requesting another shape operation.

## Phase 1: Correctness corpus and comparison metrics

Add fixtures that exercise:

- Plain Latin text.
- Kerning pairs such as `AV`.
- Standard and discretionary ligatures.
- Combining marks and emoji sequences.
- Arabic joining across potential line boundaries.
- Devanagari or another complex cluster-forming script.
- Bidirectional paragraphs.
- Font fallback within one logical run.
- Adjacent DOM spans with identical shaping styles.
- Adjacent spans with different colors but identical shaping geometry.
- Font size, family, weight, and slant boundaries.
- Letter spacing, word spacing, tabs, and justification.
- Inline images and atomic inline elements.
- Superscript and baseline shifts.

Record comparison counters before changing behavior:

- Logical runs shaped.
- Lines refined.
- Lines whose intrinsic geometry changed.
- Lines whose cluster boundaries changed.
- Backend cache hits and misses.
- Native run resources retained.
- Shape time, layout time, and paint-time shaping time.

Completion gate: the corpus reproduces the existing renderer output and can report where refinement changes geometry.

## Phase 2: Retain backend run resources

Extend `ShapedTextRun` or replace it with a richer result that includes:

- A persistent backend run identifier.
- Caret stops.
- Cluster boundaries.
- Intrinsic advances.

Change `GpuiTextCache::shape_text_run` so it does not truncate the native run it just created. Retained runs must be owned by the document-shaping transaction rather than by the per-frame run store.

Split the GPUI run stores into:

- Document-shaped resources retained across frames and width-only relayouts.
- Ephemeral line resources required only for exceptional boundary reshaping or layout placements.

Completion gate: an unchanged page can paint retained logical runs without invoking GPUI text layout again.

## Phase 3: Normalize shaping runs

Build shaping runs from geometry-affecting style rather than DOM boundaries.

Merge adjacent text when these properties match:

- Resolved font family.
- Font size.
- Weight.
- Slant.
- Language and direction when available.
- Font feature and variation settings when supported.
- Text transformation state.

Do not split shaping merely for color, underline, strikethrough, annotation, selection, or other paint-only state. Preserve paint spans separately.

Completion gate: visually identical adjacent spans produce one shaping request while retaining their distinct paint spans.

## Phase 4: Use authoritative geometry for line breaking

Make the inline token builder consume exact advances and cluster boundaries from authoritative shaped runs.

- Break opportunities that bisect a cluster must be rejected.
- Intrinsic shaping advances must be the only font geometry used by the breaker.
- Letter and word spacing must be applied in the layout layer once.
- Justification expansion must be added only after a line boundary is selected.

Remove the preliminary per-character geometry as a line-breaking authority where run shaping is supported. Keep an explicit fallback only for backends that reject run shaping.

Completion gate: the line breaker and the retained run geometry agree without copying line measurements back into `ShapedTextGeometry`.

## Phase 5: Paint run fragments directly

Extend the painter contract with a retained-run fragment operation. The exact API should carry:

- Backend run identifier.
- Source or cluster range.
- Fragment origin.
- Paint spans.
- Layout placements that do not affect intrinsic shaping.

GPUI should paint a fragment by reusing cached glyphs. It may slice or reference the existing immutable glyph allocation rather than clone and reshape it.

For scripts where line-edge context changes shaping, reshape only the affected boundary fragment. Cache it by shaping revision, run identity, fragment range, and boundary context. Interior fragments must reuse their authoritative run.

Completion gate: `prepare_visible_shaped_lines` no longer shapes ordinary visible lines.

## Phase 6: Remove convergence refinement

After line breaking, interaction geometry, and paint all use authoritative runs:

- Remove `ExactLineGeometry`.
- Remove `refine_exact_line_geometry`.
- Remove the four-pass convergence loop.
- Remove copying backend measurements back into scalar geometry.
- Remove redundant visible-line shaping paths.
- Retain only explicit exceptional boundary-fragment shaping.

Completion gate: the correctness corpus passes with zero full-document refinement calls.

## Phase 7: Incremental layout and viewport reuse

Width-only relayout should:

- Reuse parsed, styled, prepared, and authoritative shaped stages.
- Recompute inline token placement and line boundaries.
- Reuse retained backend run resources.
- Materialize only newly visible or boundary-sensitive line fragments.

The initial renderer configuration should eventually include the actual content viewport separately from preferred column width. Preferred column width remains a user setting; the initial effective layout width is the real content rectangle after persistent reader chrome such as the TOC.

Completion gate: changing between the `332px` TOC viewport and `632px` full reader viewport performs no document shaping and no full-document exact refinement.

## Performance targets

On the current Kobo test book:

- Initial exact-refinement time: 0 ms.
- Width-change exact-refinement time: 0 ms.
- Paint-time shaping for an unchanged page: 0 ms.
- Initial pipeline update: under 1,500 ms.
- Viewport-only relayout: under 250 ms.
- Book open to first reader frame: under 4,000 ms before further platform work.
- TOC open or close response: under 500 ms.

Desktop correctness has priority over these thresholds. A performance improvement that changes line wrapping or interaction geometry is not acceptable.

## Expected file areas

- `HtmlRenderer/crates/html-layout/src/shaping.rs`
- `HtmlRenderer/crates/html-layout/src/stages.rs`
- `HtmlRenderer/crates/html-layout/src/layout/`
- `HtmlRenderer/crates/html-view-core/src/renderer/interaction.rs`
- `HtmlRenderer/crates/html-view-core/src/session.rs`
- `HtmlRenderer/crates/html-view-gpui/src/text.rs`
- `HtmlRenderer/crates/html-view-gpui/src/paint.rs`
- Renderer typography and integration tests

## Risks

- Backend run identifiers can become stale if transaction ownership is incorrect.
- Slicing a shaped run at an unsafe cluster or script boundary can corrupt text.
- Bidirectional caret positions cannot be reconstructed from scalar advances alone.
- Justification can accidentally be counted both as intrinsic width and placement.
- Retaining native glyph runs increases memory use.
- Cached fragments must include every geometry-affecting style and shaping revision.

## Rollout strategy

Keep the current refinement path temporarily as a comparison oracle, not as the final fallback. During development, compute both paths for the corpus and report geometry differences. Switch production rendering only after the authoritative path matches. Remove the oracle and convergence code when the completion gates are met.

