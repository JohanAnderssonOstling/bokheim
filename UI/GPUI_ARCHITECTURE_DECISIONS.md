# GPUI Architecture Decisions

## 1. Shared library ownership

**Decision:** Add a small `LibraryRegistry` in the GPUI desktop/application layer.

- The registry stores one `Rc<Library>` per library ID.
- Browsing and reader UI crates use the same handle while running on GPUI's UI thread.
- The registry does not belong in the backend; the backend remains unaware of GPUI and windows.
- `Arc` is unnecessary unless a library handle must later cross thread boundaries and `Library` is safely `Send + Sync`.
- The registry prevents duplicate library runtimes and supports lookup for multiple reader windows.

## 2. Library event delivery

**Decision:** Each registry entry owns the backend's single `LibraryEvent` receiver and forwards events into GPUI.

- Browsing views observe GPUI state and refresh immediately.
- Reader actions update the backend and do not notify browsing UI directly.
- Do not clone the crossbeam receiver for multiple consumers.
- Remove the 200 ms polling loop.
- Event delivery remains alive while reader windows are open, even if the browsing window is closed.

## 3. Reader-window identity

**Decision:** Identify books with a composite `BookLocator` containing `library_id` and `book_id`, and identify each reader window with a separate unique `ReaderWindowId`.

- Opening a book always creates a new reader window, including when that book is already open.
- Multiple windows may therefore show the same or different books for cross-reference workflows.
- Each window owns independent navigation, search, and transient reader state.
- If multiple windows show the same book, the most recently persisted position wins.
- Book title and file path are metadata, not identity.
- The reader-window map is keyed by `ReaderWindowId` and retains the associated `BookLocator` for library-wide operations.

## 4. Removal while reading

**Decision:** Do not silently close readers in response to backend removal events.

- If an individual book disappears, keep already-loaded content open and show an unavailable warning.
- If the user removes an entire library from Bokheim, request confirmation and close that library's reader windows first.
- Locate affected reader windows using `BookLocator.library_id`.

## 5. Session persistence

**Decision:** Replace the single-window `current_page` model with browsing-window and reader preference state.

- Persist browsing-window geometry, active library, active section, and folder path.
- Persist global reader preferences and the last reader-window geometry.
- Keep reading positions in the backend.
- Do not automatically reopen reader windows initially.
- Version the session format for future migrations.

## 6. GPUI renderer adapter

**Decision:** Extract the reusable GPUI integration from `HtmlRenderer/gpui-smoke` into an `html-view-gpui` library crate.

- `html-view-gpui` depends on the framework-neutral `html-view-core`.
- `reader-ui` depends on `html-view-gpui` and composes its exported HTML view.
- GPUI painting, glyph shaping, input translation, clipboard handling, timers, repaint handling, and renderer-event forwarding belong in `html-view-gpui`.
- Reader toolbars, TOC, search controls, settings, and window lifecycle remain in `reader-ui`.
- Complete the smoke adapter's missing repaint, event, image, and clipboard support before porting the full reader.

## 7. Asynchronous loading and cancellation

**Decision:** Open reader windows immediately and load their content asynchronously.

- Display an explicit loading state while reading and preparing the EPUB.
- Perform file, database, and safe preparation work outside GPUI rendering.
- Apply a result only if the reader entity still exists and still represents the same book.
- Abandon pending results when the reader closes or changes book.
- Display an actionable error state when loading fails.

## 8. Actions and focus routing

**Decision:** Define semantic GPUI actions centrally and route them through explicit focus contexts.

- Use contexts for browsing, the reader document, reader search, the TOC, and dialogs.
- Buttons, menus, and keyboard shortcuts dispatch the same actions.
- The focused context determines the meaning of shared keys such as `Ctrl+F`, arrows, Enter, and Escape.
- Reader shortcuts must not fire while an input or dialog owns focus.

## 9. Browsing-window lifecycle

**Decision:** Maintain at most one browsing window per process, without an in-app action for reopening it.

- Closing the browsing window leaves reader windows open.
- To browse again, the user relaunches Bokheim.
- Relaunching may create a separate process while existing reader windows remain open.
- Cross-process duplication of library runtimes and scanners is accepted; do not add single-instance coordination.
- Each reader opening still creates a new reader window.

## 10. Shared UI components

**Decision:** Keep `bokheim-ui-components` limited to reusable presentation.

- It owns theme tokens, icons, and styled wrappers for buttons, toolbars, cards, panels, and common loading, empty, and error states.
- It may wrap `gpui-component` primitives.
- It must not access the backend, `LibraryRegistry`, reader-window management, authentication, or persistence.
- Browser and reader behavior remains in their feature crates.

## 11. Large-library rendering

**Decision:** Design browsing collections for virtualization and asynchronous cover loading from the beginning.

- Render book grids as virtualized rows and virtualize author lists.
- Load and decode covers asynchronously.
- Share a bounded cover-image cache across browsing views.
- Render only the active browsing section while retaining its presentation state.

## 12. GPUI crate topology

**Decision:** Use separate `bokheim-desktop`, `browser-ui`, `reader-ui`, `bokheim-ui-components`, and `html-view-gpui` crates.

- `bokheim-desktop` composes the application.
- `browser-ui` and `reader-ui` do not depend on each other.

## 13. Open-reader interface

**Decision:** Browsing sends a `BookLocator` through a desktop-provided callback, and the desktop opens a new reader window.

## 14. Shared GPUI state

**Decision:** Keep only the library registry, authentication, theme, and preferences in shared application state. Feature-specific state remains in its window entities.

## 15. Browsing navigation

**Decision:** Use Home, Authors, and Folders as browsing sections, with Search globally accessible instead of a permanent section.

## 16. Reader layout

**Decision:** Use a fixed resizable TOC/document split and a settings popover. Do not add docking or reader tabs.

## 17. `gpui-component` usage

**Decision:** Initialize `gpui-component` once, wrap every GPUI window in `Root`, and use its inputs, dialogs, popovers, notifications, and resizable panels where applicable.

## 18. Development diagnostics

**Decision:** During the current development phase, report failures through logs only. Do not add user-facing error and feedback surfaces yet.

## 19. GPUI dependency source

**Decision:** Use the sibling `../GPUI-Fork/crates/gpui` path dependency, version 0.2.2.

- Configure the workspace so `gpui-component` resolves to the same GPUI fork.
- Do not depend on RustTrados's vendored GPUI directory.

## 20. Floem coexistence

**Decision:** Keep the existing Floem implementation. Build the GPUI implementation alongside it rather than replacing or removing Floem.

## 21. Verification scope

**Decision:** Do not add a dedicated verification phase, entity tests, renderer-adapter tests, or multi-window smoke tests as part of this work.

## 22. Styling ownership

**Decision:** Keep styling out of browser and reader behavior code.

- Put reusable, named functions that return fully styled components in `bokheim-ui-components`.
- Feature crates compose those components and attach content, actions, and event handlers.
- Do not add inline layout, spacing, color, border, or typography chains to feature views.

## 23. Floem presentation parity

**Decision:** Treat the Floem UI crate as the visual and library-navigation baseline.

- Reuse its palette, typography, spacing, radii, cover proportions, and elevation conventions.
- Preserve its top-level library switcher, 96 px section rail, Home carousels, author cards and folder-grouped author results.
- Preserve its resizable folder-tree workspace, active tree rows, collapse state, and breadcrumbs.
- Keep intentional GPUI architecture differences: virtualized large collections, global search, and separate reader windows.
