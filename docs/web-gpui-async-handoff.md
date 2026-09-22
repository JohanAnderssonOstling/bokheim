# Web GPUI async-state handoff

## User-visible problem

On Chromium, opening **Settings** can freeze the UI and emit repeated:

```text
gpui::window: RefCell already borrowed
```

The browser console identifies the failing location as
`GPUI-Fork/crates/gpui/src/app/async_context.rs`, in
`AsyncApp::update_entity`. It occurs after responses from the web backend
worker are posted.

## Confirmed cause

The application lets feature entities start backend work with `cx.spawn` and
then call `WeakEntity::update(cx, ...)` directly when an awaited worker or
browser callback resolves. On web, that completion can run while GPUI's
single-threaded `AppCell` is already mutably borrowed by a DOM/render callback.
The nested `RefCell` borrow panics.

Native does not reproduce this reliably because its platform dispatcher
serializes these callbacks differently. This is a scheduling bug, not an
acceptable platform distinction.

## Why Settings exposes it

`BrowserShell` currently constructs `SyncPage`. `SyncPage` starts account and
transfer snapshot work, and Settings embeds the page's account/library views.
Those snapshots return through worker callbacks and update GPUI entities. The
Settings route makes that timing particularly visible, but analogous patterns
exist in library detail, cover, search, trash, and reader workflows.

## Changes already made

* Settings was changed to embed a shell-owned `SyncPage` rather than construct
  a new one during render.
* Initial sync/transfer refreshes start from a normal frame.
* On web, `ApplicationRoot` now owns a typed sync-event inbox, request
  generations, backend account/transfer/auth requests, and the `SyncState`
  snapshot. `SyncPage` sends commands and reads that state; worker and timer
  completions no longer update its GPUI entity directly. The inbox retries a
  UI update when the web AppCell is busy.
* Account/status collections remain ordinary vectors. Only transfer rows are
  shared with the activity display cache through an `Arc`.
* A Chromium regression test in
  `apps/web-gpui/scripts/settings-async.browser-test.mjs` delays database-worker
  replies until after Settings opens, then verifies that the sign-in form still
  opens without a borrow panic.
* A temporary `after_browser_callback` guard remains on native SyncPage paths.
* GPUI now has experimental `AsyncApp::wait_for_update_slot` and
  `AsyncApp::defer_update` helpers. These are transitional; do not treat them
  as the final architecture.
* Several other browser async paths were partially migrated to those helpers.
  This migration is incomplete and should not be expanded as the long-term fix.

## Required architecture

Use one-way state flow:

```text
backend / worker -> root event inbox -> root state reducer -> read-only stores -> views
```

1. `ApplicationRoot` (or an app-level coordinator it owns) owns backend
   requests, result delivery, request generations, and refresh scheduling.
2. Async work emits typed events only, for example `AccountSnapshotLoaded`,
   `TransferSnapshotLoaded`, and `LibraryListChanged`. It must not own or
   directly mutate view entities.
3. The coordinator applies events in one serialized UI turn and updates shared
   stores. If an event is older than the store's request generation, discard it.
4. `BrowserRoot`, `BrowserShell`, `SyncPage`, Settings, and reader views observe
   stores and render snapshots. Settings must not create, refresh, or receive
   backend callbacks.
5. The GPUI web dispatcher should ultimately serialize foreground runnable
   execution with app updates. That is the framework-level defense; the
   application event inbox is the app-level ownership boundary.

## Remaining implementation order

1. Move remaining refresh coalescing, loading/error state, and presentation
   timing out of the web `SyncPage`/activity view and into the coordinator or
   store as appropriate.
2. Migrate other UI-owned backend completions to typed coordinator events in
   batches (library updates, details/covers, browse/search, reader).
3. Only after the state flow is in place, remove the transitional
   `wait_for_update_slot`/`defer_update` calls and helpers if the web dispatcher
   has a robust global serialization guarantee.

## Verification

Use optimized builds only:

```bash
RUSTC_BOOTSTRAP=1 cargo check --release -p web-gpui --target wasm32-unknown-unknown
CARGO_BUILD_JOBS=1 servers/sync/deploy/deploy-web-remote.sh
```

After deployment, hard-reload the browser: the currently running WASM instance
cannot replace itself.
