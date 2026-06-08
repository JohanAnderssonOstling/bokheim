# Tauri 2 Android Port — Plan Index

## Context
Add a Tauri 2 Android app **alongside** the existing Floem desktop app. Both are first-class targets and share the `Backend/` crate. The Floem `UI/` crate stays as the desktop frontend; a new `MobileApp/` (Tauri 2 + Svelte 5 + Vite + foliate-js) becomes the mobile frontend. The two frontends are parallel — neither replaces the other. Mirror the Floem navigation model in the mobile UI (global page stack + per-library tab/folder state + persisted reading position), and *fix* the dead-code per-library state cache (`HomePage.library_states`) on the mobile side (and optionally desktop later) so navigation state actually survives library switches.

## Phases (execute in order)

1. [Phase 1 — Tauri 2 + Svelte scaffold](phase1-tauri-svelte-scaffold.md)
2. [Phase 2 — Backend mobile adaptation](phase2-backend-mobile-adaptation.md)
3. [Phase 3 — Tauri command layer](phase3-tauri-commands.md)
4. [Phase 4 — Svelte UI shell + navigation](phase4-svelte-navigation.md)
5. [Phase 5 — foliate-js reader integration](phase5-foliate-reader.md)
6. [Phase 6 — Android packaging & polish](phase6-android-packaging.md)

## Key decisions (locked)
- **UI**: Svelte 5 + Vite (no SvelteKit).
- **Reader**: foliate-js (vendored, no npm package).
- **Tauri**: v2 mobile (Android first, iOS later).
- **Backend reuse**: `Backend/` crate is shared by both desktop (Floem) and mobile (Tauri) builds. Almost all code is unified; only the `notify` filesystem watcher is `#[cfg]`-gated (no Android equivalent).
- **Floem `UI/` crate stays**: desktop build continues to work unchanged. Mobile gets a new `MobileApp/` directory; the two are parallel siblings.
- **`HtmlRenderer/`**: still used by desktop (Floem); not linked into the mobile target (foliate-js renders on mobile).
- **Storage (mobile)**: app-private only. Each library is a directory under `{app_data}/libraries/<uuid>/`, created and managed by the backend. **No SAF, no JNI.** Books arrive via sync (server-driven) or single-file `ACTION_OPEN_DOCUMENT` import. Multi-library is preserved.
- **Storage (desktop)**: unchanged — user-picked paths via Floem's existing folder picker.
- **`create_library` API**: unified `create_library(name, root: Option<PathBuf>)`. Desktop passes `Some(picked_path)`; mobile passes `None` and the backend mints a managed root. Same backend code, different call sites.
- **SAF-backed library locations**: explicitly deferred. Documented as a future addition in Phase 6.

## Critical files (touched across phases)
- `Backend/src/lib.rs` — rebuild as `cdylib`, expose Tauri commands
- `Backend/src/home.rs` — replace `directories-next` with Tauri paths
- `Backend/src/scanner.rs` — gate only the `notify` watcher on mobile; `walkdir` is unified
- `Backend/src/epub.rs` — accept `Read + Seek`, not `PathBuf`
- `Backend/src/library.rs` — drop `get_book_file_path`; add `open_book_reader`
- `Backend/src/home.rs` — `create_library(name, Option<PathBuf>)` signature
- New: `MobileApp/` (Tauri 2 project root) with `src-tauri/` and `src/` (Svelte) — sibling to the existing `UI/` Floem crate, not a replacement

## Workspace structure after the port
```
Html-Prototype/
  Backend/         # shared — both targets link this
  HtmlRenderer/    # desktop-only (still used by Floem)
  UI/              # desktop Floem app — UNCHANGED
  MobileApp/       # NEW — Tauri 2 + Svelte 5 mobile app
  SyncCommon/      # shared
```
