# Plan 09 — Typed incremental recomputation and cache ownership

## Prerequisite

Complete Plan 08 (`08-pipeline-boundary-hardening.md`) first. This plan relies
on immutable validated phase outputs and a stable render-output contract.

## Goal

Use the phase boundaries to recompute only the stages affected by an input
change, while guaranteeing that an incremental result is identical to a clean
full rebuild for the same inputs.

The target behavior is:

| Change | Minimum required work |
|---|---|
| Paint-only setting, selection, search highlight | Paint only |
| Scale when layout coordinates are scale-independent | Paint only |
| Viewport/column width | Layout only |
| Intrinsic image dimensions | Layout only, plus image-fragment refresh |
| Font backend/database revision | Shape + layout |
| Root font size or style environment | Style + prepare + shape + layout |
| CSS content/order/media environment | Style + prepare + shape + layout |
| HTML/source content | Parse + every downstream stage |
| Document URI/resource base change | Resource resolution and the affected downstream stages |

No caller chooses stages manually. It submits a typed input change; the
pipeline computes the earliest invalidated phase and rebuilds from there.

## Current behavior

- Column-width changes already call `LaidOutDocument::relayout`, preserving
  parsing, styling, preparation, and shaping.
- Scale changes request paint only.
- Root-font-size changes reload the document and repeat source loading, parsing,
  CSS discovery, DOM construction, styling, preparation, shaping, and layout.
- Image decoding is asynchronous, but intrinsic image metadata is currently
  mixed into document construction and does not have an explicit layout-input
  revision.
- Phase timing and memory reports exist, but there is no centralized record of
  which input version produced each cached artifact.
- `PreparedDocument::shape` and `ShapedDocument::layout` consume their inputs.
  Consumption enforces phase order, but it also prevents retaining the nearest
  valid upstream artifact for later recomputation.

## Design principles

1. **Typed causes, not dirty booleans.** A boolean cannot explain what must be
   recomputed and tends to become either under-invalidating or overly broad.
2. **Earliest-invalidated-stage wins.** Multiple changes coalesce by taking the
   earliest affected stage.
3. **Cache keys describe inputs, not internal output bytes.** Do not repeatedly
   hash complete DOM/style/layout vectors.
4. **Validation occurs at phase transitions.** Cache hits do not repeat semantic
   validation.
5. **No work during paint except paint.** Rebuilding is explicit before a frame
   is prepared.
6. **Incremental and clean builds are observably equivalent.** This is a tested
   invariant, not an assumption.

## Change and invalidation types

Introduce a framework-neutral change model in `html-pipeline`:

```rust
pub enum PipelineChange {
    SourceChanged(SourceRevision),
    ResourceBaseChanged(ResourceRevision),
    StylesheetsChanged(StylesheetRevision),
    StyleEnvironmentChanged(StyleEnvironment),
    FontEnvironmentChanged(FontEnvironmentRevision),
    ImageMetricsChanged(ImageMetricsRevision),
    LayoutConstraintsChanged(LayoutConstraints),
    PaintSettingsChanged(PaintSettingsRevision),
}

pub enum EarliestStage {
    Parse,
    Style,
    Prepare,
    Shape,
    Layout,
    Paint,
    None,
}
```

`EarliestStage` is ordered. Coalescing changes chooses the minimum/earliest
stage, so a pending CSS change cannot accidentally be hidden by a later
paint-only change.

Do not expose a general `invalidate(Stage)` API to normal callers. The mapping
from change type to stage belongs to the pipeline and is exhaustively matched.

## Input keys and revisions

Use small, explicit keys:

- `SourceRevision`: URI/source identity plus content revision or content hash
  computed once when bytes are loaded.
- `StylesheetRevision`: ordered stylesheet identities and revisions, including
  inline chunks.
- `StyleEnvironment`: root font size, media environment, language/direction
  inputs, and other values that affect computed style.
- `FontEnvironmentRevision`: font database/backend generation and shaping
  options.
- `ImageMetricsRevision`: generation of intrinsic width/height metadata.
- `LayoutConstraints`: validated viewport/column width, line-height basis, and
  future pagination constraints.
- `PaintSettingsRevision`: settings that affect only presentation.

Revisions are issued by the owner of the underlying data. They must be
monotonic within a session and must not wrap into a live value. Equality checks
must be O(1).

## Cached artifact graph

Add a `PipelineSession` or `PipelineCache` owned below `html-view-core`:

```text
LoadedSource
   |
   v
ParsedHtml
   |
   v
Document + discovered resource/style references
   |
   v
StyledDocument
   |
   v
PreparedInputs
   |
   v
ShapedInputs
   |
   v
LaidOutDocument
```

Each cache slot stores:

- the artifact;
- the exact input key that produced it;
- optional timing/memory counters;
- no back-reference to UI state.

The cache exposes one operation:

```rust
fn update(
    &mut self,
    requested_inputs: PipelineInputs,
    glyph_shaper: &mut impl GlyphShaper,
) -> Result<PipelineUpdate<'_>, PipelineError>;
```

`PipelineUpdate` reports which stages ran, which artifacts were reused, and
whether navigation anchors need remapping. Renderer code does not assemble
phase objects itself.

## Milestone 9a — Make reusable phase inputs retainable

### Problem

The current consuming transitions discard the nearest upstream value. A font
environment change should reuse prepared text and box inputs, but the prepared
artifact has already been consumed by shaping.

### Design

1. Separate immutable inputs from per-run outputs:
   - `PreparedInputs`: document, styles, box tree, original character/runs;
   - `ShapedText`: shaped glyph IDs, metrics, link/anchor glyph mappings;
   - `LayoutOutput`: line geometry and paint fragments.
2. Share immutable large inputs through `Arc` or an equivalent owned backing
   object. Phase wrappers retain private handles to the same validated inputs.
3. Shaping must not overwrite the only copy of the original Unicode character
   stream. Produce a separate glyph-ID buffer or use copy-on-write storage.
4. Layout mutates only `LayoutOutput` and layout-local caches. Relayout reuses
   `PreparedInputs` and `ShapedText`.
5. Preserve the public typestate API. Internally, consuming a wrapper may move
   cheap handles rather than unique large vectors.

### Performance constraint

Do not duplicate DOM, computed styles, box trees, or source text per cached
stage. One additional glyph-ID vector is acceptable only if measurements show
that retaining the original character buffer is cheaper than rebuilding
preparation on every reshape.

### Acceptance tests

- A font-environment change reshapes without parsing, styling, or preparing.
- A width change relayouts without reshaping.
- Pointer/capacity tests prove immutable phase inputs are shared rather than
  cloned.

## Milestone 9b — Central invalidation lattice

1. Implement the exhaustive mapping:
   - source -> parse;
   - resource base affecting resolved DOM URLs -> prepare or parse/configuration,
     according to the finalized Plan 08 DOM design;
   - CSS/style environment -> style;
   - font environment -> shape;
   - image metrics/layout constraints -> layout;
   - paint settings -> paint.
2. Coalesce pending changes before rebuilding.
3. Return an explicit `ReuseReport`:

```rust
pub struct ReuseReport {
    pub parsed: Reuse,
    pub styled: Reuse,
    pub prepared: Reuse,
    pub shaped: Reuse,
    pub laid_out: Reuse,
}
```

4. Add debug assertions that a reused artifact's input key exactly matches the
   requested key.
5. Make impossible combinations absent from the API. For example, callers
   cannot install new CSS while claiming that computed style remains valid.

## Milestone 9c — Integrate resources and image metrics

1. Separate intrinsic image metadata from immutable parsed DOM structure.
2. Give the layout phase a read-only `ImageMetrics` input keyed by resource ID.
3. When probing or decoding discovers new dimensions:
   - bump `ImageMetricsRevision` only if used intrinsic dimensions changed;
   - invalidate layout, not parsing or styling;
   - preserve the current navigation anchor across relayout.
4. Image pixel availability without a geometry change requests paint only.
5. Batch multiple image completions received during one event-loop turn into
   one relayout.

### Acceptance tests

- Decoding pixels with unchanged dimensions does not relayout.
- Newly discovered intrinsic dimensions relayout exactly once per batch.
- External CSS/resource URI changes invalidate the correct earliest phase.

## Milestone 9d — Migrate renderer settings

Move the existing ad hoc paths onto `PipelineChange`:

- column width and effective viewport width -> layout;
- root font size -> style onward while reusing source and parsed HTML;
- scale -> paint only unless a backend explicitly changes layout-space units;
- font database/backend revision -> shape onward;
- document navigation -> select or populate a different per-document cache.

Preserve glyph/CFI navigation anchors across every change. If glyph indices are
not stable after restyle/reshape, store a semantic anchor (CFI/DOM text offset)
before rebuilding and resolve it in the new output.

## Milestone 9e — Cache ownership and eviction

1. `html-pipeline` owns framework-neutral artifacts and dependency keys.
2. `html-view-core` owns navigation state, viewport state, paint caches, and the
   active `PipelineSession`.
3. UI adapters own only framework resources such as GPU images and text-layout
   objects.
4. Use a small document cache for EPUB spine navigation:
   - active document: laid out;
   - adjacent documents: optionally prepared or shaped;
   - distant documents: source/parsed cache or evicted.
5. Eviction occurs at phase boundaries and is measurable through the existing
   memory report.
6. Never let renderer paint caches keep an otherwise evictable pipeline
   artifact alive accidentally.

Start with deterministic count- or byte-based eviction. Add LRU behavior only
after measurements justify it.

## Milestone 9f — Verification and observability

### Equivalence oracle

For each change sequence:

1. update the incremental session;
2. build a fresh pipeline from final inputs;
3. compare their semantic render snapshots.

Compare DOM-addressable text, line glyph ranges, geometry with a documented
floating tolerance, decorations, images, anchors, and link mappings. Do not
compare allocation addresses or internal IDs that are intentionally unstable.

### Required scenarios

- repeated width changes;
- width -> font size -> width;
- CSS replacement and restoration;
- font backend generation change;
- image metrics arriving in different orders;
- batched simultaneous changes where the earliest stage must win;
- document navigation with cache eviction and reload.

### Instrumentation

Extend `PipelineTimings` with:

- per-stage run count;
- per-stage reuse count;
- invalidation cause;
- bytes retained per cached phase;
- time spent deciding reuse (expected to be negligible).

Debug logs should explain a rebuild as, for example:

```text
style rerun: StyleEnvironment(root_font_size 16 -> 18)
parse reused; document reused; style/prepare/shape/layout rebuilt
```

## Boundary enforcement

Extend `check-html-boundaries.sh` to ensure:

- UI adapters cannot invalidate individual stages directly;
- `html-view-core` does not construct raw phase artifacts;
- cache keys and invalidation types live in framework-neutral crates;
- paint-only code cannot obtain mutable layout or pipeline storage.

Add compile-fail coverage where practical and exhaustive-match tests for the
change-to-stage mapping.

## Performance budgets

- Invalidation decision: O(1), allocation-free.
- Width-only update: no DOM/style/shape allocations and no glyph-cache reset.
- Paint-only update: no pipeline work.
- Font-size update: no source I/O or HTML parse.
- Image completion: at most one relayout per batch.
- Retaining reusable upstream artifacts must not increase active-document memory
  by more than 10% without a measured latency benefit.
- Median width-only relayout should remain within 2% of the pre-Plan-09 path.

Use release builds and the existing TestData timing/memory profiles. Record the
baseline and result in a checked-in benchmark note so future work has a stable
comparison.

## Ordered implementation sequence

1. 9a — retainable immutable inputs and separate shaped/layout outputs.
2. 9b — typed invalidation lattice and reuse report.
3. 9d — migrate width, font-size, scale, and font-environment settings.
4. 9c — resource/image metric revisions and batching.
5. 9e — multi-document cache ownership and eviction.
6. 9f — full-build equivalence tests, diagnostics, and performance gates.

## Definition of done

- Every input change maps exhaustively to its earliest affected phase.
- Callers cannot manually under-invalidate the pipeline.
- Incremental output equals a clean rebuild across the required scenario suite.
- Width changes run layout only; scale changes paint only; font-size changes do
  not reload or reparse source HTML.
- Font and image changes rerun only their documented downstream phases.
- Cache ownership and eviction are explicit and included in memory reporting.
- Existing correctness, boundary, smoke, and performance tests pass.

## Effort and risk

Large. The highest-risk step is 9a because current shaping mutates the prepared
glyph buffer. Land reusable input/output separation before adding cache policy;
otherwise invalidation logic will be built around artifacts that cannot
actually be reused.
