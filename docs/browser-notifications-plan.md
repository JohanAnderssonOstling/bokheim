# Browser notifications

Replace the browser root's persistent error banner with transient toasts, and
leave page-level error banners alone.

## Why

`BrowserRoot` holds a single `error: Option<SharedString>` rendered as a bare
`Alert::error` (`root.rs:421`). It has three defects:

1. **It cannot be dismissed.** `Alert::on_close` exists in `gpui-component`
   and is never wired, so the alert has no close affordance.
2. **It survives navigation.** `self.error` is cleared only in
   `show_selected_library` (`root.rs:275`), which runs when the *selected
   library* changes. Plain `navigate()` does not clear it, so an error raised
   on the Library page follows the user to Settings, Store and Sync.
3. **Warnings render as errors.** `apply_catalogue` routes its `warning`
   argument through `show_error` (`root.rs:264`), so a warning appears in the
   error variant. `Alert::warning` exists and is unused.

All three are individually fixable, but the surface is wrong regardless.
Everything reaching root `show_error` is a transient outcome of a discrete
action that has already finished:

| Call site | Message |
|---|---|
| `root.rs:135` | Library setup failed |
| `root.rs:232` | Library removal failed |
| `root.rs:243` | Library activation failed |
| `root.rs:264` | Catalogue reconcile warning |
| `root.rs:267` | Catalogue reconcile failed |

None of those is a state. A banner that displaces page content until dismissed
is the wrong shape for all of them.

## Scope

**In:** the five root-level messages above.

**Out, deliberately:**

- **Page-level errors** (`browser_error_banner`, `common.rs:16`, used by Home,
  Search, Authors and the hierarchy pages) stay exactly as they are. These are
  genuine states — the page has no content — they pair the alert with a Retry
  action, they keep the body rendered beneath, and each page nulls its error on
  the next successful load. That is the correct surface for a load failure.
- **Form validation** (`universal/sync/authentication.rs`, `account.rs`) stays
  inline. Field errors belong next to the field; "Email is required" as a toast
  would be strictly worse.
- **The reader window** keeps swallowing failures for now. A failed annotation
  save or delete currently reaches only `log::error!`, which is a
  real gap, but the reader is a separate window with its own root and needs its
  own layer wiring. It inherits this pattern as a follow-up.

## The surface already exists

`gpui-component`'s `Root` constructs a `NotificationList` unconditionally
(`root.rs:246` in the component crate) and exposes
`Root::render_notification_layer`, a sibling of the `render_dialog_layer` that
`BrowserRoot` already calls in both of its render paths (`root.rs:411`, `:424`).

Pushing is `window.push_notification(note, cx)`. `Notification` offers
`error` / `warning` / `info` / `success` constructors, `.title()`, `.icon()`,
`.id1::<T>(key)` for deduplication, and `.autohide(bool)` — defaulting to true
with a five-second timer.

So this is wiring, not a new subsystem.

## Steps

1. **Render the layer.** Add `Root::render_notification_layer(window, cx)`
   alongside the existing `Root::render_dialog_layer(window, cx)` in both
   `BrowserRoot::render` paths — the first-run branch and the main branch.

2. **Convert the five call sites** to `window.push_notification`, with severity
   matched to the message: `Notification::warning` for `root.rs:264`,
   `Notification::error` for the other four.

3. **Errors do not auto-hide; warnings do.** Apply `.autohide(false)` to the
   error notifications and leave the default on the warning. These errors are
   failures of actions the user explicitly requested, and there are few of
   them — an error nobody saw is a bug report never filed. Volume is low enough
   that persistent error toasts will not accumulate, especially with step 4.

4. **Deduplicate with `.id1()`**, keyed per message source. `apply_catalogue`
   can fire from scanner events, sync reconciliation and explicit setup in a
   burst; without a key those produce a stack of identical toasts. Cheap now,
   irritating to retrofit.

5. **Delete the old mechanism**: the `error` field (`root.rs:32`), `show_error`
   (`root.rs:280`), the render branch (`root.rs:419-421`), and the
   `self.error = None` line in `show_selected_library` (`root.rs:275`).

## Implementation note: window availability

`push_notification` requires a `Window`. Four of the five call sites already
have one — `remove_library`, `activate_library` and `apply_catalogue` all take
`window: &mut Window`.

The exception is `handle_setup` (`root.rs:130`), a subscription callback with
only `cx`. Use the `cx.spawn` + `update_in` pattern the file already applies in
`navigate_later`, `activate_library_later` and `apply_catalogue_later`, rather
than introducing a different mechanism.

## Result

The change deletes more than it adds: one field, one method, one render branch
and one clearing line removed; five converted call sites and two layer lines
added. `Alert::warning` is never needed, because warnings become warning-severity
toasts.

## Verification

- `cargo check`.
- Manual: trigger a library removal failure and confirm the toast appears,
  can be dismissed, and does not reappear on navigation. Confirm a catalogue
  warning renders in the warning variant and auto-hides after five seconds.
  Confirm repeated catalogue events collapse to a single toast.
- Confirm page-level errors on Home, Search, Authors and the hierarchy pages
  still render inline with Retry, and still clear on the next successful load.
