# Kobo Application Invalidation Plan

## Scope

This plan covers changes to the shared Bokheim GPUI application and UI libraries. The goal is to reduce unnecessary view reconstruction, layout, and rendering while keeping the same UI code on desktop and Kobo.

Changes to GPUI core, `gpui_kobo`, framebuffer damage tracking, and FBInk are outside this plan. Those layers should consume the more precise application invalidations produced here.

## Current Symptoms

Device logs show that many input events invoke a render taking 200-980 ms and then produce `changed=false`. Changed reader frames can also approach one second. The application should avoid notifications for no-op state transitions and prevent local changes from invalidating root browser or reader views.

## Design Rules

1. State belongs to the smallest component that renders it.
2. A mutation must compare old and new state before calling `cx.notify()`.
3. Parent views must not subscribe to child-only visual state.
4. Navigation and data changes may invalidate a screen; hover, pressed, selection, progress, and toolbar state must remain local.
5. Component identities and child order must remain stable between renders.
6. Fixed-size visual changes must not trigger ancestor layout changes.
7. Background work must publish one consolidated state update instead of several intermediate updates.

## Phase 1: Notification Audit and No-op Guards

Audit `apps/browser` and `apps/reader` for every `cx.notify()`, entity update, subscription, observer, timer, and asynchronous callback.

Classify each notification as one of:

- Root navigation or screen replacement.
- Data/layout change.
- Local paint change.
- No-op or duplicate notification.

For every mutation, compare the resulting state before notifying. Repeated selection, page navigation at a boundary, unchanged query results, repeated focus, and duplicate progress values must return without notification.

Add temporary debug-only counters for notifications by component and reason. These counters must not log every render in release builds.

Deliverables:

- No-op guards around browser navigation, library selection, filters, reader page navigation, reader progress, and toolbar visibility.
- Named notification reasons in debug instrumentation.
- Removal of duplicate notifications emitted by both a command handler and its asynchronous completion.

## Phase 2: Browser State Isolation

Split browser state by responsibility instead of storing all interactive state in the root browser view.

Recommended ownership:

- Browser shell: current route and active library only.
- Navigation/sidebar entity: expanded state, selection, and local focus.
- Header/search entity: query text, focus, and clear-button state.
- Library results entity: query result generation and visible item collection.
- Book item presentation: stable book identity and immutable display data.

The browser shell should be notified only when route selection changes the main content or when the active library changes. Sidebar pressed/focus state and search editing must notify their own entities.

Keep book item IDs stable using the domain book identifier. Do not derive identity from list position. Reordering or filtering should retain entities or presentation records for books that remain visible.

Deliverables:

- Local entities for navigation and search state.
- A results entity that owns library query results independently of browser chrome.
- Stable IDs for book rows/cards.
- No root notification for repeated selection or child-only visual state.

## Phase 3: Library List Virtualization

Only construct, lay out, and paint visible library items plus a small overscan range.

Use GPUI's virtual/uniform list facilities where item height is fixed. If the current layout uses variable-size cards, normalize card dimensions on Kobo-sized windows or use a visible-range adapter.

Requirements:

- Scrolling changes the visible range without rebuilding browser chrome.
- Off-screen thumbnail decoding must not notify the root view.
- Thumbnail completion updates only the owning visible item or results entity.
- Results generations discard stale asynchronous thumbnail and query responses.
- Overscan must be conservative on Kobo to avoid preparing multiple screens of unused content.

Deliverables:

- Visible-range list/grid rendering.
- Generation tokens for asynchronous results.
- Bounded thumbnail work and cache size.

## Phase 4: Reader Separation

Separate reader chrome from document content.

Recommended ownership:

- Reader shell: publication identity and open/closed state.
- Document viewport entity: document layout, current location, and visible page.
- Reader toolbar entity: visibility and button state.
- Progress entity: displayed progress text/value.
- Dialog/overlay entities: local modal state.

A page turn should update the document viewport and progress entity. It must not reconstruct unrelated toolbar, window navigation, or publication metadata. Showing or hiding controls must not rebuild document layout.

Page navigation must be guarded at boundaries. If the requested next/previous location equals the current location, do not notify or save progress.

Persist reading position after a successful location change. Debounce persistence so rapid page turns do not produce a database operation and follow-up notification for every input.

Deliverables:

- Independent document and chrome entities.
- Boundary guards for page navigation.
- Local progress updates.
- Debounced persistence without UI feedback notifications.

## Phase 5: Stable Layout and Paint-only Components

Adjust shared components so common state changes preserve their layout bounds.

Apply this to buttons, navigation rows, book cards, toolbar controls, progress labels, and selection indicators:

- Reserve fixed space for icons, borders, progress text, and selected markers.
- Avoid conditionally inserting/removing layout children for pressed or selected state.
- Change color, visibility, or opacity inside reserved bounds instead.
- Keep element IDs stable across normal state transitions.
- Avoid rebuilding text runs when only background or border state changes.

Desktop behavior and appearance must remain equivalent. These are general component improvements, not Kobo-specific branches.

## Phase 6: Consolidate Asynchronous Updates

Library loading, thumbnail completion, document opening, progress restoration, and metadata loading can currently produce closely spaced frames.

Each workflow should publish a single coherent UI state where possible:

- Gather related results before updating the owning entity.
- Discard stale results using generation IDs.
- Do not notify for internal cache writes.
- Batch multiple completed thumbnails into one results update per event-loop turn.
- Avoid an immediate loading-state notification when the operation completes within the same turn.

The application must remain responsive on desktop; batching windows should be event-loop based rather than long time delays.

## Implementation Order

1. Add no-op guards and notification-reason instrumentation.
2. Separate browser navigation/search/results ownership.
3. Virtualize library results and isolate thumbnail updates.
4. Separate reader viewport, chrome, and progress ownership.
5. Stabilize shared component layout and identity.
6. Consolidate asynchronous updates.
7. Remove or disable temporary verbose instrumentation after measurements are captured.

## Measurement Plan

Collect counts for the following scenarios on desktop and Kobo:

- Open Library.
- Select the already active library.
- Open one book.
- Turn ten pages with hardware buttons.
- Attempt previous/next navigation at a boundary.
- Show and hide reader controls.
- Return to Library.
- Exit.

Record:

- Notifications by component and reason.
- Frames requested.
- Frames that produce changed output.
- Browser and reader render duration.
- Database/progress writes.

## Acceptance Criteria

- Repeating an already-selected action requests no frame.
- Page navigation at a boundary requests no frame.
- A button or selection-state change notifies only its owning component.
- Reader toolbar changes do not rebuild document layout.
- Page turns do not rebuild browser or unrelated reader chrome.
- Off-screen books are not constructed or painted.
- Thumbnail completion does not invalidate the browser root.
- One user action normally produces at most one changed frame.
- The shared desktop UI retains existing behavior without Kobo-specific component forks.

## Relationship to Backend Work

Application isolation reduces the amount of GPUI work and prevents avoidable frame requests. It does not, by itself, guarantee a narrow framebuffer update. `gpui_kobo` should separately skip unrequested frames and derive damage regions from changed scene or framebuffer content.

If application isolation and backend frame gating still leave scene construction as a dominant cost, then a later GPUI change can add explicit paint-only/layout invalidation and cached subtree scene bounds. That decision should be based on measurements from this plan rather than made upfront.
