# Hierarchy page rework — handoff

Written 2026-08-12 as a mid-implementation handoff. Design doc:
`docs/hierarchy-page-redesign.html`.

**Completed 2026-08-12.** Desktop and mobile now share the recursive scoped
search behavior and uniform hierarchy presentation. Per the final visual
direction, hierarchy items have no borders: spacing separates them and
`raised_bg` supplies their surface. The standalone Search destination is gone,
bindings are regenerated, desktop folder download is wired, dead title-filter
grid code is removed, and the app has been launched successfully. The sections
below are retained as implementation history; their "Remaining" and "Blocking"
notes are no longer current.

## Decisions this implements

Settled with Johan before starting; do not relitigate without asking.

1. **Tight uniform grid**, fixed-width cells, one line per child, count right-set
   in its own column. A label-width mosaic was drawn and rejected: its counts
   only compare within a row.
2. **Totals only** on the cells. No downloaded figure, no `31/55` pair.
3. **Search is recursive and scoped** to the node being browsed — the folder and
   everything under it, the subject and its narrower subjects. Same on both
   frontends.
4. **Child grid hides while a query is active.** Results come from anywhere
   beneath, so the child list no longer describes what is on screen.
5. **Search page and its navigation entry are removed.** At the root of either
   hierarchy the scope is the whole library, which is all that page ever was.
6. **Book listings stay direct-only** (unchanged behaviour).
7. `downloaded_book_count` **is spent on a desktop folder-download action**.

### Known wart, accepted deliberately

Counts are recursive (`get_sub_dirs` walks a `descendants` CTE;
`get_subject_children` rolls assignments up through `assignment_ancestors`) but
book listings are direct-only. So a subject card reads `41` and opens to fewer
books, with no second figure to explain the gap — decisions 2 and 6 together.
Johan chose both knowing this. Do not "fix" it by adding a direct/total pair
without asking.

## Done

### Shared contract — `client/app-contract`

- `frontend.rs`: `LibraryFeature::Search` removed; `LIBRARY_FEATURES` is now 4
  entries with a comment explaining the absence; `the_two_navigation_groups_cover_every_destination`
  expects 7 rather than 8.
- `cargo test -p app-contract` — 34 passed.

### Desktop components — `apps/ui/gpui/components`

- `library.rs`: `hierarchy_grid()` (wrapping row, 1px hairlines over a tinted
  container) and `hierarchy_cell()` (fixed width, icon + ellipsised name +
  right-set count). `folder_button` / `hierarchy_card` replaced by these.
- `folder_buttons()` **kept** — the store's catalogue links still use it, and a
  handful of links of wildly different lengths is the case a mosaic suits.
- `tokens.rs`: `HIERARCHY_CELL_HEIGHT` 30.0, `HIERARCHY_CELL_MIN_WIDTH` 208.0.

Cells are a fixed px width, not a fraction of the container: the page cannot
measure itself mid-render, and fixed width is what makes counts land on the same
x-position in every row. Cost is a gutter at the right edge.

### Desktop pages — `apps/ui/gpui/browser`

- `library/pages/hierarchy.rs`
  - `HierarchyCrumb` gained `hierarchy_downloaded_book_count()` (currently unused
    — it is there for the folder-download action).
  - Count no longer concatenated into the label; `render_child` builds a cell.
  - `HierarchyState` gained `results: Option<BookGrid>`, `search_generation`,
    `begin_search()`, `is_current_search()`, `set_results()`, `clear_search()`,
    `query()`, `focus_search()`.
  - `HierarchyState::new` takes an `on_query_changed` callback so the page can
    start a search when the box changes.
  - Render: empty query → child grid + own books; non-empty → results grid only,
    with `browser_inline_note("No matching books")` when empty and
    `browser_loading_message(searching_label)` while in flight.
  - `children_matching` and the `FilterCache` are gone.
  - `merge_matches()` flattens `LibrarySearchResult` — title matches, then author
    matches not already claimed by content hash, so a book matching both appears
    once.
- `library/services/context.rs`: `search_folder()`, `search_subject()` added
  beside the existing global `load_search()`.
- `folders/mod.rs`, `subjects/mod.rs`: own a `search_request: LatestRequest`,
  debounce on `browsing_content.search_debounce_ms` (220), reject stale
  generations, expose `focus_search()`.
- Route removal: `route.rs`, `navigation/mod.rs`, `library/mod.rs`,
  `library/pages/mod.rs`; **`library/pages/search.rs` deleted**.
- `root.rs`: `on_focus_search` navigates to Folders only when not already on
  Folders/Subjects, then focuses that page's box. `on_show_search` deleted.
- `lib.rs`: `ShowSearch` action removed; shortcuts renumbered to follow the rail
  — ctrl-5 Store, ctrl-6 Sync, ctrl-7 Settings (was 6/7/8).

`cargo check -p browser-ui` — clean, 2 warnings (see below).

## Blocking

**The contract no longer has `LibraryFeature::Search`, and mobile still
references the `search` destination.** Mobile will not typecheck until the
"Remaining — mobile" items are done. Do not ship the contract change alone.

## Remaining

In the order I would do them.

1. **Mobile** — `apps/mobile/src`
   - `App.svelte`: `.folder-grid` adopts the uniform grid that `.subject-grid`
     already uses (`repeat(auto-fill, minmax(...))`), one line per child, count
     right-set, totals only. `.subject-grid` needs its count moved to a right
     column to match.
   - Remove the `search` destination: `App.svelte` (lines ~273, ~446, ~462 —
     it already redirects `search` → `folders`, so this is deletion),
     `lib/navigation.ts`, `lib/ui-session.ts`, `components/shell/AdaptiveNavigation.svelte`.
   - Mobile search behaviour is **already correct** (`searchLibraryFolder` /
     `searchLibrarySubject` at `App.svelte:1154`) and already hides children
     while a query is active. No change needed there.
2. **Regenerate bindings** — `cd apps/mobile && npm run generate:bindings`. The
   contract shape changed. Then `npm run check` and the node test suites.
3. **Folder download action** (decision 7) — the contract already carries
   `download_folder_label` / `folder_downloaded_label`; mobile's `FolderCard`
   uses them, desktop has nothing. Add it to the desktop cell and let
   `hierarchy_downloaded_book_count()` drive its state. Until this lands, that
   trait method is dead code.
4. **Dead code sweep** — likely unused now, verify before deleting:
   `library/title_filter.rs` `FilterCache` and possibly `title_matches`
   (`normalize_filter` is still used); `widgets/book_grid.rs`
   `matching_title_count`, `render_matching_titles`, `views_matching`.
5. **Update the design doc** — mark implemented, or fold it into a findings doc.
6. **Run the app.** Nothing here has been run, only compiled. See "Not verified".

## Verification status

| | |
|---|---|
| `cargo test -p app-contract` | 34 passed |
| `cargo check -p ui-components` | clean |
| `cargo check -p browser-ui` | clean, 2 warnings |
| `cargo check --workspace` | **not run since the route removal** |
| mobile `npm run check` | **not run — expected to fail, see Blocking** |
| app launched | **no** |

Outstanding warnings: `hierarchy_downloaded_book_count` never used (expected
until item 3); one pre-existing warning unrelated to this work.

## Not verified

- The grid has never been rendered. Cell width (208px), height (30px) and the
  right-edge gutter are guesses from the HTML mockup, not measurements.
- Search has never been exercised against a real library: debounce feel, the
  loading message, stale-generation rejection, and what happens when a query
  returns hundreds of books (the 100-result cap is contract-side and I did not
  confirm the desktop path applies it).
- `merge_matches` dedupe assumes `content_hash` is the right identity for a book
  card. It compiles; it has no test.
- Focus-search from a non-hierarchy page navigates to Folders. That is a
  behaviour change nobody has seen yet.
