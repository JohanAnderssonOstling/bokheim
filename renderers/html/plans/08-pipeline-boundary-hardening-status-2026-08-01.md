# Plan 08 status report (2026-08-01)

## External assessment snapshot (post-navigation/render migration)

- Overall completion: **~78–82%**
- Validation half: **well underway**
  - `8a` glyph metric validation substantially complete.
  - `8b` computed-style value validation substantially implemented.
  - `8c` DOM sealing significantly advanced with typed builder states and stricter root checks.
- Representation-isolation half: **now substantially complete**
  - `8d` render contract migration has been largely completed through `html-view-core` and key in-crate call sites.
  - `8e` layout internals are now largely hidden behind render-side view APIs and non-public `LaidOutDocument` access.

## Current work completed since the earlier snapshot

- `html-view-core` fully moved from internal line access in `renderer/render.rs` and `renderer/nav.rs` to render-contract APIs.
  - Removed direct line access via `document.lines()` / `document.line()` in these modules.
  - Replaced field reads with `RenderLine` accessors (`point()`, `height()`, `glyphs()`, etc.).
  - Switched decoration iteration to `render_view().decorations()` and `RenderDecoration` methods.
- Test-only internal access in `session.rs` was migrated to `render_view().line(...)`.
- Cleared a lingering unused local in line glyph draw path.

## Updated milestone status

- 8a Glyph validation: **~85–90% complete**
- 8b Style validation: **~90% complete**
- 8c Sealed DOM builder: **~65% complete**
- 8d Render contract migration: **~90% complete**
- 8e Hide layout internals: **~80–85% complete**
- 8f Transition policy: **~90% complete**

## What remains

- Confirm no layout-internal coupling in remaining HTML renderer adapters (GPUI/Floem/other consumers outside `html-view-core`).
- Finalize 8e by completing remaining acceptance coverage and ensuring boundary checks remain current.
- Expand acceptance tests around stateful builder transitions and render-boundary behavior.

## Output produced

- New implementation report saved at:
  - `HtmlRenderer/plans/08-pipeline-boundary-hardening-implementation-report-2026-08-01.md`
