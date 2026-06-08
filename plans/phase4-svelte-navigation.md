# Phase 4 — Svelte UI shell + navigation

## Goal
Replicate the Floem navigation model (global page stack + per-library tab/folder state + persisted reading position) in Svelte 5, *and* fix the dead-code `library_states` cache so per-library state actually persists across switches.

## Source of truth (mirrors Floem types)
| Floem | Svelte equivalent |
|---|---|
| `NavStack { pages, index }` (page_navigation.rs:36) | `nav` rune store |
| `Page` enum (page_navigation.rs:29) | discriminated union |
| `LibraryTab` enum (library_page.rs:14) | `'home'\|'authors'\|'search'\|'folders'` |
| `HomePage.library_states: HashMap` (dead code) | `libraryStates: Map<id, LibraryState>` (live) |
| TOC `CollapseState: Rc<RefCell<HashMap>>` (toc.rs:11) | `Map<string, boolean>` rune |

## Files to create

```
MobileApp/src/
  lib/
    nav.svelte.ts          # nav stack + per-library state cache
    api.ts                 # typed wrappers around invoke()
    types.ts               # mirrors backend serde structs
  routes/                  # not SvelteKit — just folder organization
    Home.svelte
    Library.svelte
    Reader.svelte
  components/
    LibrarySwitcher.svelte # drawer/sheet (replaces Ctrl+L overlay)
    BottomTabs.svelte      # replaces 92px sidebar
    FolderBrowser.svelte
    BookCard.svelte
    AuthorList.svelte
    SearchPanel.svelte
  App.svelte
  main.ts
```

## `nav.svelte.ts` shape
```ts
type Page =
  | { kind: 'home' }
  | { kind: 'library'; libraryId: string }
  | { kind: 'reader'; libraryId: string; bookId: string };

type LibraryState = {
  activeTab: 'home' | 'authors' | 'search' | 'folders';
  folderStack: Array<[string, string]>;
  scroll: Record<string, number>;
};

export const nav = $state({ stack: [{kind:'home'}] as Page[], index: 0 });
export const libraryStates = new Map<string, LibraryState>();

export function navigate(p: Page) {
  nav.stack = [...nav.stack.slice(0, nav.index + 1), p];
  nav.index = nav.stack.length - 1;
}
export function back() { if (nav.index > 0) nav.index--; }
export function current(): Page { return nav.stack[nav.index]; }

export function getLibraryState(id: string): LibraryState {
  let s = libraryStates.get(id);
  if (!s) { s = { activeTab: 'home', folderStack: [], scroll: {} }; libraryStates.set(id, s); }
  return s;
}
```

## State preservation rules (the user's explicit requirement)
- **Library switch**: `Library.svelte` reads `getLibraryState(libraryId)`. On unmount, scroll positions are written back to `s.scroll[tab]`.
- **Tab switch within a library**: tabs render with `style="display:none"` when inactive (matches Floem's hidden-not-destroyed pattern); each tab's component holds its own scroll container, so DOM-level scroll persists for free.
- **Folder navigation**: `s.folderStack` is mutated by `FolderBrowser.svelte`; survives library switches automatically because it lives in the cache.
- **Reader CFI**: foliate-js `relocate` event → debounced `invoke('update_reading_position', {bookId, cfi})`. On reader mount, fetch from backend and `paginator.goTo(cfi)`.

## App shell (`App.svelte`)
```svelte
{#if current().kind === 'home'}
  <Home />
{:else if current().kind === 'library'}
  <Library libraryId={current().libraryId} />
{:else}
  <Reader libraryId={current().libraryId} bookId={current().bookId} />
{/if}
```

`Library.svelte` renders a top app-bar (title + library switcher button) and `<BottomTabs>` showing Home/Authors/Search/Folders. All four panels mount; only active one is `display:block`.

## Hardware back button
`@tauri-apps/plugin-os` doesn't expose Android back directly; in Tauri 2 mobile, the WebView intercepts back via `popstate`. Alternative: use the Tauri 2 mobile `onCloseRequested` window event and call `back()`. If `nav.index === 0`, allow default (exit app).

## Library switcher
Drawer that slides from the left (CSS transform), listing libraries. Tap → `navigate({kind:'library', libraryId})`. The drawer triggers from a hamburger button in the app-bar — replaces the Ctrl+L keyboard overlay.

## Verification
- Switching libraries A → B → A returns user to the *exact* tab and folder depth they were on in A.
- Switching tabs within a library preserves each tab's scroll position.
- Hardware back navigates the page stack; closes app at index 0.
- Reader re-entry resumes at last CFI.

## Out of scope
- Reader rendering (Phase 5).
- Style polish, animations (Phase 6).
