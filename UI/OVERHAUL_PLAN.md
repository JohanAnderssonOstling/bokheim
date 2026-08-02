# UI Overhaul Plan

Proposed UX/visual overhaul of the Bokheim desktop app (floem UI), grounded in the
current state of `UI/src`. Items are tagged with rough effort: **tiny / small /
medium / large**.

## Recommended starting order

1. **Virtualized grids** — fixes the real black-screen bug in Authors.
2. **Session persistence** — scaffolding already exists, high payoff.
3. **Reader settings popover** — biggest day-to-day reader annoyance.

---

## 1. Visual foundations

### 1.1 Dark mode *(small–medium)*
`style.rs` has two light palettes (Greco, Library Serif) behind a clean
`ThemePalette` struct; a dark palette is mostly filling in the ~20 colors.

Blocker to fix first: the nav rail SVG icons in `library_page.rs` hard-code
`stroke="#374151"` and will not follow any theme. Make icon color come from the
palette (substitute the stroke from the active palette, or render icons via
color-aware paths).

### 1.2 Reading themes in the reader *(medium)*
Sepia/dark page background and reading font choice matter more in an EPUB app
than app chrome. The renderer already exposes font-size/scale/column-width
signals; extend that settings group. Needs HtmlRenderer support for page
background/text colors.

### 1.3 Focus & hover consistency *(small)*
Keyboard shortcuts exist (Ctrl+1–4, arrows/enter in the switcher) but nothing
shows focus. Add a focus-ring style to the shared bases (`base_button`,
`ui_label` interactive derivatives) — one place now, thanks to the layering.

## 2. Shell & navigation

### 2.1 Wire up session persistence *(medium, high payoff)*
`LibraryStateData::from_data`/`snapshot` in `library_page.rs` are written but
never called (the current dead-code warnings). Finish the loop:

- persist active library id, active tab, folder stack across launches
- persist window size/position
- optional: "reopen last book" on startup

### 2.2 Hide "Inspect" behind a debug flag *(tiny)*
It is a dev tool sitting in the main top bar; gate it with
`#[cfg(debug_assertions)]`.

### 2.3 Library switcher → command palette *(small–medium)*
The switcher already has arrow/enter/escape handling. Add a text input with
fuzzy filtering and it becomes a proper Ctrl+L palette instead of a
click-through modal.

## 3. Library browsing

### 3.1 Virtualize the grids *(medium, fixes a real bug)*
Large non-virtualized card grids blow the vello scene budget — the Authors tab
goes silently black past a window-size threshold (see memory:
floem-vello-scene-budget-black-screen). Use `virtual_stack`. This also lets us
delete the "Load more authors" pagination (400 visible + 200 per click).
Folders book grids have the same risk for big folders.

### 3.2 Build the Search tab *(medium)*
`search_tab.rs` is a stub rendering the word "Search". v1: title/author
substring search over the library DB, results in the existing books grid,
focus the input on tab switch.

### 3.3 Real cover placeholder *(small)*
Books without covers render the literal text "Placeholder". Replace with a
generated cover: title + author on a palette-tinted card.

### 3.4 Card affordances *(medium)*
- read-progress bar on covers everywhere (today only home carousels show progress)
- right-click context menu: open, mark finished, reveal folder
- optional: details side panel on selection

## 4. Reader

### 4.1 Settings dropdown → popover panel *(small–medium)*
The current dropdown is a full-width strip that pushes content down and mixes
steppers with a 13-line wall of shortcut labels. Split it:

- compact popover anchored to the ☰ button for settings
- shortcuts move to a `?`-key overlay modal (reuse `centered_modal` + scrim)

### 4.2 Progress affordance *(small)*
Percent-through-book and/or "page x/y" in the top or bottom bar. The CFI
position signal already exists and feeds the progress DB.

### 4.3 Collapsible TOC panels *(small)*
Toolbar toggle buttons for both panels. The right "document headers" panel is
narrow (160px default) and always present even when the chapter has no
headers.

## 5. Errors & feedback (cross-cutting) *(small–medium)*
Failures currently go to `eprintln!` (e.g. `create_library` failure in
`home_page.rs`) — the user sees nothing. Add:

- a toast component (components layer) + `RwSignal<Vec<Toast>>` in `Signals`
- surface library-create/auth/sync errors through it
- empty states with a call to action (e.g. "Add a library" button in the empty
  home view)
