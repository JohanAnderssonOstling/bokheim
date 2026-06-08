# Phase 5 — foliate-js reader integration

## Goal
Render EPUBs in `Reader.svelte` using foliate-js, with reading position synced to the backend via the existing `update_reading_position` command and TOC navigation matching the Floem reader's behavior.

## Vendoring foliate-js
foliate-js has no npm package. Add as a git submodule:
```
git submodule add https://github.com/johnfactotum/foliate-js MobileApp/src/vendor/foliate-js
```
Vite serves it as static ES modules. Key exports we use:
- `view.js` (the `<foliate-view>` custom element) or compose `paginator.js` + `epub.js` directly
- `epub.js` (the foliate one — *not* futurepress epub.js) for parsing

## Loading model
Two viable paths:

**Path A (recommended) — `<foliate-view>` web component.**
```svelte
<script>
  import 'src/vendor/foliate-js/view.js';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  let view;
  export let bookId; export let libraryId;

  async function load() {
    const meta = await invoke('open_book', { libraryId, bookId });
    // meta.handle: { kind: 'path', path } | { kind: 'uri', uri }
    const blob = await fetchBookBlob(meta.handle);
    await view.open(blob);
    if (meta.cfi) view.goTo(meta.cfi);
    view.addEventListener('relocate', onRelocate);
  }

  const onRelocate = debounce((e) => {
    invoke('update_reading_position', { bookId, cfi: e.detail.cfi });
  }, 500);
</script>

<foliate-view bind:this={view} on:mount={load}></foliate-view>
```

**Path B — manual paginator** (more control, more code). Use only if Path A's defaults conflict.

## Getting the book bytes to foliate-js
Three options depending on backend:
1. **Desktop / app-private path**: use Tauri asset protocol — `convertFileSrc(path)` and let foliate-js fetch it.
2. **Android SAF URI**: backend exposes a `read_book_bytes(bookId) -> Vec<u8>` command (or streams via custom URI scheme). Pass a `Blob` to foliate-js.
3. **Custom URI scheme**: register `tauri://book/<id>` in `tauri.conf.json` and stream chunks. Best for large books (avoids loading whole EPUB into memory). Recommended once it works.

Start with option 2 for simplicity; switch to 3 if memory pressure on Android.

## TOC
foliate-js exposes `view.book.toc` — array of `{label, href, subitems}`. Render with the same hierarchical pattern as the Floem `toc.rs`:
- Collapse state in a `Map<string, boolean>` rune (mirrors `CollapseState` in toc.rs:11).
- `active_toc_entry` (toc.rs equivalent) — track from `relocate` events; highlight the matching entry.
- Mobile presentation: bottom sheet (drag-up from a toolbar button), full-height when expanded. Replaces the desktop sidebar TOC.

## Reading-position semantics
- foliate-js emits CFI strings in `relocate.detail.cfi`; pass through unmodified to `update_reading_position`.
- On book open, `meta.cfi` from `open_book` → `view.goTo(cfi)` *after* `view.open(blob)` resolves.
- Also call `update_last_read` on first load (matches Floem behavior in epub_reader.rs:83).

## Reader UI chrome
- Top bar: back button, book title, TOC button, settings (font, theme).
- Bottom: progress bar + page indicator (`view.relocate.detail.fraction`).
- Tap-to-toggle chrome (foliate convention): single-tap center 1/3 of screen toggles bars.
- Page turn: tap left/right thirds, or swipe — foliate-view handles natively.

## Settings (basic)
- Font size: `view.renderer.setStyles({...})`
- Theme (light/dark/sepia): swap CSS variables in foliate-view's shadow root
- Persist settings in `localStorage` (purely UI state — backend doesn't need to know).

## Verification
- Open a book → first page renders, CFI from previous session restored.
- Page turns work via swipe and tap.
- TOC opens, tapping an entry jumps; current entry highlighted.
- Close + reopen book → resumes at exact CFI.
- Switch from reader → library → back to reader: still at correct position.
- Test EPUBs: at least one EPUB 3 with fixed-layout, one RTL, one with MathML.

## Out of scope
- Annotations / highlights (future).
- Cross-device sync of position beyond what `sync.rs` already does.
- Reading statistics.
