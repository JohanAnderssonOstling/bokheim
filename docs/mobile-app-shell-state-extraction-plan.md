# Mobile App Shell State Extraction

## Outcome

`apps/mobile/src/App.svelte` is 319 lines with 3 `$state` declarations, down from
2259 lines and 69 rune declarations. What remains is application composition:
navigation helpers, the four session objects it wires together, the lifecycle
registration, and the markup.

The original plan aimed at extracting page state. That turned out to be the
smaller half of the problem — most of what it listed was already dead — and the
work that mattered was removing places where the shell kept a second copy of
something the URL, a route, or the backend already owned.

## The recurring defect

Every extraction so far had left the shell holding a duplicate of state that
belonged elsewhere, with no way to keep the copies in step. Each one was a live
bug, not just untidiness:

- **Browsing preferences** were loaded independently by the shell, settings,
  store and each library route. Saving in settings wrote to the database and
  notified nobody, so changing the theme did not apply until restart.
- **Download progress** was tracked by the shell, which alone registers the
  Tauri listeners, while the routes rendered their own copies. Live progress
  never reached the pages showing it.
- **Folder paths** in the persisted session came from a controller that had
  stopped running on navigation, so the session recorded stale locations.
- **The sidebar destination** was a hand-maintained mirror of the URL.
- **Post-import server sync** existed only in the shell's dead copy of the
  import flow; the folder route that replaced it never synced.

The fix in each case is the same: one owner, and a signal for anyone who needs
to react.

## Where things live now

| Concern | Owner |
| --- | --- |
| Shell → route invalidation, download progress | `lib/app/library-events.ts` |
| Both preference documents | `lib/app/preferences.svelte.ts` |
| Which library, and where in it | `lib/app/library-session.svelte.ts` |
| Signed-in verdict | `lib/app/account-status.svelte.ts` |
| Opening books, audiobooks, downloads | `lib/app/reader-session.svelte.ts` |
| Startup, per platform | `lib/app/startup{,-native,-web}.ts` |
| Library create/open/import/remove | `lib/library/overlay-controller.svelte.ts` |
| Post-import server sync | `lib/library/import-sync.ts` |
| Search-field focus | `lib/app/search-focus.ts` |
| Android update prompt | `components/shell/ApkUpdateNotice.svelte` |

Deleted outright: `data-controller`, `search-controller`, `trash-controller`,
`folder-controller`, `route-state.svelte.ts`, `screen-transition.ts`,
`reader-controller.ts`.

## Routing

The URL is the state. `activeDestination` derives from `page.route.id`;
`activeLibraryId` derives from the route parameter, with a stored fallback only
for the routes that carry no library (account, settings, store). Selecting a
library or a destination is now just navigation.

`synchronizeRoute` is gone. It existed because two things rendered outside
their own routes, and both were fixed: `/` redirects from `src/routes/+page.ts`
(`/library` already resolved which library to open), and the reader routes now
render the reader.

## The reader

`/reader/epub/[bookId]` and `/reader/pdf/[bookId]` were empty shells that set a
title while `ApplicationSurface` rendered the reader above the router. They now
open their own book and render it, with their own loading and error states,
sharing `routes/reader/reader-route.svelte.ts`.

That inverted the open flow — navigate first, then open the file in the route —
which removed the need for `ScreenTransitionController` entirely, along with its
handoff cache and the reader route cache key. There is no second opener to
coordinate with. Leaving the reader is `history.back()`, so it agrees with the
hardware back button instead of shadowing it.

Cost: choosing between the pdf and epub route needs one `getBookFileName` call
before navigating, where it used to fall out of opening the file.

## Verification

- `npx svelte-check` — no errors in any file touched here; 14 warnings, matching
  the pre-change baseline. It does not flag unused `$derived` consts, so dead
  state needs its own check.
- `npm run build:native` and `npm run build:web` both pass. The native build's
  `native-backend-only` plugin still proves no `lib/web` code ships.
- All `test:*` scripts pass except `test:catalog`, which fails on
  `__BOKHEIM_NATIVE__` being undefined under plain `node --test`; that predates
  this work. `test:account-contract` and `test:browsing-contract` were repaired
  — both had moved a directory deeper without their paths being updated.
- `src/lib/app/app-utils.test.ts` covers the session/route round trip in both
  directions.

## Not done

**Nothing has run on a device.** Reader open and close, PDF range sessions on
Android, audiobook hand-off, back navigation, library switching, preference
changes and the Ctrl+F focus path all changed shape and are unexercised. The
reader rewrite is the highest risk by a wide margin.

`webAppReady` still lives in the shell rather than with the web startup
sequence that owns its meaning.
