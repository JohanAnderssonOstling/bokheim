# Web Platform Test sources and adaptations

The parser and style suites consume a pinned subset of the original Web
Platform Tests directly. Unchanged upstream files remain in the shared corpus
selected through `HTML_WPT_ROOT`. Engine manifests and nonvisual fixtures live
under `testdata/wpt` in the external
[HtmlEngine repository](https://github.com/JohanAnderssonOstling/HtmlEngine),
while GPUI reftest ledgers live beside the GPUI test under
`apps/renderers/html/crates/html-view-gpui/tests/wpt`. The dev-only
`html-wpt-test-support` crate parses native tree-construction `.dat` files and
extracts literal declarative CSS helper calls without executing JavaScript.

At revision `7ea3d665ff9ae1df1f742433d51bd1e9c6499a3b`, the HTML runner discovers all
1,934 cases from all 61 tree-construction corpora in their applicable scripting
modes: 3,828 possible runs. It compares a read-only
semantic parser tree that preserves doctypes, comments, namespaces, template
contents, and processing-instruction distinctions; fragment contexts and the
HTML scripting flag are explicit parser inputs. The three upstream
`document-compatmode` tests directly verify no-quirks, limited-quirks, and
quirks modes. Because html5ever 0.39 and its current main branch predate the
June 2026 processing-instruction tokenizer states, exact reasoned manifests
ignore the affected assertions. They also quarantine verified html5ever/scraper
tree-sink gaps and html5ever-owned exact diagnostic differences. The active
audits execute 3,629 tree runs and 3,451 error-count runs with no known active
divergences. In total, 199 tree runs and 377 error-count runs are explicitly
ignored. Passing cases in the same upstream files remain active. Four of the
ignored runs are scripted WPT cases requiring a JavaScript runtime, which this
noninteractive renderer intentionally does not provide.

The CSS runner scans all 2,409 vendored CSS HTML files, including the 1,267
files under CSS `parsing` directories, 233 declarative helper-test files outside
those directories, and the 909 layout-test files under `css/`. It loads
referenced local static data resources without modifying the upstream files. It
extracts 31,853 literal or finitely generated helper assertions, directly
executes 20,639 property-value, selector, whole-rule, and import-rule assertions
within the known syntax scope, and reports 11,202 assertions for
properties or value features the renderer does not implement. Gradient-bearing
declarations are recognized as a distinct unrendered capability and do not
participate in the cascade or `@supports`. Modern color mismatches are also
classified as unsupported dependency syntax: passing color assertions remain
active, but `html-style` does not implement a second parser for syntax that
Lightning CSS cannot represent reliably. CSS Grid assertions outside the typed
subset represented by the style model and layout pipeline are classified as
unsupported; supported track and placement values remain active.
CSS Masking is also unsupported through paint, including `clip-path` and mask
images. Mismatches for unimplemented background images, clipping, positioning,
radii, border images, shadows, and sizing are classified separately while
background-color and border strictness remain active. Filter Effects are
unsupported because the renderer has no offscreen filtering/compositing stage.
CSS Transitions are unsupported because this noninteractive renderer has no
animation clock or frame-driven restyling. CSS Transforms are likewise outside
the document renderer scope. CSS Inline Level 3 `vertical-align` syntax remains
classified as a dependency gap because Lightning CSS exposes only the CSS2
grammar; values that cannot pass that parser are not represented downstream.
The parser-supported `text-overflow: clip | ellipsis` subset reaches layout and
paint. Custom string markers remain a dependency syntax gap because the pinned
Lightning CSS value type exposes only those two keywords.
Four hundred eleven calls remain dynamic, browser-dependent, or dependency
panics contained by the adapter. The exact active ledger currently records
1,000 syntax divergences, and seven selector runs are explicitly ignored for
dependency grammar/recovery gaps.

The no-JavaScript adapter expands static arrays, tuples, nested loops, template
substitution, simple declarative wrappers, CSS color helpers, and the finite
font and gradient matrices used by WPT. Computed-value helpers contribute their
specified-value parsing assertion; CSSOM serialization, resolved layout,
animation/interpolation behavior, legacy alias identity, and interactive DOM
semantics are not claimed. Unsupported calls are counted and can be printed
with `HTML_WPT_VERBOSE=1`; they are never silently treated as passing tests.

The local style regressions also exercise the production path for strict
selector-list rejection, rule-level error recovery, CSS Text Level 4
`white-space`, percentage/calc letter and word spacing (including inheritance),
and invalid box declarations. This keeps a passing syntax adapter from being
mistaken for proof that the value reaches computed style and layout.

The layout runner consumes 914 unchanged upstream `check-layout` HTML files from
the same pinned revision. It loads their external stylesheets through the
production resource pipeline, uses deterministic framework-neutral glyph
metrics (including WPT Ahem's full-em advance), correlates layout boxes with
source DOM nodes, and executes 21,483 upstream `data-expected-width` and
`data-expected-height` assertions with WPT's one-pixel tolerance. Of those,
13,430 pass directly, 5,488 exact assertion-runs are reasoned capability
exclusions, and 2,565 are exact active layout divergences. The adapter also
recognizes and reports 18,805 offset, client-size, scroll-size, margin, padding,
bounding-rectangle, and display assertions that require geometry not exposed
by the current semantic layout view. Files requiring DOM or style mutation
remain outside the direct runner. Inline scripts are accepted when they are a
pure `checkLayout(...)` trigger or only wait for browser fonts before running
that trigger; the deterministic shaper makes the latter wait semantically
irrelevant. Files containing only currently unavailable geometry kinds are
still parsed and audited, but skip shaping and layout to keep the enlarged
suite inexpensive. Intrinsic Flex bases are
resolved from the renderer's content measurements before constructing Taffy's
item style because Taffy's `flex_basis` field accepts only auto, lengths, and
percentages. Replaced Flex items carry their decoded intrinsic dimensions and
ratio into that same boundary; percentage-height replaced descendants use the
definite cross size for their intrinsic main-axis contribution. The WPT runner
probes pinned image fixtures through the production resource provider. Authored
`aspect-ratio` values cross the typed computed-style boundary, participate in
ordinary auto block-size resolution, and reach Taffy's Flex/Grid sizing; the
`auto` flag continues to prefer intrinsic ratios for replaced elements. A new
failure or a known failure that starts passing fails the runner. Missing layout
boxes are audited as engine divergences, while unknown `check-layout`
attributes fail adapter validation rather than being silently skipped. Set
`HTML_WPT_LAYOUT_VERBOSE=1` to print every
executed assertion-run key.

The GPUI visual runner complements those semantic stage tests with 21,544
unchanged static WPT reftests. It renders both the test and its declared references through
`HtmlView`, the production GPUI paint adapter, and a feature-gated WGPU headless
target at an 800 x 600 viewport and device scale 1. The windowed and headless
targets share the same GPUI scene encoder; only target acquisition and pixel
readback differ. Each process reuses its application, WGPU device, test window,
pipeline cache, and glyph atlas across pages, periodically refreshes that
harness, and bounds its rendered-reference cache. Currently 10,297 tests match
directly, 11,099 remain in the exact reconciled active failure ledger, and 148
are explicitly skipped for quirks-mode or HTTP requirements. Per-page
renderer panics are also ledgered and do not abort later cases; ten observed
non-terminating inputs are isolated by an exact timeout ledger.

The adapter reads standard `match`, `mismatch`, and document-wide or
reference-scoped `fuzzy` metadata from the original HTML. It refuses scripts,
`reftest-wait`, and server-substituted documents in the entire test/reference
closure, so the suite cannot accidentally claim interactive browser behavior.
For the vendored CSS2 encoding tests, the resource provider also reconstructs
the upstream HTTP `Content-Type` media type and charset metadata that is not
stored in the static fixture bytes. Those values enter the production resource
metadata API; stylesheet decoding then applies BOM, transport charset,
`@charset`, referring-document charset, and UTF-8 precedence in that order.
Static tests for unsupported capabilities, Ahem-dependent layout, SVG, MathML,
and image formats remain active as exact failures. Run it with
`cargo test -p html-view-gpui --test wpt_reftest`; set
`HTML_WPT_GPUI_SHARD=INDEX/TOTAL` for deterministic parallel shards and
`HTML_WPT_GPUI_RANGE=START..END` to subdivide one shard.

## Reduced adaptations

Some crate-local tests use reduced semantic cases adapted from the
[Web Platform Tests](https://github.com/web-platform-tests/wpt) project. Each
adapted test links to its exact upstream source. Browser scripts, reference
rendering, fonts, and unrelated assertions are intentionally omitted.

Current reduced adaptations, retained as focused crate-local regressions:

- `html-parse`: `blocks.dat`, `adoption01.dat`, and `entities01.dat`;
- `html-style`: `important-vs-inline-002.html` and
  `all-prop-unset-color.html`;
- `html-layout`: `break-spaces-newline-011.html` and flex intrinsic sizing
  cases from `row-005.html`.

Web Platform Tests is distributed under the following license. The vendored
copy is also retained at `apps/renderers/html/testdata/wpt/LICENSE.md`.

## The 3-Clause BSD License

Copyright © web-platform-tests contributors

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
