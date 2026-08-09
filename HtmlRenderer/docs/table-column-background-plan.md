# Plan: CSS table column and column-group backgrounds

Status: deferred; ready for another agent

Priority: low for fiction-focused reading, medium for textbooks, technical
manuals, reports, schedules, and other table-heavy EPUB content.

## Goal

Paint CSS backgrounds authored on `display: table-column-group` and
`display: table-column` across the resolved table grid, in the correct CSS
table-layer order.

The immediate regression is filed under `max-height`, but `max-height` is not
the defect. The property correctly does not apply to a column group. The test
fails because the column group's black background is absent.

Primary WPT acceptance test:

```text
css/CSS2/normal-flow/max-height-applies-to-005.xht
```

Expected result: a filled black 96px square. Current result: the four cell
letters are painted on white because the black column-group background is
never emitted.

Run it with:

```bash
HTML_WPT_GPUI_FILTER=max-height-applies-to-005.xht \
  cargo test -p html-view-gpui --test wpt_reftest \
  runs_pinned_static_wpt_reftests_through_gpui -- --nocapture
```

When it passes, remove this exact entry from
`crates/html-view-gpui/tests/wpt/known-gpui-reftest-failures.txt`, update the README totals from
the then-current ledger counts, and run the cause-registry test.

## Reader benefit

This has little visible benefit for ordinary prose novels. It matters for
table-heavy publications:

- vertical category or comparison bands render correctly;
- highlighted totals, dates, and specification columns retain their meaning;
- light text does not become unreadable when its expected dark background is
  missing;
- financial, scientific, timetable, and educational tables more closely match
  their authored design.

## Current architecture and missing information

Relevant files:

- `crates/html-layout/src/layout/box_builder.rs`
- `crates/html-layout/src/layout_model/layout_tree.rs`
- `crates/html-layout/src/layout/table_layout.rs`
- `crates/html-layout/src/layout/decorations.rs`
- `crates/html-layout/src/layout/box_layout.rs`

`box_builder.rs` currently expands `<col span>` and `<colgroup span>` into
`TableColumnWidthHint` entries. Each entry retains only one `StyleIndices`.
Column and column-group elements deliberately do not become ordinary layout
boxes. This is a good constraint: columns participate in table geometry and
painting, not normal block layout.

The existing metadata is sufficient for width hints, but loses:

1. whether a style came from a column or its column group;
2. which effective columns belong to the same group;
3. the independent background layers needed by CSS table painting.

The resolved geometry already exists in `TableColumnLayout`:

```rust
struct TableColumnLayout {
    widths: Vec<f64>,
    starts: Vec<f64>,
    table_width: f64,
}
```

Row geometry and the final grid height are also known in `layout_table`.

## Proposed data model

Do not create fake block boxes for columns. Replace the width-only metadata
with table-owned structural metadata. One reasonable shape is:

```rust
struct TableColumnTrack {
    column_style: Option<StyleIndices>,
}

struct TableColumnGroupSpan {
    style: StyleIndices,
    start: usize,
    span: usize,
}

struct TableBox {
    // existing rows, row groups, and captions
    columns: Vec<TableColumnTrack>,
    column_groups: Vec<TableColumnGroupSpan>,
}
```

Exact names are not important. The invariants are:

- one effective track per expanded column;
- a column's style remains distinct from its group style;
- group start/span survives box construction;
- width resolution can still read the effective column width hint;
- anonymous/missing columns receive default metadata without panicking.

For a `<colgroup>` containing `<col>` children, record the group start before
expanding its children and its span afterward. For an empty group, expand the
group's own `span` attribute into tracks. A standalone `<col>` has no group
span.

## Paint model

CSS table backgrounds conceptually paint from back to front:

1. table;
2. column group;
3. column;
4. row group;
5. row;
6. cell.

Column and group backgrounds must therefore be emitted before row-group, row,
and cell decorations. Do not merely call `emit_block_background()` at the end
of `layout_table`; that risks placing the new fragments above cell content.

Recommended approach:

1. Capture the decoration insertion point before table descendants emit paint.
2. Finish column widths and row heights.
3. Build table-owned background fragments in CSS layer order.
4. Insert/splice those fragments and their parallel decoration-owner entries at
   the captured position, or refactor table decoration emission so all table
   layers are emitted together in the correct order.
5. Preserve the existing collapsed-border resolution, which must remain above
   backgrounds.

Before choosing insertion versus refactoring, verify the renderer's actual
decoration traversal order. `box_layout.rs` currently emits the table's generic
background/decorations after `layout_table` returns, while row groups and cells
emit during `layout_table`. The implementation must explicitly correct or
account for that ordering rather than relying on call order accidentally.

### Geometry

- Column rectangle X comes from `columns.starts[col]` and
  `columns.widths[col]`.
- Column rectangle Y covers the table grid, excluding captions.
- A group rectangle spans its first through last effective column.
- Correct handling of separated-border spacing must be established with tests;
  the primary WPT uses default separated borders and must have no visible white
  gaps in the expected black square.
- Backgrounds are visible only through transparent layers above them; do not
  suppress cell, row, or row-group backgrounds.

Use the table box as the decoration owner if columns remain non-box structural
metadata. This keeps clipping/finalization ownership valid without inventing
geometry nodes.

## Required focused tests

Add Rust layout/fragment tests near the existing table layout tests. At minimum:

1. **Column-group coverage**: a two-column group with a black background emits
   a rectangle covering the complete grid area.
2. **Property applicability**: `max-height: 0` on the group does not reduce its
   painted grid coverage.
3. **Layer precedence**: a column background appears over its group background,
   and a cell background appears over both.
4. **Group span**: two adjacent groups retain separate spans and paint only
   their assigned columns.
5. **Standalone column**: `<col span="2">` expands and paints both tracks.

Prefer assertions over decoration fragment rectangles/colors and ordering, not
only screenshots. This makes failures distinguish geometry from GPUI raster
parity.

Useful broader WPTs to inspect after the primary test passes:

```text
css/CSS2/backgrounds/background-color-applies-to-005.xht
css/CSS2/backgrounds/background-applies-to-005.xht
```

Some background-image variants depend on unsupported image/background
capabilities and should not be used as initial acceptance criteria.

## Verification

Run in this order:

```bash
cargo test -p html-layout table
cargo test -p html-layout
HTML_WPT_GPUI_FILTER=max-height-applies-to-005.xht \
  cargo test -p html-view-gpui --test wpt_reftest \
  runs_pinned_static_wpt_reftests_through_gpui -- --nocapture
cargo test -p html-view-gpui --test wpt_reftest \
  known_failure_cause_registry_is_well_formed -- --nocapture
```

Also run `rustfmt --check` and `git diff --check` on touched files.

The workspace has recently had concurrent inline/hyphenation and table-layout
work. Start only after `cargo check -p html-layout` succeeds or after confirming
that any remaining compiler errors are owned by the agent responsible for
those edits. Do not repair unrelated in-progress changes as part of this task.

## Non-goals

- Do not add video support.
- Do not add CSS background-image support merely to satisfy related WPTs.
- Do not turn table columns into ordinary block layout boxes.
- Do not rewrite the table width algorithm.
- Do not broaden this into full collapsed-border conflict resolution.

## Completion criteria

The task is complete when:

- the primary WPT matches with no expected-failure entry;
- focused fragment tests prove group/column/cell ordering and spans;
- existing table layout tests pass;
- the failure-cause registry remains well formed;
- no unrelated renderer behavior or unsupported feature is changed.
