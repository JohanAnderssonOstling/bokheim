# WPT Early-Pipeline Correctness Plan

## Objective

Improve conformance in pipeline order, fixing shared early computations before
later layout and paint symptoms. A failure count is not by itself a priority:
the preferred target is the earliest incorrect result with the widest
downstream fan-out.

This plan uses the pinned static WPT corpus as its evidence and acceptance
gate. The current ledger classifies the 21,544-test visual manifest as:

- 10,297 expected direct matches;
- 11,099 active tracked failures;
- 124 explicitly unsupported quirks-dependent tests and 24 unsupported HTTP
  tests, which the harness skips only after exact-ledger validation;
- 6,148 active failures remain in the supported-feature upper bound after
  excluding 4,951 failures with explicit unsupported, dependency, or
  infrastructure causes;
- 10 timeouts, already included in the active failure count.

Unsupported features and dependency-owned behavior remain active where
possible and are recorded with exact reasons.

## Latest full visual rerun

The reconciled 21,544-test rerun produced 1,914 failing-to-passing transitions
and 1,308 passing-to-failing transitions, for a net reduction of 606 active
failures. Test-level support triage of the 1,308 regressions found:

- 1,040 tests that require intentionally unsupported renderer capabilities;
- 3 tests whose behavior is owned by pinned Lightning CSS;
- 261 regressions in features the renderer intends to support after fixing the
  four HTML-input regressions.

The 261 remaining supported regressions are ordered by their earliest plausible owning
pipeline stage:

| Order | Pipeline layer | Supported regressions |
| ---: | --- | ---: |
| 1 | HTML inputs, base URLs, and presentational hints | 0 |
| 2 | Selector matching and cascade | 5 |
| 3 | Computed font-relative values | 5 |
| 4 | Box construction, generated content, and list markers | 19 |
| 5 | Text preprocessing, shaping, and inline geometry | 66 |
| 6 | Intrinsic sizing, normal flow, Flex, Grid, and tables | 81 |
| 7 | Positioned and alignment geometry | 67 |
| 8 | Borders, overflow propagation, and painting | 18 |

This transition audit is not a replacement for the complete active backlog:
the reconciled ledger still contains 6,148 failures not excluded by an
unsupported, dependency, or infrastructure cause. The transition counts only
identify regressions introduced relative to the preceding ledger.

## Current earliest-stage status

HTML byte decoding, tokenization, and DOM tree construction are green for all
non-excluded vendored tests.

- All 20 `html/syntax/charset` visual reftests pass.
- Standards-mode document tests pass. Quirks and limited-quirks documents are
  intentionally rejected at DOM construction with an `expect` failure.
- The 124 visual manifest entries whose test or reference enters quirks mode
  are recorded as `unsupported:quirks-mode` in a dedicated exact-path ledger.
- All non-excluded tree-construction runs pass.
- All vendored static XHTML reftests parse as XML.
- The remaining parser exclusions are explicitly attributed to `html5ever`,
  `scraper`, JavaScript DOM mutation, or `document.write`.
- Static HTML element state is normalized once into a compact bitset at DOM
  finalization. Supported selectors consume that state for explicit/inherited
  direction, links, enabled/disabled controls, checked/default selection,
  required/optional controls, read-only/read-write content, placeholders,
  radio indeterminacy, open/closed elements, and defined built-ins.
- Focused static state reftests now pass for `:dir()`, `:required`, `:optional`,
  `:open`, option `:checked`, and radio `:indeterminate`.
- Supported legacy dimension, color, list-type, and replaced-border hints now
  pass their focused HTML/XHTML reftests. Canvas dimensions remain intrinsic
  rather than being incorrectly converted into CSS declarations.
- Remaining failures in the audited HTML rendering families are attributed to
  later table/replaced/bidi geometry or explicitly unsupported fieldset,
  frameset, marquee, video, native-widget, automatic-directionality, and bidi
  isolation behavior.

The next work therefore begins at the DOM-to-style boundary.

## Priority order

| Order | Pipeline layer | Direct WPT signal | Reason for priority |
| ---: | --- | ---: | --- |
| 1 | DOM-to-style inputs | 15 HTML-directory failures; 0 attributed early-input faults | Incorrect mode, namespace, hints, or UA declarations contaminate every later stage |
| 2 | Selectors and cascade | 19 | Determines which declarations exist and win |
| 3 | Computed-value resolution | 0 | The live candidates are fixed, dependency-classified, unsupported, or reassigned to their first observable later stage |
| 4 | Box-tree construction | 459 | Defines the boxes and formatting contexts later layout operates on |
| 5 | Text preprocessing and shaping | 2,485 | Determines text runs, whitespace, advances, baselines, bidi, and logical direction |
| 6 | Intrinsic sizing | 419 | Shared by flex, grid, tables, floats, replaced content, and shrink-to-fit |
| 7 | Normal-flow and positioned geometry | 3,280 | Depends on correct boxes, text measurement, and sizes |
| 8 | Fragmentation | 1,375 | Requires stable unfragmented geometry |
| 9 | Painting and effects | 1,402 | Must consume correct final geometry |
| 10 | Unclassified cross-feature tests | 117 | Requires test-level triage before assigning an owning stage |

The computed-value row is a live audited count. The downstream numeric rows are
older directory-owner proxies and are not additive after test-level
reassignment and unsupported classification; the 6,148 global
supported-feature upper bound is authoritative. WPT directory placement
identifies the feature being tested, not necessarily the layer containing the
root cause. Of those 19 HTML-directory failures, none are currently attributed
to the early input stage; all 15 are list, table, text, or replaced-element
layout failures.

## 1. DOM-to-style inputs

Audit the semantic information derived from the parsed document before normal
author CSS is resolved:

- standards-mode enforcement, with quirks and limited quirks explicitly unsupported;
- namespace and HTML/XHTML distinction;
- presentational hints;
- UA stylesheet origin and order;
- stable element state such as `open`, `disabled`, `checked`, and list ordinal
  data.

Start with:

- `quirks/`;
- simple `html/rendering/non-replaced-elements` tests;
- HTML attributes such as `align`, `width`, `height`, `border`, `bordercolor`,
  `cellpadding`, `cellspacing`, `nowrap`, `bgcolor`, `color`, `clear`, list
  `type`, and the legacy attributes of `hr` and `body`.

Do not initially mix in fieldset, details, select, or native-widget behavior.
Those frequently require specialized box construction or an explicit
unsupported classification.

### Acceptance gate

- Quirks and limited-quirks input cannot cross the parse-to-DOM boundary.
- Presentational hints enter the cascade as typed declarations.
- Presentational hints rank above UA rules and below author rules.
- HTML-only behavior does not leak into XML/XHTML nodes incorrectly.
- Relevant focused WPT groups have no unexpected failures or stale known
  failures.

## 2. Selectors and cascade

Relevant current groups include:

- `css/selectors`: 57 failures;
- `css/css-cascade`: 30 failures;
- `css/css-variables`: 63 failures;
- `css/css-conditional`: 40 failures;
- `css/css-style-attr`: 6 failures.

Work in this order:

1. selector validity and matching;
2. specificity;
3. origin and importance;
4. cascade layers;
5. inheritance;
6. `initial`, `inherit`, `unset`, and `revert`;
7. custom-property substitution and invalid-at-computed-value behavior;
8. pseudo-element cascades.

### Acceptance gate

- Selector matching and cascade tests are correct before inspecting layout.
- Invalid selectors and declarations cannot leak partial values.
- A winning declaration has a traceable origin, priority, and reason.
- Dependency-owned parser behavior is recorded as dependency-unsupported rather
  than patched around downstream.

## 3. Computed-value resolution

Relevant current groups include CSS Values, Fonts, Color, Display, and Box,
representing roughly 598 direct failures.

Priorities:

- `em`, `rem`, `ex`, `ch`, and percentage resolution;
- `calc()` and min/max functions;
- invalid-at-computed-value behavior;
- logical properties and direction-dependent mapping;
- `currentcolor`;
- font-relative inheritance;
- shorthand atomicity;
- display blockification;
- capability fallback that preserves earlier valid declarations.

### Acceptance gate

- Computed styles contain renderer-owned typed values rather than parser AST
  objects.
- Layout does not repeat cascade or computed-value interpretation.
- Relative values remain relative only when their used-value dependency is not
  yet available.

### Current status

The computed-value gate is closed for currently supported features. The prior
87-test figure was a stale directory proxy rather than an exact live set. The
replacement audit is recorded path-by-path in
`testdata/wpt/computed-value-stage-audit.txt`:

- CSS Values has no unclassified supported visual failure;
- ordinary and modern supported color conversion, system colors, and
  `currentColor` are green; the eight remaining Color failures begin at group
  compositing;
- the remaining Display failures have correct computed values and begin at
  specialized box construction, flex/table layout, or root painting;
- two remaining Font cases begin at font fallback or MathML shaping, and the
  remaining logical float/clear case begins at geometry;
- 30 author-font/descriptor/palette/synthesis cases and five registered custom
  property cases now carry explicit unsupported reasons;
- 36 invalid-value mismatches introduced by pinned Lightning CSS alpha.72 are
  exercised under eight exact dependency prefixes rather than treated as
  renderer-owned computed-style defects.

All 239 `html-style` unit tests pass. The 22,977-file declarative CSS WPT gate
also passes with 31,853 extracted literal assertions, including its exact known
dependency ledger. `html-style-model` contains no production Lightning CSS AST
types, and production layout consumes renderer-owned computed/used-value
representations rather than reparsing declarations.

## 4. Box-tree construction

Relevant groups include:

- `css/css-display`: 93 failures;
- `css/css-content`: 45 failures;
- `css/css-pseudo`: 83 failures;
- `css/css-inline`: 128 failures.

Priorities:

- `display: none`;
- blockification;
- anonymous blocks and anonymous table objects;
- generated content and `::before`, `::after`, and `::marker`;
- replaced-element classification;
- out-of-flow extraction;
- establishment of block, inline, flex, grid, and table formatting contexts;
- inline splitting around block descendants.

### Acceptance gate

- The box tree can be tested independently of geometry.
- Formatting-context ownership is explicit and compact.
- Layout never needs to reconstruct missing anonymous or generated boxes.

### Current status

- All 56 focused box-builder tests pass, covering suppression,
  blockification, anonymous block and table fixup, inline splitting,
  `display: contents`, generated `::before`/`::after`, list markers, and
  out-of-flow anchors.
- `display-contents-table-001.html` now passes: descendants exposed through
  adjacent box-suppressed ancestors participate in one anonymous table, so
  row groups and header/footer ordering are fixed up together.
- Unusual HTML elements whose `display: contents` used value becomes none no
  longer leave a whitespace-only line box. That WPT's remaining offset belongs
  to root/body overflow propagation and margin collapsing.
- Inline SVG and the specialized `::details-content` pseudo are explicitly
  unsupported. Remaining supported display-contents mismatches are assigned
  to later inline-fragment, inline-flex, flex, root/body propagation, or paint
  stages.

The next active work is therefore text preprocessing, beginning with
whitespace transformation before shaping.

## 5. Text preprocessing and shaping

The largest related groups are:

- CSS Text: 1,118 failures;
- Writing Modes: 869 failures;
- Fonts: 208 failures;
- Text Decoration: 227 failures;
- Ruby: 88 failures;
- CSS Inline: 128 failures.

Work in this order:

1. text-node and generated-content collection;
2. whitespace transformation;
3. text transformation;
4. run segmentation;
5. font selection and metrics;
6. shaping and cluster mapping;
7. bidi;
8. logical writing direction;
9. line-box construction.

The CSS2 whitespace gate contains 111 tests: 108 direct passes and three
tracked failures. All ten actionable preprocessing failures now pass. The
remaining three require intentionally unsupported bidi/RTL processing and are
recorded with that reason in the unsupported audit and cause registry.

The CSS2 text-transform gate contains 26 tests and now passes completely.
Transformation is a dedicated preprocessing pass over the logical inline text,
before run segmentation and shaping. It preserves DOM source positions while
supporting one-to-many Unicode case mappings and determines capitalization
across inline style boundaries rather than treating each shaping span as a
separate word stream.

The broader 102-test CSS Text transform gate additionally moved ten tests from
tracked failures to direct passes. Its 36 remaining failures require later
language tailoring or unsupported transform values such as full-width,
full-size-kana, and math-auto, plus a smaller set of modern word-boundary and
text-geometry cases. One old aggregate fixture has a documented stale Unicode
expectation that contradicts the focused normative mapping test.

Run segmentation now crosses DOM, paint-only, spacing, and out-of-flow anchor
boundaries while splitting at font changes, forced breaks, atomic inline
content, non-baseline vertical alignment, and the affected edge of non-zero
inline margin, padding, or border. In the ten-test CSS Text boundary-shaping
gate, six tests pass directly. The four tracked cases are attributed to later
vertical-align line geometry or intentionally unsupported bidi isolation and
Arabic bidi shaping rather than to segmentation.

Logical shaping spans may contain multiple source ranges, allowing non-empty
out-of-flow descendants to shape in their own formatting context without
fragmenting the surrounding in-flow text. Contiguous spans retain the direct
glyph-slice fast path; only discontiguous spans use a temporary scatter buffer.

Font selection now accepts the valid CSS zero font size instead of leaving raw
Unicode scalar values in the glyph-ID arena. The focused zero-size gate no
longer produces three shaping panics; its three remaining mismatches belong to
later margin/line-box and intrinsic-sizing behavior. Standards-mode legacy
`font[size]` values are converted in the dedicated HTML presentational-hint
module during the prerequisite cascade, including relative parsing and 1..=7
clamping. This moves `font-size-xxx-large.html` to a direct pass.

All 68 tracked font-variant failures now carry explicit unsupported or backend
dependency reasons. Sixty-six require unloaded author test fonts; the remaining
two depend on the host emoji environment or optional control of Arabic required
ligatures. Generic family selection now resolves `ui-monospace` through the
same installed-face path as `monospace`, moving two standard-family tests to
direct passes.

The remaining family-name, first-available-font, system-ui oracle, author-alias,
and ui-rounded failures now identify their external font dependency explicitly.
Relative `bolder` and `lighter` computed weights no longer use fixed 700/300
values: they resolve from the parent's computed weight using the CSS weight
table, including inside the `font` shorthand. The three legacy visual weight
fixtures remain dependency-classified because they require the external
CSSTest families with controlled face coverage.

Italic and oblique are now distinct values from computed style through the
renderer-neutral shaping contract, GPUI face requests, and both glyph and line
cache identities. Oblique angles are intentionally reduced to the backend's
single oblique face class because GPUI exposes no angle-specific selector. The
remaining italic/oblique fallback test and all 19 font-synthesis tests are
explicitly classified as unsupported: their oracles require author `@font-face`
descriptor matching and synthesis controls that the static font host does not
implement.

The three default-feature tests are likewise author-font or vertical-writing
oracles, not failures of the enabled baseline OpenType feature set. The two
remaining standard-family failures are now explicit: one requires unsupported
font-synthesis control, while the other assumes a browser default font that is
visually distinct from `ui-sans-serif`, contrary to the reader host's configured
system-UI default policy.

The `font-size-adjust` property is intentionally unsupported. All 14 tracked
property tests carry that explicit reason and are excluded from actionable font
selection work. This decision is separate from the `size-adjust` descriptor on
author `@font-face` rules, which is already unavailable with author font loading.

Feature-query syntax validation now uses the renderer's own `font-kerning`
grammar when Lightning CSS retains the declaration as unparsed. Consequently,
`@supports (font-kerning: auto)` evaluates consistently with the cascade and
`font-kerning-01.html` passes directly. The other four kerning fixtures require
an unloaded author test font, with two additionally requiring vertical `vkrn`
shaping, and carry explicit unsupported reasons.

The three feature-across-space failures do not expose another logical-run
segmentation defect: each oracle depends on the unloaded LigatureSymbols author
font, with one also depending on an `@font-face` feature descriptor. The
descriptor-specific and named-alternates fixtures likewise require unsupported
author-font descriptors or `@font-feature-values`; all five now carry explicit
unsupported reasons.

Language tags remain available in inherited computed style and already split
logical shaping spans. The two language-rendering mismatch fixtures cannot
test whether those tags reach shaping because both sides first require an
unloaded Lato author font; fallback substitution makes their English and
Turkish documents identical. They are therefore classified only by that proven
earlier failure, not as backend defects. `font-language-override` is
intentionally unsupported. The Japanese kerning fixture is likewise
author-font-only rather than evidence of a horizontal `kern` defect.

Root cap-height is now a first-class shaped metric. `font-size: rcap` remains
symbolic through computed style and resolves from the selected root face; the
ordinary `cap` unit follows the same selected-face path, which is required by
the WPT reference. `rcap-in-monospace.html` now passes directly. The sibling
`rex` and `ric` fixtures require unloaded purpose-built author fonts and carry
explicit unsupported reasons.

If vertical writing is intended, introduce logical axes here rather than adding
physical-axis exceptions independently to every layout algorithm.

## 6. Intrinsic sizing

Relevant groups include:

- CSS Sizing: 425 failures;
- Flexbox: 762 failures;
- Grid: 517 failures;
- Gaps: 311 failures;
- Tables: 91 failures;
- replaced-element and image sizing tests.

Build one authoritative system for:

- min-content and max-content contributions;
- fit-content and shrink-to-fit;
- percentage definiteness;
- `auto` sizing;
- aspect-ratio transfer;
- min/max constraints;
- border-box and content-box conversion.

Flex, grid, tables, floats, inline blocks, positioned boxes, and replaced
elements must consume this shared system rather than maintain independent
approximations.

## 7. Normal flow and positioned geometry

Only after the earlier gates are satisfied, finish:

- margin collapse and clearance;
- floats and formatting-context avoidance;
- containing blocks;
- relative and absolute positioning;
- overflow geometry;
- final used-size and coordinate conversion.

Existing margin, float, BFC, and initial-containing-block improvements form the
starting baseline.

## 8. Fragmentation

The directly related backlog is approximately:

- CSS Break: 879 failures;
- Multicolumn: 354 failures;
- Paged Media: 135 failures.

Pages and columns should use one shared fragmentation model operating on stable
unfragmented geometry. Avoid solving break tests with page- or column-specific
layout exceptions.

## 9. Painting and effects

Address only after final geometry is reliable:

- stacking contexts and paint order;
- clipping and overflow;
- backgrounds and borders;
- opacity and compositing;
- transforms;
- masks and filters.

First classify effect families that are intentionally outside the renderer's
scope so unsupported capabilities do not obscure actionable geometry bugs.

## Execution policy

For every work group:

1. Run the narrow WPT family and record the observed baseline.
2. Diagnose the earliest incorrect pipeline output.
3. Add a focused unit or boundary regression at that layer.
4. Implement the shared fix without downstream test-specific compensation.
5. Rerun the narrow WPT family.
6. Remove passing tests and stale causes from exact ledgers.
7. Run the owning crate suites and cause-registry validator.
8. Recount and update documented compliance.

Unsupported behavior must include a machine-readable reason. Dependency-owned
behavior must identify the dependency. JavaScript and other tests excluded by
the static importer must remain documented separately from renderer failures.

Do not run `cargo fmt` while executing this plan.

## Immediate next actions

1. Perform a bounded audit of document-mode propagation and presentational
   hints.
2. Fix the actionable `quirks/` failures caused before layout.
3. Move directly to `css/selectors`, `css/css-cascade`, and
   `css/css-variables`.
4. Proceed to computed values and box construction only after those gates are
   clean.
