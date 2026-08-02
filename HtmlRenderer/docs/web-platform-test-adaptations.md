# Web Platform Test sources and adaptations

The parser and style suites consume a pinned subset of the original Web
Platform Tests directly. The unchanged files, exact revision, complete hash
manifest, and upstream license live in `HtmlRenderer/testdata/wpt/`. The
dev-only `html-wpt-test-support` crate parses native tree-construction `.dat`
files and extracts literal declarative CSS helper calls without executing
JavaScript.

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

The CSS runner scans all 142 files in the box, cascade, text, and selectors
parsing directories. It extracts 1,260 literal helper calls, directly executes
780 property-value, selector, whole-rule, and import-rule assertions within the
known syntax scope, reports 473 unknown-property assertions and 15 dynamic or
unimplemented calls, exact-ledgers 21 known syntax divergences, and explicitly
ignores seven selector runs attributable to dependency grammar/recovery gaps.
Computed-value helpers currently
contribute their specified-value parsing assertion; computed serialization is
not claimed. The adapter is deterministic and intentionally has no JavaScript
runtime or interactive DOM.

The local style regressions also exercise the production path for strict
selector-list rejection, rule-level error recovery, CSS Text Level 4
`white-space`, percentage/calc letter and word spacing (including inheritance),
and invalid box declarations. This keeps a passing syntax adapter from being
mistaken for proof that the value reaches computed style and layout.

## Reduced adaptations

Some crate-local tests use reduced semantic cases adapted from the
[Web Platform Tests](https://github.com/web-platform-tests/wpt) project. Each
adapted test links to its exact upstream source. Browser scripts, reference
rendering, fonts, and unrelated assertions are intentionally omitted.

Current reduced adaptations, retained as focused crate-local regressions:

- `html-parse`: `blocks.dat`, `adoption01.dat`, and `entities01.dat`;
- `html-style`: `important-vs-inline-002.html` and
  `all-prop-unset-color.html`;
- `html-layout`: `break-spaces-newline-011.html`.

Web Platform Tests is distributed under the following license. The vendored
copy is also retained at `HtmlRenderer/testdata/wpt/LICENSE.md`.

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
