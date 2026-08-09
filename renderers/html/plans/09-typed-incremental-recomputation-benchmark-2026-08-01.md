# Plan 09 benchmark record - 2026-08-01

## Environment and command

The release profile was run from `HtmlRenderer` with:

```text
cargo test --release -p html-view-core -- --ignored --nocapture
```

The two ignored profile tests process the repository `TestData` corpus and
report pipeline timing plus retained laid-out-document memory.

## Result

| Metric | Result |
| --- | ---: |
| HTML files processed | 173 |
| Failed files | 0 |
| Total initial-build time | 1,502.499 ms |
| Mean initial-build time | 8.685 ms/file |
| Mean layout time | 8.502 ms/file |
| Heaviest laid-out document | 3,962.64 KiB |

The heaviest document was `app0025.html` in the Principles of Computer System
Design TestData EPUB. Its largest retained allocations were DOM nodes
(1,536.00 KiB), layout-tree boxes (702.02 KiB), and shaped glyph IDs
(401.50 KiB).

## Cache-retention result

Plan 09 retains immutable prepared and shaped artifacts with `Arc`. The
`phase_wrappers_share_immutable_prepared_and_shaped_inputs` acceptance test
proves pointer identity for `PreparedDocument -> ShapedDocument ->
LaidOutDocument`; a width-only update therefore keeps the original shaped
glyph buffer instead of cloning it into the layout cache. Renderer-owned
auxiliary glyph registration uses copy-on-write, so paint support cannot
mutate a cached pipeline artifact.

The focused width-change test additionally proves that repeated width updates
do not reparse or reshape. The release corpus profile above is retained as the
Plan 09 baseline for future incremental latency and retained-memory comparisons.
