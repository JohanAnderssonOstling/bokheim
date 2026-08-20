# Reader session ownership

Give the EPUB and PDF readers one session object with one owner, so that
position, annotations, bookmarks and panel state stop being modelled three
different ways at once. Prop drilling out of `Reader.svelte` is the symptom this
starts from; the format fork in `ReaderBook` is the cause it ends at.

## Why

Reader state currently reaches components through three parallel mechanisms, and
which one applies depends on facts no reader declares.

**Props.** `Reader.svelte` builds eleven controllers (`selection`,
`annotationStore`, `annotationEditor`, `chrome`, `nav`, `bookmarkList`,
`footnotes`, `search`, `position`, `prefs`, marks) and hands each child a
flattened projection of one of them. `SearchBar` takes seven props that are all
fields of `search`; `SelectionContextMenu` takes four that are all `selection`.

**Context.** `setContext("epub-reader", …)` (`Reader.svelte:171`, string key)
serves `EpubReaderSidebar` and `SettingsPanel`.

**A module-level global.** `lib/reader/marks.ts` holds `let activeRuntime` and
exports `readerAnnotations()`, `readerBookmarks()`, `openReaderBookmark()` and
the rest as free functions.

The third one is load-bearing, and it should not be.
`provideReaderAnnotationActions` and `provideReaderBookmarkActions`
(`components/shared/readers/annotations/*-actions.ts`) are never called
anywhere in the repository. `useReaderBookmarkActions()` at
`EpubReaderSidebar.svelte:15` therefore returns `undefined` and `bookmarkItems`
is always `[]`. The sidebar renders only because the shared panels fall back to
the global.

That fallback is not dead legacy. `installReaderMarks` is called from exactly
one place — `Reader.svelte:180` — so the same component serves two different
data sources depending on which reader mounted it:

- EPUB mounts `BookmarkPanel` → `readerBookmarks()` hits → props ignored.
- PDF mounts `BookmarkPanel` → global is empty → falls through to
  `passedBookmarks`.

`BookmarkPanel.svelte:44` branches its click handler on exactly this:
`readerBookmarks() ? openReaderBookmark(bookmark.id) : onOpen?.(bookmark)`.
Selecting between two data paths with a module-level `let` means one reader per
process, no ownership, and cleanup by identity guard.

Underneath all of it, `ReaderBook` (`lib/shared/types.ts:123`) carries both
formats' locator vocabularies in one interface:

```ts
initialCfi?: string | null;                 saveCfi: (cfi, progress) => …
initialPdfPage?: number | null;             savePdfPage?: (pageIndex, pos, progress) => …
initialPdfFullPagePosition?: number | null;
```

Both `Reader.svelte` (548 lines) and `PdfReader.svelte` (1651 lines) consume it
and each ignores half the fields. The format difference is genuinely narrow —
how a position is named, and what draws pixels — but it is modelled at the top
as a union prop and then re-forked at every leaf.

## Target

The two readers stay separate. The fat union splits rather than unifying.

**`ReaderBook` becomes `ReaderEpubBook` and `ReaderPdfBook`.** The four locator
fields stop being optional-and-ignored: the EPUB type carries `initialCfi` and
`saveCfi`, the PDF type carries `initialPdfPage`,
`initialPdfFullPagePosition` and `savePdfPage`, and neither declares the
other's. `openForRoute` returns the one matching the kind the caller already
decided via `getBookFileName`.

**A `ReaderRepository` per reader.** Five controllers currently repeat the same
`canUpsert`/`canDelete`/`canLoad: () => Boolean(book.x)` triad, probing an
optional-function bag to discover what the backend supports. A repository
interface replaces it, with an explicit capability set where a capability really
is optional. EPUB and PDF get their own; the backend commands underneath are
already distinct calls.

**Foliate confined to a module, not a port.** Every `<foliate-view>` call and
all Overlayer logic moves out of the component into one EPUB-only module. This
is encapsulation, not polymorphism — there is no second implementation and no
interface for the PDF side to satisfy.

**A session graph constructible without a component.**
`createEpubReaderSession({ repository, bookId })` returns the controllers.
Today all eleven are born inside `onMount`, so the coordination logic that is
actually the product — `requestClose`'s precedence of editor over panel over
reader — cannot be unit tested at all. The PDF reader gets the same treatment
separately, sharing the pattern but not the code.

**A session graph constructible without a component.**
`createReaderSession({ engine, repository, bookId })` returns the controllers.
Today all eleven are born inside `onMount`, so the coordination logic that is
actually the product — `requestClose`'s precedence of editor over panel over
reader — cannot be unit tested at all.

**One provider, narrow consumers.** A symbol-keyed module exposing
`provideReaderSession(session)` plus `useReaderSearch()`, `useReaderSelection()`,
`useReaderChrome()` and friends. Children consume the controller, not a
flattened copy of it. No module-level globals.

**Presentational panels.** `AnnotationsPanel` and `BookmarkPanel` take `items`
plus callbacks and hold nothing.

## Steps

Each step compiles and ships on its own.

1. **Provide the action contexts, delete the global.** Call
   `provideReaderAnnotationActions` and `provideReaderBookmarkActions` from
   *both* `Reader.svelte` and `PdfReader.svelte`, then remove
   `lib/reader/marks.ts`'s `activeRuntime` along with the `?? passedBookmarks`
   and `?? annotations` fallbacks and the branch at `BookmarkPanel.svelte:44`.
   This must land as one change: removing the global alone would silently switch
   EPUB onto a props path nobody fills.

2. **Move optimistic updates into the store.** `BookmarkPanel` keeps `created`
   and `deleted` local state shadowing the real list, and `AnnotationsPanel`
   keeps a `deleted` set. Both are second sources of truth for data the session
   owns. Fold them into the annotation and bookmark stores, leaving the panels
   pure.

3. **Split `ReaderBook`** into `ReaderEpubBook` and `ReaderPdfBook`, each
   declaring only its own position fields. `openForRoute` returns the one
   matching the kind the caller already resolved. Nothing in either reader has
   to ignore half an interface afterwards.

4. **Confine foliate to one module.** Move `reader-foliate-view.ts`, the CFI
   calls and the Overlayer drawing out of `Reader.svelte` into a single
   EPUB-only module. No interface, no second implementation.

5. **Lift the EPUB session graph out of `onMount`** into
   `createEpubReaderSession(deps)`, and add the `requestClose` precedence tests
   that this first makes possible.

6. **Introduce `ReaderRepository`**, deleting the `Boolean(book.x)` capability
   probes across the five controllers.

7. **Migrate children to `useReader*()` hooks** one at a time, deleting props as
   each moves, then remove the `"epub-reader"` string context and
   `reader-context.ts`.

8. **Apply steps 4–7 to `PdfReader.svelte` separately**, if and when it earns
   the work. It shares the pattern, not the code.

Steps 1 and 2 are worth doing regardless of whether the rest proceeds.

## Decisions

**Context for route-owned components, props for shared ones.** `SearchBar`,
`SettingsPanel`, `EpubReaderSidebar`, `FootnoteSheet`, `SelectionContextMenu`
and `ImageViewer` read context. Everything under `components/shared/readers/`
stays prop-driven — `AnnotationEditor`, `SidebarShell`, `PanelShell`, `TopBar`
are shared with the PDF reader and should stay dumb. This preserves the
existing convention that pages compose and handle events while components
create and style.

**Symbol keys, not string keys.** `"epub-reader"` is replaceable by any
component in the tree.

**No rewrite.** The controller decomposition is good and ports nearly as-is;
`reader-search`, `reader-position`, `reader-chrome` and `reader-selection` are
the right grain, and the seams the target needs are already cut. The only thing
a rewrite buys is never having shipped the marks global, and step 1 deletes
that.

## Risks

**Step 1 changes live behaviour in both readers at once** and is the highest
risk in the list despite being the smallest. The EPUB sidebar's bookmark list is
currently fed by a path that the code appears not to intend; anything relying on
that accident changes shape.

**Step 3 touches persisted data.** `Locator.raw` must round-trip existing stored
CFIs and PDF page positions unchanged, and existing rows carry no `engine`
discriminator — it has to be inferred from the book's file type on read.

**Step 8 is a 1651-line component** with device-specific behaviour (native PDF
range sessions on Android) that no test covers.

## Verification

- `npx svelte-check` against the pre-change warning baseline. It does not flag
  unused `$derived` consts, so dead state needs its own check.
- `npm run build:native` and `npm run build:web`.
- The `test:*` scripts, noting that `test:catalog` already fails on
  `__BOKHEIM_NATIVE__` under plain `node --test`.
- New unit tests over `createReaderSession` with a fake engine and in-memory
  repository, covering `requestClose` precedence and locator round-tripping.
- On device: reader open and close, bookmark add/open/delete in **both** readers,
  annotation create and reopen, sidebar in compact and wide layouts, and PDF
  range sessions on Android.

## Debris to sweep

Independent of the above, in files this work touches:

- `reader-annotation-store.svelte.ts:157` — a stray
  `import { getContext, setContext } from "svelte"` after all the code, unused.
- `Reader.svelte` imports `READER_PREFERENCES_CONTRACT` and
  `readerPreferenceConstraints`; neither is used.
- `nav` (`Reader.svelte:122`) closes over `position`, declared at line 154. It
  works because the reference is lazy. Step 5 makes the ordering explicit.
