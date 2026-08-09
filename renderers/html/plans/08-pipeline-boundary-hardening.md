# Plan 08 — Complete pipeline boundary hardening

## Goal

Finish the framework-agnostic HTML pipeline boundary work so that:

- finalized parser, style, shaping, layout, and render-contract values always
  satisfy their invariants;
- each phase can change its internal representation without forcing unrelated
  phases or UI adapters to change;
- malformed or foreign inputs are rejected at an explicit `Result` boundary;
- the additional validation does not add work to relayout or per-frame paint.

The precise guarantee is **invalid finalized phase states are
unrepresentable**. Untrusted input values may exist temporarily while being
built, but they cannot become `Document`, `ComputedStyles`, `ShapedDocument`,
or `LaidOutDocument` without validation.

## Completion status (verified 2026-08-01)

Plan 08 is complete. All milestones 8a–8f and every definition-of-done item
have implementation and regression evidence. In particular:

- glyph metrics are privately constructed, validated, and registered through
  append-only registries;
- style insertion and finalization reject invalid numeric values and foreign
  branded handles;
- DOM construction uses typed root states, permanently unique document
  lineages, opaque node IDs, and a validating finalization boundary;
- renderer-facing code reads laid-out state only through the borrowed
  `RenderView` contract;
- mutable layout representations are private modules of `html-layout`, and the
  former `html-layout-model` crate is gone;
- internal HTML crates are marked `publish = false`, with boundary enforcement;
- phase order remains `ParsedHtml -> StyledDocument -> PreparedDocument ->
  ShapedDocument -> LaidOutDocument`.

Final verification:

- `check-html-boundaries.sh`: passed;
- focused seven-crate unit, integration, and compile-fail suite: passed;
- root renderer library tests: 6 passed;
- root renderer, GPUI smoke, and Floem smoke builds: passed;
- release TestData profile: 173 documents, 0 failures, 2.663 ms mean layout
  time per document, 4,903.76 KiB largest retained document;
- allocation counter regression test: creating and traversing the render view
  performs zero allocations.

The release profile is the first recorded Plan 08 measurement, so it is the
baseline for future comparisons. There is no historical pre-change sample from
which to make a defensible percentage-regression claim.

### Enforced architecture

- phase order is encoded as
  `ParsedHtml -> StyledDocument -> PreparedDocument -> ShapedDocument ->
  LaidOutDocument`;
- completed phase fields are private and raw prepared/layout storage is not
  exposed to the renderer;
- computed styles and CSS-owned strings are separate from the parsed DOM;
- document lineage, style-store brands, and style-string brands reject values
  from another source result;
- style-to-layout transfer is zero-copy and relayout reuses phase inputs and
  top-level output buffers;
- `check-html-boundaries.sh` enforces the current dependency direction and
  prevents known reach-through APIs and publishable internal crates.

Future work belongs in a new plan: keep the boundary gate in CI, compare future
release profiles with the baseline above, and preserve the render contract when
layout internals evolve. Property-specific numeric wrappers remain an optional
API refinement, not an unmet invariant: invalid finalized styles are already
rejected both on insertion and again at `finish()`.

## Architectural target

```text
HTML text
   |
   v
html-parse ------> immutable validated Document
                         |
                         v
html-style ------> immutable validated ComputedStyles
                         |
                         v
html-layout -----> private box/inline/layout representations
                         |
                         v
                  immutable RenderDocument view
                         |
                         v
               UI/framework adapters
```

Only the value immediately downstream of a phase is part of that phase's
contract. Builder types, mutable trees, caches, and deduplication maps remain
implementation details.

## Milestone 8a — Validate glyph registration

### Problem

`GlyphMetric` has public fields, and `GlyphMetrics::push` accepts any value.
A `GlyphShaper` can therefore register NaN, infinity, negative advance, or
negative ascent/descent and still produce a `ShapedDocument`.

### Design

1. Replace public construction with a validated value:
   - `GlyphMetric::try_new(ch, advance, ascent, descent, baseline_offset)`;
   - require every numeric field to be finite;
   - require `advance`, `ascent`, and `descent` to be non-negative;
   - keep `baseline_offset` finite, allowing the range needed by real fonts.
2. Make metric fields private and add read-only accessors.
3. Replace direct `&mut GlyphMetrics` access in `GlyphShaper` with an
   append-only `GlyphRegistry<'_>`:
   - `register(metric: GlyphMetric) -> GlyphId`;
   - `len()` and read-only lookup as needed;
   - no replace, truncate, or arbitrary mutable-slice access.
4. Extend `ShapeError` to distinguish:
   - an unregistered glyph ID;
   - rejected metrics;
   - registry capacity exhaustion.
5. Replace `auxiliary_glyph_metrics_mut()` with an append-only auxiliary
   registry or a narrow `register_auxiliary_glyph` operation.

### Acceptance tests

- NaN/infinite metrics are rejected.
- Negative advance/ascent/descent are rejected.
- A shaper returning an unregistered ID is rejected.
- A valid shaper still produces identical glyph IDs and layout geometry.
- Auxiliary glyphs can be appended but existing metrics cannot be modified or
  removed.

## Milestone 8b — Validate computed style values

### Problem

Store branding makes style handles safe, but public style structs can contain
invalid floats. `finish()` currently validates ownership and node coverage, not
the values stored behind those handles.

### Design

1. Add `ComputedStyleValueError` with the property group and offending value.
2. Make style insertion fallible:
   `ComputedStylesBuilder::push(...) -> Result<StyleIndices,
   ComputedStyleValueError>`.
3. Validate before interning:
   - every float is finite;
   - font size and used line height are positive (with `line-height: normal` encoded as existing sentinel semantics);
   - border widths, padding, and border spacing are non-negative;
   - percentages and lengths obey the property's permitted negative range;
   - font weight is within the supported CSS range;
   - enum/string handles belong to the same builder.
4. Retain a full validation pass in `finish()` as defense in depth. It is
   construction-only and runs once per style result.
5. Centralize the rules on style substructures (`Font::validate`,
    `InheritedText::validate`, etc.) so the resolver and tests do not duplicate
    domain knowledge.
6. Longer term, replace public-field aggregate construction with validated
   constructors or property-specific types where doing so materially improves
   correctness. Do not introduce dozens of wrappers merely for type purity.
   Add type-level invariants for common value categories (`PositiveF32`,
   `NonNegativeF32`, finite CSS lengths/percentages) at the point where style
   values first enter the style phase.

### Acceptance tests

- Non-finite font, spacing, length, percentage, and border values cannot enter
  `ComputedStyles`.
- Negative values remain accepted only where CSS permits them (for example,
  margins and letter spacing).
- Builder/store branding and deduplication still work.
- Construction dedup maps are still discarded by `finish()`.

## Milestone 8c — Introduce a sealed DOM builder

### Problem

`DomNodeId` implements `From<u32/usize>`, `set_dom_root` accepts unchecked IDs,
and append operations tolerate missing parents. A malformed tree can therefore
be represented as a completed `Document` outside the parser.

There is also an explicit tension in this milestone: many current protections are
runtime checks in `finish()`. Where practical, those should move to compile-time
typed transitions so callers cannot represent impossible states.

### Design

1. Add `DocumentBuilder`; make `Document` the immutable finished value.
2. Remove public integer conversion into `DomNodeId`. IDs are returned only by
   the builder or document queries.
3. Brand node IDs with the builder/document identity, or validate identity and
   bounds on every builder operation. Identity allocation must never wrap and
   reuse a live brand.
4. Introduce a typed builder state machine (for example:
   `DocumentBuilder<NeedsRoot>` and `DocumentBuilder<Rooted>`) so `finish()`
   for non-empty documents is only callable once `set_dom_root()` has been
   successfully executed.
5. Builder operations return `Result` for:
   - foreign or out-of-bounds parent IDs;
   - adding text below a non-element parent;
   - multiple roots or a root with a parent;
   - invalid string/image references.
6. `finish()` validates:
   - a non-empty document has exactly one element root;
   - every node is reachable exactly once from the root;
   - parent and child links are reciprocal;
   - the graph is acyclic;
   - all string, attribute, class, image, and TOC indices are in bounds.
7. Move parser-only `DomElementData`, cache mutation, append operations, and
   root assignment onto `DocumentBuilder`.
8. Keep post-parse configuration explicit and invariant-preserving. Root font
   size and title can use consuming `with_*` methods or validated setters before
   styling; image dimension resolution may update only resource metadata.
9. Change parsing APIs to return `Result<Document, DocumentBuildError>` where
   callers can supply arbitrary builders. The HTML parser may use an internal
   `expect` only for invariants guaranteed by its own traversal.

### Acceptance tests

- Integer literals cannot be converted into `DomNodeId` (compile-fail).
- Foreign-builder IDs, missing parents, multiple roots, cycles, and invalid
  indices cannot produce `Document`.
- Parsed HTML produces the same DOM/TOC/image metadata as before.
- `StyledDocument` exposes no mutation path back into the DOM.

## Milestone 8d — Define a stable render-output contract

### Problem

The renderer currently consumes layout's concrete `Line`, `InlineRun`,
`InlineKind`, offset, and fragment structures. Those are read-only, but a layout
representation change still ripples into `html-view-core`.

### Design

1. Define the minimum immutable contract the renderer needs, either in a small
   `html-render-contract` crate or as a deliberately stable module owned by
   `html-layout`.
2. Expose `LaidOutDocument::render_view()` with borrowed, allocation-free
   queries for:
   - line count and line geometry;
   - glyph ranges and glyph metrics;
   - per-glyph positioning/offset runs;
   - text-run ranges needed by search and selection;
   - decorations and image fragments;
   - box ancestry/tag/class/link queries;
   - anchors and document metadata.
3. Replace public `InlineKind` pattern matching with semantic iterators such as
   `text_runs()`, `marker_runs()`, and `images_for_line()`.
4. Keep contract fields private where invalid ranges or indices are possible;
   use constructors validated by layout and read-only accessors for rendering.
5. The contract may borrow layout-owned storage. Do not copy line, glyph, or
   fragment vectors merely to create the view.
6. Migrate `html-view-core` and the GPUI/Floem adapters to depend only on
   `html-layout`, the render contract, pipeline orchestration, and resources.

### Acceptance tests

- Renderer crates have no direct dependency on `html-dom`,
  `html-style-model`, or `html-layout-model`.
- Renderer source does not name `LayoutTree`, `LayoutState`, `LayoutBox`,
  `InlineContent`, `StyleIndices`, or raw DOM node structures.
- Search, selection, CFI, navigation, images, decoration, and painting tests
  pass through the new contract.
- Creating `render_view()` allocates nothing.

## Milestone 8e — Hide mutable layout representations

### Problem

Rust has no friend-crate visibility. As long as `html-layout-model` is a
separate crate, the mutable API needed by `html-layout` is necessarily public
to every possible consumer.

### Design

1. After 8d removes renderer dependencies, move `LayoutTree`, `LayoutBox`,
   `InlineContent`, and `LayoutState` into private modules of `html-layout`.
2. Remove `html-layout-model` from the workspace, or reduce it to immutable
   render-contract values only.
3. Do the analogous cleanup for style construction:
   - keep immutable computed-style values in `html-style-model` if layout needs
     them;
   - keep builder-only mutation as narrow as possible;
   - mark internal crates `publish = false` during migration, while recognizing
     that this is distribution protection rather than Rust visibility.
4. Stop re-exporting internal tree/run/state types from the layout facade.
5. Update `check-html-boundaries.sh` so any new direct dependency on an internal
   representation crate fails CI.

### Acceptance tests

- No workspace crate outside `html-layout` can name or construct a layout tree,
  inline buffer, or mutable layout state.
- `LaidOutDocument` remains constructible only by consuming a valid
  `ShapedDocument`.
- Relayout can mutate only layout-owned state.

## Milestone 8f — Clarify transition guarantees

`StyledDocument::into_parts()` plus `PreparedDocument::try_new()` currently
allows callers to attempt a mismatched pair, but no invalid `PreparedDocument`
can result. That is a sound runtime boundary, not a compile-time pairing.

Choose deliberately between:

- **Recommended:** retain this design, document that attempted foreign pairing
  is an input error, and keep lineage validation. It preserves the desired
  `html-layout -> style-model` dependency instead of coupling layout to the CSS
  implementation crate.
- If compile-time pairing is judged more important than dependency direction,
  move ownership of `StyledDocument` and the style-to-layout transition into a
  common orchestration crate. Do not make `html-layout` depend on Lightning CSS
  merely to call `StyledDocument::prepare()`.

The acceptance requirement is that invalid completed states remain impossible;
it is not necessary to make every rejected function argument impossible to
write.

## Boundary and regression enforcement

Extend `HtmlRenderer/scripts/check-html-boundaries.sh` to enforce:

- no renderer dependency on DOM/style/layout implementation models;
- no public integer construction of branded IDs;
- no public raw mutable access to completed phase storage;
- no wildcard representation re-exports;
- no public builder/mutator surface for layout internals after 8e;
- no direct CSS-string interning into the parsed document.

Add compile-fail tests for:

- constructing `DomNodeId`, `StyleIndices`, and other branded IDs;
- skipping shaping or layout phases;
- reaching raw storage through completed phase wrappers;
- directly constructing validated metrics and render-contract ranges.

Add runtime tests for every fallible boundary and retain the existing
cross-document, foreign-store, zero-copy, and relayout-reuse tests.

## Performance constraints

Boundary hardening must not turn into hot-path validation:

- validate DOM, styles, and glyph metrics once when each phase is finalized;
- no validation pass during relayout, scrolling, selection, or paint;
- `render_view()` and its iterators must be borrowing and allocation-free;
- preserve style deduplication and discard construction maps after finishing;
- preserve top-level relayout buffer capacities;
- avoid enlarging per-node/per-box optional handles; retain niche-size tests;
- checked branded-handle lookups may perform one identity comparison, not a
  repeated full-table validation.

Before and after each milestone, run the ignored TestData timing and memory
profiles. Investigate a regression above 2% in median layout time or retained
document memory; do not treat normal debug-build noise as a regression.

Completion note: historical before-milestone output was not retained. The
2026-08-01 release run recorded above is therefore the comparison baseline for
subsequent changes; the allocation-free contract itself is guarded directly by
an exact allocation-counting test.

## Ordered implementation sequence

1. **8a — Glyph validation and append-only registry.** Small, closes a real
   invalid `ShapedDocument` path.
2. **8b — Computed-style value validation.** Medium, closes invalid styled data
   before layout.
3. **8c — Sealed DOM builder.** Larger and foundational; land separately.
4. **8d — Stable borrowed render contract.** Migrate renderer consumers before
   hiding representations.
5. **8e — Merge/hide mutable layout representations.** Mechanical after 8d.
6. **8f — Record the transition policy and finish enforcement/audits.**

Each milestone should land with its own tests and boundary-script update. Do
not combine the DOM-builder migration and render-contract migration in one
change; both have wide but independent call-site impact.

## Definition of done

- [x] All completed phase objects can be created only through validating
  transitions.
- [x] No backend can inject invalid glyph metrics into `ShapedDocument`.
- [x] No caller can create a completed DOM with forged nodes or broken tree links.
- [x] No invalid numeric style value can enter `ComputedStyles`.
- [x] Mutable layout representations are private to `html-layout`.
- [x] Renderers consume only the immutable render-output contract.
- [x] Boundary script, compile-fail tests, focused crate tests, root renderer build,
  and GPUI/Floem smoke builds pass.
- [x] Zero-copy style transfer, allocation-free render views, and relayout reuse are
  covered by regression tests.

## Effort and risk

- 8a: small–medium, low risk.
- 8b: medium, moderate CSS-domain validation risk.
- 8c: large, highest API-migration risk.
- 8d: large, highest renderer-behavior regression risk.
- 8e: medium after 8d, mostly file/module movement.
- 8f: small.

The safest delivery strategy is a sequence of independently verifiable changes,
with performance measurements at every finalized phase boundary.
