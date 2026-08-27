# Reader keyboard focus and EPUB/PDF sharing — handoff

Written 2026-08-19, mid-work. Related: `docs/reader-keyboard-ownership-plan.md`,
which covers the earlier move of reader keys onto GPUI actions. This document
picks up from the focus bugs that survived it.

Everything described as done compiles: `cargo check -p reader-ui --features pdf`
and `--features kobo` are both clean. `cargo test -p reader-ui --features pdf
--lib` reports **65 passed, 4 failed**; all four failures are pre-existing and
unrelated (see "Not ours" below).

## The focus rule this all turns on

Reader navigation — arrows, space, page up/down — is bound in a **document key
context** that only the document element establishes while it holds focus:
`ReaderDocument` for EPUB (`epub/render.rs`, established in
`html-view-gpui/src/view.rs`), `PdfReaderDocument` for PDF (established per page
element in `pdf-view/src/lib.rs`). The reader shell around it establishes only
`Reader`, which carries `f11`, `ctrl-f` and `escape`.

Two consequences, and every bug fixed here is one of them:

1. If focus moves off the document, paging dies until the document is clicked.
2. If focus lands on an element that stops rendering, GPUI's dispatch falls back
   to the root node and **no** binding fires — not even `ctrl-f`.

CBZ is immune: `cbz_reader.rs:433-434` puts `key_context` and `track_focus` on
the same element, so stealing focus to the container is harmless. That is the
shape the other two should arguably converge on.

## Done — focus fixes

- **Document claims focus on mouse-down.** `html-view-gpui/src/view.rs` and
  `pdf-view/src/lib.rs` now call `window.prevent_default()` after focusing.
  Previously only a click on a glyph, image or table stopped propagation, so a
  click on a margin let a focusable ancestor take focus back.
- **Shell container is focusable only before the document exists.**
  `epub/render.rs` and `pdf_reader.rs` gate `track_focus` on the document not
  being ready. The container stays on the dispatch path either way, so `Reader`
  bindings keep firing.
- **PDF hands the keyboard over on load.** `PdfReaderView::new` focused the
  shell immediately after `PdfView` had focused itself, so PDF keys were dead
  from startup until a click. The handover now happens on the first
  `PageChanged` (`pdf_reader.rs`), mirroring EPUB's `finish_load`.
- **Dismissal paths return focus** — annotation popup Save/Close/Delete in both
  readers, PDF's `close_search`, PDF TOC entries. EPUB's TOC and `close_search`
  already did.
- `Focusable` for both readers answers with the document handle once there is
  one.

## Done — sharing

New modules under `shell/` (which `shell/mod.rs` describes as "mechanics every
reader window needs, written once"):

- **`shell/annotation_editor.rs`** — popup placement (`geometry`) and the panel
  itself (`editor`). The two geometry functions were the same math with the same
  four constants under different names; EPUB keeps a thin wrapper because it
  derives its column count from requested width and scale rather than being told.
- **`shell/state_panel.rs`** — sidebar tabs, annotation rows,
  `reader_toc_tree`, `now_millis`. Moved here
  from a crate-root `reader_state_panel.rs`.
- **`shell/marks.rs`** — `MarksState`. EPUB-only pieces stayed in
  `epub/marks.rs`: `Selection` (PDF never used it), `set_selection`, and
  `overlays`/`renderer_annotation`, which exist to skip PDF annotations.

Behaviour changes that came with it, all deliberate:

- EPUB's annotation popup **gained** Close, four styles and four colours. The
  EPUB renderer already supported all of them (`epub/marks.rs`).
- EPUB's Save no longer dismisses the popup, matching PDF — styles and colours
  can be tried in sequence; Close dismisses.
- EPUB's popup now stops mouse-down propagation, as PDF's already did.
- Element ids were de-namespaced (`reader-*`/`pdf-*` → `reader-state-*`,
  `reader-toc-*`). A window hosts one reader, so they cannot collide, but
  anything keyed on the old ids externally needs updating.

## Open: the line-count question

Johan asked why total code went up. Measured: tracked files −136, new `shell/`
files +445, net **+309** (the −136 also contains unrelated in-flight work).
Scored honestly:

| Extraction | Verdict |
|---|---|
| Popup geometry | Won — identical math, now one copy |
| Annotation editor | Lost on lines; bought EPUB feature parity |
| Sidebar rows | **Lost** — +21 lines, no new behaviour |

`reader_state_rows` only pays off at a third caller or on the next change to
that panel. **This is an open decision, not a settled one** — reverting it is
the cheapest way to claw back lines, and the tabs, geometry and `MarksState`
moves all stand without it.

## Open: `/simplify` review, findings NOT yet applied

Three of four review agents completed; the **altitude review was interrupted and
never ran**. Findings below are unapplied and unverified by me. Ranked by my
judgement of value.

### Worth doing

1. **`StatePanelActions` should hold `fn` pointers, not `Rc<dyn Fn>`**
   (`shell/state_panel.rs`). Every closure at both call sites is non-capturing,
   so six heap allocations per sidebar render plus a dozen refcount clones per
   row are all avoidable. Flagged independently by both the simplification and
   efficiency agents.
2. **Double clones per row** (`shell/state_panel.rs`, the two loops).
   `.iter().cloned()` then a second `.clone()` into the Go closure, per row, per
   frame. `ReaderAnnotation` is ~6 heap allocations. Iterate by reference and
   clone only into the closures.
3. **`doc_fraction` is write-only** (`epub/progress.rs`). Added as a field and a
   sixth positional argument, threaded through six test call sites, never read —
   the one caller passes a literal `0.0` under a TODO. This is Johan's in-flight
   work, not mine; it should land with the commit that emits it.
4. **The annotation editor's action table is duplicated verbatim** in both
   readers (~26 lines each: the same `save` closure, the same style row, the same
   four brand colours). `annotation_editor` should own the standard rows and take
   three callbacks. Flagged by both reuse and simplification agents.
5. **Dead `&mut Context<C>` parameters** on both tab helpers
   (`shell/state_panel.rs`) — never used, but every call site passes `cx`.

### Worth considering

6. **Fold the panel container into the shared module.** Both readers build it
   identically, which contradicts the rationale comment in `state_panel.rs`
   claiming they differ. `reader_state_rows` could return the whole panel.
7. **`close_annotation_popup`/`close_annotation_editor` are the same
   transition**, and `annotation_popup_anchor` lives outside `MarksState` while
   `selected_id` is inside it — a two-field invariant cleared by hand in six
   places. Moving the anchor into `MarksState` would delete both functions.
8. **`save_selected_annotation` bodies are now byte-identical** for their first
   eleven lines in both readers (adding `color` to EPUB completed the copy). A
   `shell::marks::apply_edit` helper would leave each reader only its own
   `persistence::write`.
9. **`now_millis` is in the wrong module** — a clock helper in a widget module,
   all ten callers are persistence code.
10. **EPUB's retained geometry tests duplicate the shared ones** and never
    exercise the one thing the wrapper adds (column derivation from width, gap
    and scale).
11. **PDF minor**: `full_page_position()` is a pure alias for private
    `current_page_position()`; `set_full_page_position` does three inert things
    at its only call site; `full_page_position()` is computed before the
    `unchanged` early-return and thrown away; the digit pre-scan in
    `pdf_page_from_cfi` is redundant with `parse`.

### Deeper question the altitude review was meant to answer

The focus discipline is currently a **convention**: each reader has its own
`focus_document`, and every click handler that dismisses something must remember
to call it. That is how these bugs happened in the first place. Whether a trait,
a focus-restoring wrapper element, or the CBZ shape (document context on the same
element that tracks focus) would make it structural is unexamined. Worth running
before more handlers accumulate.

## Not ours — pre-existing breakage in the tree

- **4 failing tests**: `shell::marks::tests` ×3 and
  `epub::marks::tests::overlays_skip_annotations_the_renderer_cannot_place`. All
  panic in `ContentHash::new("1")`, which now requires 64 lowercase hex
  characters. The fixture needs a valid hash; `fixture_content_hash` in
  library-model looks like the intended source. Three of these moved with
  `MarksState`, so they changed path but not cause.
- **Two out-of-scope repairs I made** because `reader-ui` would not compile at
  all without them, both in Johan's in-flight work — drop if they conflict:
  - `client/library-model/src/lib.rs` — `pdf_page_position_reading_position` and
    `pdf_page_position_from_reading_position` were missing from the `pub use`.
  - `pdf_reader.rs` — `.all(char::is_ascii_digit)` doesn't typecheck (`all`
    passes `char` by value); now a closure.

## Working notes

- Files in `apps/ui/gpui/reader/src/` were being edited **concurrently** by
  another session during this work (`reader_state_panel.rs` appeared mid-stream
  and did not compile — missing `Entity`/`Sizable` imports and an `FnMut` bound
  where `TabBar::on_click` needs `Fn`; fixed). Re-read before assuming.
- `/home` hit 100% full mid-session, which manifests as `ld terminated with
  signal 7 [Bus error]` and rustc incremental-cache write failures, not as a
  code error.
