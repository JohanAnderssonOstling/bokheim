# Reader keyboard ownership

Move EPUB reader keyboard dispatch from the HTML view's raw `on_key_down` up
into GPUI's action system, without forking the cross-platform shortcut table.

## Why

The EPUB reader currently has two overlapping keyboard systems.

`HtmlView::on_key_down` (`apps/renderers/html/crates/html-view-gpui/src/view.rs`)
resolves raw keystrokes against the shared contract table and drives the
renderer directly. Separately, the reader declares GPUI actions
(`apps/ui/gpui/reader/src/lib.rs`) and wires `on_action` handlers for
`PreviousPage`/`NextPage` that no `KeyBinding` ever targets, plus bindings for
history and font size that duplicate what the view already handles.

Consequences:

- `PreviousPage`/`NextPage` actions and their handlers are unreachable.
- `Alt+Left/Right` (history) and `Alt+=/-` (font size) exist in both systems;
  the focused HTML element consumes the key first, so the GPUI bindings are
  effectively dead.
- Toolbar tooltips describe GPUI bindings while behaviour comes from the view,
  so the two drift silently. This already produced tooltips advertising
  `Ctrl++` against a `ctrl-=` binding.
- Reader keys are invisible to GPUI's action system: not rebindable, not
  discoverable, not inspectable.

## Current layering

1. **Policy** — `client/app-contract/src/frontend.rs`
   `READER_NAVIGATION_SHORTCUTS`: 11 entries mapping
   `(key, primary_modifier, alt, shift)` to `ReaderNavigationAction`.
   Serialised as `ReaderNavigationContract` inside `FrontendContract`, so the
   mobile Svelte app consumes the same table. `ReaderNavigationKey::from_key_name`
   accepts both DOM (`"ArrowLeft"`) and GPUI (`"left"`) key names.

   | Keys | Action |
   |---|---|
   | `Up` / `Down` | Previous/Next line |
   | `Left` / `Right` | Previous/Next page |
   | `PageUp` / `PageDown` | Previous/Next page |
   | `Space` | Next page |
   | `Ctrl+Left` / `Ctrl+Right` | Previous/Next section |
   | `Alt+Left` / `Alt+Right` | History back/forward |

2. **Dispatch** — `html-view-gpui/src/view.rs`
   `HtmlView::on_key_down` resolves against the table and calls layer 3. Three
   branches sit outside the table: `Alt` + `=`/`+`/`-` font size, `Ctrl+C`
   copy, `Escape` clear table selection. The element also establishes the
   `ReaderDocument` key context during paint.

3. **Mechanism** — `RendererSession` (html-view-core)
   `previous_page`, `next_page`, `previous_line`, `next_line`,
   `previous_document`, `next_document`, `navigate_history_back`,
   `navigate_history_forward`, `change_root_font_size`, `copy_selection`,
   `clear_table_selection`.

Layer 3 is already correctly placed. This plan moves layer 2 into the reader
and derives the reader's bindings from layer 1.

## Target

GPUI owns event handling; html-view-core still implements the operations.

- `bind_keys` generates `KeyBinding`s from `READER_NAVIGATION_SHORTCUTS` rather
  than hand-writing them, so the contract stays the single source of truth for
  desktop and mobile alike.
- Bindings target the `ReaderDocument` context, which the HTML document element
  already establishes, so they fire only when the document has focus and never
  while the search input does. `Space` stays safe to bind for that reason.
- `HtmlView` exposes each navigation operation as a public method; the reader's
  `on_action` handlers call them through the existing `with_renderer` helper.
- `HtmlView::on_key_down` keeps only what the view legitimately owns:
  `Ctrl+C` copy and `Escape` clear-table-selection.

## Steps

1. **Widen `HtmlView`'s public API.** Only `next_page`/`previous_page` are
   public today. Add wrappers for `previous_line`, `next_line`,
   `previous_document`, `next_document`, `navigate_history_back`,
   `navigate_history_forward`, `change_root_font_size`. Each notifies as the
   existing page-turn wrappers do.

2. **Add the four missing actions** to `reader/src/lib.rs`: `PreviousLine`,
   `NextLine`, `PreviousSection`, `NextSection`.

3. **Generate bindings from the contract.** In `epub/render.rs::bind_keys`,
   map each `ReaderNavigationShortcut` to a GPUI keystroke string
   (modifier prefixes plus the key name) and to its corresponding action, all
   in the `ReaderDocument` context. Remove the hand-written `alt-left`/
   `alt-right` history bindings, which the table now supplies.

4. **Wire the reader's `on_action` handlers** for the four new actions, calling
   the new `HtmlView` methods via `with_renderer`.

5. **Strip navigation from `on_key_down`.** Delete the
   `resolve_reader_navigation_shortcut` branch and the `Alt` font-size branch.
   Keep `Ctrl+C` and `Escape`.

6. **Fix focus on open.** `epub/state.rs` focuses the reader's page-level
   handle, not the HTML view's. Navigation bindings live in `ReaderDocument`,
   so the document must hold focus for them to fire. Focus the view's handle
   when the reader becomes ready.

7. **Restore focus on search close.** `close_search` clears the query and
   deactivates search but leaves focus on the hidden input, leaving document
   keys dead until the user clicks. Return focus to the document.

## Decisions

- **Font size stays a reader-level action**, not a contract entry. It is not
  navigation. The `on_key_down` branch is deleted and the existing
  `IncreaseFont`/`DecreaseFont` bindings become the only path.
- **History narrows from `Reader` to `ReaderDocument` scope.** `Alt+Left/Right`
  currently works while the search box has focus; after this it will not.
  Accepted: it is document navigation.
- **`Ctrl+C` and `Escape` stay in the view.** Both concern selection state the
  view owns exclusively.

## Risks

- Steps 6 and 7 are load-bearing. Before this change a raw key handler caught
  events regardless of which handle held focus; afterwards, focus in the wrong
  place means no navigation at all. Both must land with the rest.
- `ReaderNavigationKey` to GPUI keystroke mapping must match GPUI's key names
  (`left`, `right`, `up`, `down`, `pageup`, `pagedown`, `space`). PDF and CBZ
  already bind `pageup`/`pagedown`, confirming those spellings.
- The generated bindings must not collide with `gpui-component`'s `Input`
  bindings. They cannot: `ReaderDocument` is established by the document
  element, and the search input sits under `ReaderSearch`.

## Verification

- `cargo check` across the workspace.
- Unit test asserting every `READER_NAVIGATION_SHORTCUTS` entry produces a
  GPUI keystroke string, so a new contract entry cannot silently fail to bind.
- Manual: open an EPUB, page with `Left`/`Right`/`Space`/`PageUp`/`PageDown`
  without clicking first; `Ctrl+Left`/`Right` moves section; `Alt+Left`/`Right`
  moves history; `Ctrl+F` then `Escape` returns keyboard control to the page.
