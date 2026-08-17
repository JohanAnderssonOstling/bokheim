# gpui desktop design findings

Design audit of the gpui desktop UI — `apps/ui/gpui/{components,browser,reader}`, the theme
bridge in `browser/src/stores/preferences.rs`, and the layout contract in
`client/app-contract/src/{frontend,preferences}.rs`.

Read at the code level, not by rendering the app. Nothing here was measured from a
screenshot; the contrast figures are computed from the palette constants and the
size/spacing counts are from the source. Anything that depends on how it actually looks
on screen is called out as unverified.

Part 1 covers the design system — palette, scale, typography, component styling.
Part 2 covers layout flow and information architecture, and matters considerably more.

---

## What already works

- **A real component layer.** `apps/ui/gpui/components/` owns styling; pages compose.
  Almost nothing under `browser/src/**/pages` sets its own colors or paddings, so most
  findings below are fixable in a handful of files.
- **Semantic theme indirection.** `BrowserTheme` (`components/src/tokens.rs:5`) exposes 12
  intent-named colors mapped out of `gpui_component`'s theme. Only 16 literal colors
  appear across ~16.5k lines.
- **Text contrast passes AA everywhere.** Body ink 13.5:1, muted text 4.8–4.9:1,
  accent-on-white 6.0–6.6:1, danger 6.0–6.9:1.
- **Layout constants come from the shared contract**, so the mobile and desktop shells
  agree on cover size, rail width, and topbar height.
- **Retained entities.** `BookGrid::set_books` reuses card entities across refreshes
  (`browser/src/library/widgets/book_grid.rs:113`), and `ReaderToolbar::set_model`
  repaints only on a real change (`reader/src/epub/toolbar.rs:96`).
- **Absolute-width reader split** that survives window resize
  (`components/src/reader.rs:399`), and an aspect-aware image popup with a test (`:277`).

---

# Part 1 — Design system

## 1.1 There is no dark theme

`ApplicationTheme` has exactly two variants (`client/app-contract/src/preferences.rs:42`):
*Greco futuristic* (`app_bg #eeebe2`) and *Library serif* (`app_bg #efe8db`). Both are warm
light creams. Nothing reads `cx.window_appearance()`.

For a desktop reading app this is the most visible gap. The `ApplicationPalette` struct is
already the right shape — this is two new palette constants plus system-appearance follow.

## 1.2 The surfaces don't separate

Contrast ratios against `app_bg`, Greco palette:

| token | vs `app_bg` |
| --- | --- |
| `hover_bg` | **1.003** |
| `surface_bg` | 1.085 |
| `white_surface` | 1.134 |
| `card_bg` | 1.164 |
| `border` | 1.304 |

`card_bg` vs `white_surface` is **1.027** — four tokens that are the same cream.
`hover_bg` is luminance-identical to `app_bg` in *both* palettes (1.003), so hover on the
page background is invisible. Card and panel definition therefore rests entirely on 1px
borders at **1.3:1**, under the 3:1 floor in WCAG 1.4.11 for UI boundaries. Shadows don't
help: `shadow` is the border color at α 0.25 (`components/src/tokens.rs:22`), i.e.
low-contrast tan on cream.

Fix: darken `border` to ~3:1, move `hover_bg` 3–4% off `app_bg`, and either collapse
`card_bg`/`white_surface` into one token or make them meaningfully different.

## 1.3 No spacing, radius, or type scale in practice

Counted across the gpui tree:

- **16 distinct spacing values** — 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 16, 18, 20, 24
- **10 distinct corner radii** — 0, 1, 5, 6, 7, 8, 9, 10, 11, 12
- **14 type sizes** — 10px through 28px in 1px steps

Radii are the worst: `rounded(px(7))`, `(8)`, `(9)`, `(10)`, `(11)`, `(12)` appear on
adjacent components and read as inconsistency rather than intent.

Meanwhile `apps/ui/design-tokens/design_tokens.json` declares a clean scale — spacing
5/9/14/18/28, radius 8/11/16/24, type 12/13/15/17/21/28 — and **no Rust code reads it**.
It has also drifted from the contract it nominally describes: the JSON says
`nav_item_height: 88` and cover `204×326`; the code uses 64 and `204×306`.

Fix: put 3–4 radii, 5–6 spacing steps, and 6 type sizes in `components/src/tokens.rs` and
replace the literals. Then either generate that file from the JSON or delete the JSON.

## 1.4 The serif/sans pairing never renders

`section_title` hardcodes `font_family("Source Serif 4")`
(`components/src/common.rs:37`). That font is **not bundled** — `add_fonts` installs only
Noto Sans, Libertinus Serif, and (Kobo) Terminus (`apps/desktop-gpui/src/lib.rs:66`), and
it is the only reference to that name in the repo. So the reader settings title
(`components/src/reader.rs:152`) and the footnote heading
(`reader/src/epub/render.rs:352`) fall through to an unpredictable fontconfig match.

Everywhere else "title font" is `interface_font(cx)`
(`browser/src/stores/preferences.rs:19`), which returns *the user's body font*. Titles are
therefore the same family as body text, only larger and semibold.

Net: the editorial serif-title/sans-body pairing is attempted in exactly two places, both
broken, and is a no-op elsewhere. Either bundle a display serif and have
`section_title`/`carousel_title` use it deliberately, or drop the pretense and remove the
`font_family` argument threaded through eight component signatures.

## 1.5 Flat action hierarchy

`browser_action_button_base` (`components/src/browser.rs:150`) gives *every* topbar and
switcher action `secondary().outline()` + `text_color(accent)` + `FontWeight::BOLD`. With
six bold accent-colored outline buttons in a topbar, none reads as primary and the accent
stops meaning "act here." `primary_action_button` exists but is used only on the first-run
screen (`browser/src/root.rs:357`).

Fix: demote routine topbar actions to muted weight; reserve accent + bold for one primary
action per surface.

## 1.6 Three unrelated elevation idioms

- `box_shadow(2, 8, 14)` on covers (`components/src/library.rs:15`) — note the 2px
  *horizontal* offset
- `box_shadow(0, 14, 34)` on the switcher dialog (`components/src/browser.rs:105`)
- the toolkit's `shadow_lg()` on the annotation editor (`components/src/reader.rs:105`)

Fix: define `raised` and `overlay` in `tokens.rs`; use nothing else.

## 1.7 No motion, and no loading state

Zero `with_animation` calls; only 9 `hover()` calls in the entire tree. Every state change
is an instant snap — route switches, switcher open/close, selection.

`loading` flags exist throughout the store and browse models but nothing renders for them:
no skeleton, no spinner, no `Indicator`. Book covers pop in as JPEGs resolve, so a grid
fills in raggedly (`browser/src/library/widgets/book_card.rs:89`).

Highest polish-per-effort item here: a cover skeleton at 204×306 plus a 120–150ms fade on
the switcher scrim.

## 1.8 Cover art gets cropped

`book_cover_image` uses `ObjectFit::Cover` into a fixed 2:3 frame
(`components/src/library.rs:23`). Real covers run 1:1.3 to 1:1.7, so anything off-ratio
loses title text off the top or bottom. `Contain` on a `surface_alt` mat preserves the art
and reads more like a shelf.

Related: the missing-cover fallback is an unstyled raw string — "No cover" / "Audiobook" —
at inherited body size (`browser/src/library/widgets/book_card.rs:114`). It needs muted
color, smaller type, and an icon.

## 1.9 Book card geometry doesn't add up

`BOOK_GRID_ROW_HEIGHT` is 438, so the card is 432 (`components/src/library.rs:11`), but the
children sum to ~410: 306 cover + 51 title + 22 author + 16 progress + 3×5 gap. That leaves
22px of unexplained slack.

`book_card_title` also carries `mt(9)` *inside* a `gap(5)` column (`:31`), so title spacing
is 14 while every other gap in the card is 5 — two spacing mechanisms in one component.

## 1.10 The nav rail's identity slot is weak

In a 96px rail, the position where an app mark belongs holds `library_switcher_trigger`
(`components/src/browser.rs:78`): a text-only ghost button, 10px bold, ellipsized, no icon.
Below it, items are 80×64 with `large()` icons over 11px semibold labels. There is no app
mark and no visual anchor — the first thing the eye lands on is a truncated library name.
The rail is 96 wide with 80-wide items and `px(8)` padding, so items sit flush to both
edges.

---

# Part 2 — Layout flow and information architecture

## The IA as built

```
nav rail (flat, 8 items, one column)
├─ Home        → carousels, no header
├─ "Library"   → Folders hierarchy   ┐ topbar: breadcrumb + filter + actions
├─ Subjects    → Subjects hierarchy  ┘
├─ Authors     → split: index | books, own filter, own heading
├─ Search      → input + result grid, no header
├─ Store       ┐
├─ Sync        ├─ replace the library page entirely; Escape is the only exit
└─ Settings    ┘
reader = separate OS window per book, no path back (desktop)
```

Two scopes exist in the code and are carefully modelled — library-scoped vs. universal
(`browser/src/route.rs:1`–`9`) — and the UI expresses neither. Everything is one
undifferentiated column of eight equal buttons.

## 2.1 No navigation history, and every route change destroys page state

`LibraryRoot::set_route` builds a new page entity and drops the old one
(`browser/src/library/mod.rs:106`–`115`). Only `folder_path`/`subject_path` survive,
because they were deliberately lifted up to `LibraryRoot`. Everything else is discarded:
the search query, the selected author, every scroll position, every title filter, the
store's catalog page. There is no back action at all — the keymap is eight direct jumps
plus Escape (`browser/src/lib.rs:38`–`53`).

This makes the core browsing loop impossible. *Authors → select an author → see a book →
check Subjects → come back* returns an empty Authors page with no selection and the index
scrolled to top. The nav rail is not navigation; it is eight mode switches.

**This is the highest-cost finding in the document** — more than everything in Part 1
combined.

The doc comment at `library/mod.rs:3` frames destruction as a feature: "changing routes
drops that page, its retained cards, and any in-flight page work as one operation." That is
clean for cancelling in-flight work, but it pays for that by losing the user's place.
Cancel the tasks; keep the state.

Fix: hold pages in a `HashMap<LibraryRoute, LibraryPage>` instead of one slot — card
entities are already retained, so the expensive part is solved — and add a `Vec<Route>`
history with `alt-←`/`alt-→` and mouse-back.

## 2.2 The topbar exists on two of eight destinations

`browser_topbar` has exactly one call site: `browser/src/library/pages/hierarchy.rs:172`.
Folders and Subjects get breadcrumb + filter + actions; Home, Authors, Search, Store, Sync,
and Settings each invent a header or have none.

Chrome therefore appears and vanishes as you move, and the search field physically
relocates: topbar-center on Folders, inside the left panel on Authors
(`pages/authors/mod.rs:182`), floating at the top of the body on Search
(`pages/search.rs:92`). There is no stable place for "where am I" or "act on this view."

Fix: a persistent topbar owned by `BrowserRoot` with title/breadcrumb, filter, and action
slots filled per route. This also gives Home and Authors the headers they currently lack.

## 2.3 Three identical-looking inputs with three different scopes

1. Full-text SQLite search over titles and authors, 150ms debounce (`pages/search.rs`)
2. Client-side filter of the *current hierarchy level only* (`pages/hierarchy.rs:174`)
3. Client-side author-name filter (`pages/authors/mod.rs:182`)

Same box, no labels, wildly different semantics — two only filter what is already in
memory. Placeholders are inconsistent too: the hierarchy filter says "Filter by title"
(`hierarchy.rs:52`); the other two have none.

Separately, search results are `title_matches` then `.extend(author_matches)` with no
dedup, grouping, or section labels (`pages/search.rs:81`–`83`), so a book matching both
appears **twice** in the grid.

## 2.4 Library switching resets everything; browse position is global, not per-library

`activate_library` and `replace_catalogue` both call
`install_library_root(library, LibraryRoute::Home, Vec::new(), Vec::new())`
(`browser/src/library/host/mod.rs:123`, `:155`), and `BrowserSession` holds a single
`folder_path`/`subject_path` (`browser/src/model.rs:23`).

So switching library forces you to Home, discards where you were, and also forgets where
you were in the library you are returning to. For a multi-library app, the primary flow is
the one being punished.

Fix: `HashMap<LibraryId, (LibraryRoute, folder_path, subject_path)>` in the session.

Separately, a full-screen scrim modal (`browser/src/navigation/switcher.rs:120`) is heavy
for a top-level context switch, and it is launched from a truncated 10px text button.

## 2.5 Subjects has no keybinding, and the keymap contradicts the rail order

`ctrl-1..7` maps Home, Folders, Authors, Search, Store, Sync, Settings
(`browser/src/lib.rs:41`–`47`). The rail reads Home, Library, Subjects, Authors, Search,
Store, … Sync, Settings (`browser/src/navigation/mod.rs:81`–`92`).

`ctrl-3` therefore lands on the rail's *fourth* item, and Subjects — a full top-level
hierarchy — is unreachable from the keyboard.

## 2.6 Rail labels don't describe their destinations, and scope changes are invisible

"Library" goes to `Folders`. "Home" is carousels. Folders and Subjects are two parallel
hierarchies with identical presentation and no indication of how they relate. Store, Sync,
and Settings are library-independent but sit in the same flat column — and entering one
*replaces* the library page with the rail selection as the only cue: no title, no close
button, Escape only (`browser/src/root.rs:331`–`340`). Escape is overloaded: it closes the
switcher, otherwise it leaves the universal page.

Fix: group the rail into library-scoped and app-scoped sections with a divider; rename
"Library" → "Folders", or merge Folders/Subjects into one browse page with a hierarchy
selector; give the universal pages a header with a close affordance.

## 2.7 Hierarchy pages present two unrelated blocks with no headings

`pages/hierarchy.rs:121`–`127` renders child folders as a flex-wrapped row of buttons, then
the book grid, with nothing labelling either. A directory with 3 subfolders and 200 books
is three pills followed by an unheaded wall of covers. No sort control, no result count,
no view toggle.

`LibraryView::{Covers, Detailed}` exists in the contract
(`client/app-contract/src/preferences.rs:166`) and is implemented in the mobile Svelte app,
but **no gpui code reads it**. Desktop is covers-only.

## 2.8 Reader chrome never gets out of the way, and shows no reading progress

The 58px toolbar is permanent (`components/src/reader.rs:20`) — no immersive mode, no
auto-hide on page turn, no fullscreen.

`ToolbarModel` (`reader/src/epub/toolbar.rs:53`) carries title, history flags, audio state,
and column count, but no position, percentage, chapter progress, or time remaining.
Progress *is* tracked and *is* shown on library cards
(`browser/src/library/widgets/book_card.rs:123`); the one place it is needed is the one
place it is absent.

## 2.9 Desktop reader windows have no way back to the library

`ReturnToLibrary` is wired only under `cfg!(feature = "kobo")`
(`reader/src/epub/toolbar.rs:176`–`178`). Window-per-book is a legitimate model, but it
then needs a window list, an "activate library window" binding, or at minimum the button.

## 2.10 All reader windows open at identical coordinates

`ReaderSettings.geometry` is a single global (`reader/src/windows.rs:160`–`164`), written by
one saver (`apps/desktop-gpui/src/lib.rs:169`). Opening a second book lands it exactly on
top of the first, perfectly occluding it — so reading two books side by side, the thing
multi-window buys you, is broken by default.

Fix: cascade by 24–32px per open window, or key geometry per book.

## 2.11 Toolbar controls misdescribe themselves

`←`/`→` appear as bare text labels among icon+label buttons
(`reader/src/epub/toolbar.rs:179`–`184`). They mean *link* history — footnote round-trips —
which reads as page back/forward.

The audio button is hidden with `.opacity(0.0)` while remaining in layout (`:192`), leaving
a permanent gap in the toolbar instead of being omitted.

## 2.12 Any library event re-queries the whole active page

`handle_library_event` routes `BookAdded`, `BookRemoved`, `DirectoryAdded`,
`DirectoryRemoved`, `ScanComplete`, and subject-enrichment batches to `refresh_active`
(`browser/src/library/mod.rs:159`–`168`), which reloads every row for the page. During an
initial scan that is one full page query per book added, and with no loading state (1.7) the
page visibly churns.

Fix: coalesce into a trailing-edge debounce of ~250ms.

---

# Suggested sequencing

**First — restore the concept of place.** Findings 2.1, 2.2, and 2.4 are one underlying
problem. Fix together: retained pages, a history stack, per-library session state, and a
root-owned persistent topbar. This is the difference between eight modes and an app that
can be browsed.

**Second — make the chrome honest.** 2.3 and 2.6: say what each input searches, group the
rail by scope, give universal pages a way out. Cheap, and removes most of the confusion
left after the first pass.

**Third — the visual foundation.** 1.1 (dark palette + appearance follow), 1.2 (border and
hover contrast), 1.3 (one scale in `tokens.rs`), 1.4 (bundle or drop the display serif).
Three files, most of the visible improvement.

**Fourth — reader flow.** 2.8 through 2.11: progress indicator, immersive mode, a way back
to the library, cascade window placement, fix the history labels.

**Fifth — polish.** 1.5 through 1.9 plus 2.12: one primary action per surface, one
elevation pair, skeletons and a short fade, `Contain` covers, debounce the refresh storm.
