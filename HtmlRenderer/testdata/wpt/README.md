# Vendored Web Platform Tests

This directory contains an unmodified, pinned subset of the Web Platform Tests
repository. Normal test runs are offline: they consume these files directly
through `html-wpt-test-support`.

- Upstream: <https://github.com/web-platform-tests/wpt>
- Revision: `7ea3d665ff9ae1df1f742433d51bd1e9c6499a3b`
- Retrieved: 2026-08-01
- License: WPT BSD 3-Clause; see `LICENSE.md`

## Included suites

- all 61 `html/syntax/parsing/resources/*.dat` tree-construction corpora;
- the three `document-compatmode` document-mode tests;
- all 142 HTML files in `css/css-box/parsing`, `css/css-cascade/parsing`,
  `css/css-text/parsing`, and `css/selectors/parsing`.

`SHA256SUMS` pins all 206 upstream files byte-for-byte. The `known-*.txt`
files are local, exact divergence ledgers: unexpected failures and known
failures that start passing both fail the owning test, so the ledgers cannot
silently expand or become stale. The `ignored-*.txt` manifests identify exact
assertion-runs excluded for a named external capability gap; the separately
named parser-stack error-count manifest retains observed dependency diagnostics
for audit context. Passing cases from the same upstream corpus remain active.

Do not hand-edit vendored files. Update the pinned revision, replace the
upstream files byte-for-byte, refresh the hashes, and run the owning crate
tests. Stage adapters may classify unsupported capabilities, but must report
their skip counts and reasons.
