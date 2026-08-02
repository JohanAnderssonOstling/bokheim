# Plan 08 Implementation Report (2026-08-01)

## What was implemented in this pass

- Reinforced type-system enforcement for node identifiers:
  - `html-dom` DOM query APIs now require `DomNodeId` directly (removed `impl Into<DomNodeId>` arguments):
    - `element_ref`, `text_ref`, `node_ref`
    - `is_text_node`, `is_element_node`
    - `get_dom_tag`, `get_dom_id`, `get_dom_classes`
    - `dom_has_class`, `get_dom_parent`, `get_dom_namespace`
    - `is_html_element_in_html_document`, `dom_ancestors`
    - `get_dom_href`, `get_dom_attr`, `has_dom_attr`
    - `get_dom_attr_expanded`, `has_dom_attr_expanded`
    - `get_dom_attr_in_any_namespace`, `has_dom_attr_in_any_namespace`
  - These now validate directly through internal lineage/index checks and reject forged/foreign IDs by type-flow constraints.

- Reduced mutable escape hatch in `DocumentBuilder`:
  - Changed `document_mut` visibility from `pub` to `pub(crate)`.

- Enforced typed IDs in style lookups:
  - `html-style-model::ComputedStylesBuilder::set_node_style` now accepts `DomNodeId`.
  - `html-style-model::ComputedStylesBuilder::style_for_node` and `html-style-model::ComputedStyles::style_for_node` now accept `DomNodeId`.
  - `html-style::StyledDocument::style_for_node` now accepts `DomNodeId`.

- Plan document update:
  - Added an explicit external snapshot section in `08-pipeline-boundary-hardening.md` capturing the status per milestone 8a–8f.

## Current status against external snapshot

- 8a Glyph validation: implementation still strong after previous work; this pass did not change glyph code.
- 8b Style validation: unchanged; style value checks remain present and active.
- 8c Builder sealing: this pass advances type-level guarantees by removing unchecked public conversions in query APIs and narrowing builder mutable exposure.
- 8d Render contract: still largely incomplete.
- 8e Hide layout internals: still largely incomplete.
- 8f Transition policy: remains implemented through existing lineage/runtime checks.

## Notes

- No test or build validation was executed in this pass.
- Remaining work should focus on milestones 8d and 8e, plus additional compile-fail tests for typed-ID surface hardening in the public API.
