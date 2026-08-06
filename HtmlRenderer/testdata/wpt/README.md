# Pinned Web Platform Test Corpus

This directory contains the unchanged, pinned upstream Web Platform Test files
used as a shared local corpus. Set `HTML_WPT_ROOT` to this directory's absolute
path when running engine or GPUI integration suites.

Project-owned test selections and expected-result ledgers do not live in this
corpus:

- engine manifests and nonvisual fixtures live under `testdata/wpt` in
  <https://github.com/JohanAnderssonOstling/HtmlEngine>;
- GPUI reftest manifests and failure ledgers live in
  `HtmlRenderer/crates/html-view-gpui/tests/wpt`.

- Upstream: <https://github.com/web-platform-tests/wpt>
- Revision: `7ea3d665ff9ae1df1f742433d51bd1e9c6499a3b`
- Retrieved: 2026-08-01
- License: WPT BSD 3-Clause; see `LICENSE.md`

## Included suites

- all 61 `html/syntax/parsing/resources/*.dat` tree-construction corpora;
- the three `document-compatmode` document-mode tests;
- all 1,267 HTML files under every CSS `parsing` directory, 233 additional
  declarative CSS helper-test files outside those directories, and the local
  static script data referenced by that corpus;
- 914 noninteractive `check-layout` tests and their shared static resources,
  selected without a browser DOM or JavaScript runtime. They contain 21,483
  width/height assertions that the adapter can evaluate and 18,805 additional
  browser-geometry assertions that its semantic layout view audits but cannot
  yet evaluate;
- 21,544 unchanged, script-free visual reftests and their local
  test/reference/resource closure across CSS, HTML rendering, SVG, MathML,
  fonts, image formats, quirks, and compatibility suites. They are rendered at
  800 x 600 and device scale 1 through the production GPUI paint adapter and a
  feature-gated WGPU headless target.

`SHA256SUMS` pins all 36,104 upstream files byte-for-byte. The owning projects'
`known-*.txt` files are exact divergence ledgers: unexpected failures and known
failures that start passing both fail the owning test, so the ledgers cannot
silently expand or become stale. The `ignored-*.txt` manifests identify exact
assertion-runs excluded for a named external capability gap; the separately
named parser-stack error-count manifest retains observed dependency diagnostics
for audit context. Passing cases from the same upstream corpus remain active.

Do not hand-edit vendored files. Update the pinned revision, replace the
upstream files byte-for-byte, refresh the hashes, and run the owning crate
tests. Stage adapters may classify unsupported capabilities, but must report
their skip counts and reasons.

## GPUI visual reftests

Run the complete visual selection with:

```sh
HTML_WPT_ROOT=/absolute/path/to/wpt cargo test -p html-view-gpui --test wpt_reftest
```

The runner reuses a headless GPUI application, WGPU device, window, render
pipeline, and texture atlas across pages. It periodically refreshes the harness,
refreshes immediately after a caught panic, and bounds the repeated-reference
image cache to avoid corpus-size-dependent memory growth. WPT `match`,
`mismatch`, and `fuzzy` metadata are consumed directly from the unchanged HTML;
fuzzy ranges use their specified upper bounds and may be scoped to a reference.

The manifest is deliberately noninteractive. Both tests and references are
rejected if they contain scripts or `reftest-wait`; server-substituted `.sub.`
documents are also excluded. Tests carrying the WPT `http` flag are retained
in the manifest but skipped through `unsupported-http-gpui-reftests.txt`; the
static reader harness does not synthesize HTTP response metadata. Static tests
whose test or reference enters quirks or limited-quirks mode are retained but
skipped through the exact `unsupported-quirks-mode-gpui-reftests.txt` ledger;
production DOM construction rejects either mode. Static tests for other
currently unsupported features,
fonts, and image formats remain active and are represented by exact failures
rather than capability skips. The current ledgers classify the 21,544 tests as
10,297 expected direct matches, 11,099 active tracked failures, 124 unsupported
quirks-dependent tests, and 24 unsupported HTTP tests. Of the active failures,
6,148 are not currently covered by an unsupported, dependency, or
infrastructure cause and are therefore the actionable supported-feature upper
bound; 4,951 are explicitly classified outside that backlog. A new mismatch, a
missing ledger entry, or a known failure that starts passing fails the runner.

Known failures that have been explicitly triaged also have a machine-readable
entry in `known-gpui-reftest-causes.txt` using
`TEST | CATEGORY:FEATURE | EXPLANATION`. Unsupported features are recorded
there rather than removing their tests from the manifest. Cause entries must
refer to a current manifest failure; once support makes a test pass, the runner
rejects the stale failure and cause entries so both can be removed together.

`supported-css2-known-gpui-reftest-failures.txt` is the current actionable
CSS2 backlog for the static book renderer. Its audit companion,
`intentionally-unsupported-css2-gpui-reftest-failures.txt`, records CSS2
failures excluded because a feature is intentionally unsupported or because
the behavior is owned by a dependency, together with the exact reason. These
are
triage views rather than runner inputs; the exact conformance ledger remains
`known-gpui-reftest-failures.txt`.

For local or CI parallelism, split the sorted manifest deterministically with
`HTML_WPT_GPUI_SHARD=1/4` through `4/4`; `HTML_WPT_GPUI_RANGE=START..END` can
further divide a selected shard for diagnosis or constrained CI workers.
Renderer panics are caught per page and retained as exact failures, so one
malformed or unsupported document cannot hide results later in the manifest.
Ten observed non-terminating cases remain in both the manifest and failure
ledger but are isolated by `known-gpui-reftest-timeouts.txt`; probe them
directly when investigating the underlying layout issue.

Regenerate the static selection from a checkout of the pinned revision with:

```sh
python3 HtmlRenderer/scripts/import-gpui-wpt-reftests.py \
  --upstream /path/to/wpt \
  --destination HtmlRenderer/testdata/wpt \
  --manifest HtmlRenderer/crates/html-view-gpui/tests/wpt/gpui-reftests.txt
```

The importer refuses to overwrite a vendored file if it differs from the
pinned upstream bytes. It recursively imports local references, stylesheets,
fonts, images, and other discoverable static resources, updates `SHA256SUMS`,
and writes the exact manifest count.
