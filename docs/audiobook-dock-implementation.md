# Persistent audiobook playback implementation

Accepted behavior: opening an audiobook starts it in a bottom dock without leaving the browsing page. The dock expands into the full player and minimizes without reopening audio. Playback survives library switching and saves to its originating library. One active session retains its source and leases. Explicit close stops playback. Restart restores the dock paused, without opening the stream until Play. Network loss drains buffered audio, then buffers with bounded/backoff retries; position does not advance while buffering and user pause/close cancels automatic resume. Headphone disconnection pauses silently. Removing the originating library ends playback and saves progress. Kobo updates must avoid ticking seconds/animations in the dock.

Implementation checkpoints:

- Extract application-owned playback session and retain original library/source/persistence resources.
- Connect dock and expanded view to one session; preserve browsing context and replace books safely.
- Persist and restore active book identifiers paused; handle removed libraries and unavailable sources.
- Implement streaming recovery, source-revision validation, retry cancellation, buffering state.
- Verify lifecycle, resource ownership, persistence, network and headphone behavior with optimized release checks/tests and Kobo validation.

Completion requires all checkpoints; UI ownership work alone does not prove streaming recovery or restart restoration.

## Current starting point

The shell retains an `AudiobookSurface` containing a shared playback session and
dock. `AudiobookReader` now observes that session and owns only full-page UI
state. The session's persistence worker captures the originating `LibraryClient`
and book hash; navigation does not change that destination.

The remote `RangeReader` currently propagates fetch failures from `Read`; it does
not itself implement recovery. Keeping the player alive across library changes
therefore does not establish reliable offline/reconnect behavior.

## Implementation sequence

### 1. Extract an application-owned playback session

- Introduce one playback controller shared by application surfaces, independent
  of the selected browsing library and the lifetime of a reader page. Do not
  create separate controllers per window if additional windows are supported.
- Move engine, publication metadata, position, speed, sleep timer, source ownership,
  and progress persistence out of `AudiobookReader` into the session.
- Retain the originating library client, book locator, resolved source, and any
  cache/download leases for the lifetime of playback. Copy presentation metadata;
  retain resource handles instead of duplicating the audiobook file.
- Leave focus, menus, scrubbing gestures, and chapter-list scrolling in the views.
- Expose transport commands and observable snapshots. Represent playback status
  separately from user intent: loading, playing, paused, buffering, ended, failed;
  plus whether playback is requested. Buffering must not erase a user's pause.
- Tag asynchronous work with a session generation/cancellation token so late
  loads or retries cannot restart a closed or replaced book.

Acceptance: destroying/recreating either view or switching libraries does not
restart the decoder, lose the source, or redirect progress writes.

### 2. Complete dock and full-player navigation

- Opening an audiobook starts it in the dock, leaving the browsing page visible.
- Show cover, title, play/pause, forward 30 seconds, and close. Tapping the body
  expands the existing full player; Back minimizes to the preserved browsing page.
- Both views issue commands to the same session. Opening the active book reuses
  it; an explicit chapter/position target seeks rather than creating a decoder.
- Replacing a book saves/stops the old session and cancels its pending work.
  Closing the dock stops playback and clears the restorable active session.
- Show loading/buffering/failure states without modal interruptions. On Kobo,
  avoid second-by-second dock updates and animated loading indicators.

Acceptance: expand/minimize and library switching preserve audible continuity;
there is never more than one active playback engine.

### 3. Persist and restore the active session

- Store a versioned active-session record containing origin library ID, book ID,
  position, speed, and enough cached metadata to render the dock. Do not persist
  live handles, credentials, URLs as identity, or a playing flag for auto-start.
- Save periodically and on pause, seek completion, replacement, close, and normal
  shutdown. Order writes so an old session cannot overwrite the replacement.
- Restore a paused dock after restart, without opening the stream or audio output.
  Resolve the source and refresh credentials only when the user presses Play.
- Handle missing books, removed libraries, and expired authentication explicitly.
  Save progress before intentional library removal, then stop/release its session.
  Do not discard the last valid position merely because the server is offline.
- Cancel startup restoration if the user has already opened or closed a session.

Acceptance: restart causes no audio or media-stream request until Play; progress
continues to belong to the original library after browsing another library.

### 4. Recover streaming interruptions

- Preserve typed transport errors through the source adapter. Distinguish timeout,
  disconnection, and retryable server errors from authentication, missing source,
  source revision mismatch, decoding failure, and genuine end-of-file.
- Let already-decoded audio play out after connectivity is lost. Enter buffering
  only when the output queue is exhausted; hold the audible playback position.
- Retry missing ranges with cancellable exponential backoff and a capped delay.
  Keep blocking network reads off the UI/output threads. Avoid changing unrelated
  document-reader retry behavior when extending the shared range reader.
- Validate revision/checksum before combining old buffered data with new ranges.
  Never interpret a temporary range failure as successful end-of-book.
- Resume automatically after sufficient prebuffering only if playback is still
  requested. Pause, close, replacement, and seek invalidate inappropriate retries
  or queued results. If the retry budget is exhausted, retain position for Play.

Acceptance: simulated network loss drains the buffer, holds position, and resumes
without skipping; pausing during recovery prevents subsequent automatic playback.

### 5. Preserve audio-device and lifecycle behavior

- Headphone/output disappearance silently pauses and clears playback intent.
  Reconnection alone must not resume audio; Play recreates the output as needed.
- Keep device loss distinct from network buffering so a network retry cannot
  restart playback after headphones disconnect.
- Tie background playback/sleep policy to the session, not full-page visibility;
  preserve the existing platform-specific audio service behavior.
- Ensure close/replacement interrupts blocked source work and releases threads,
  output devices, stream handles, and cache leases without freezing the UI.

### 6. Validate before deployment

- Add state-transition tests for opening, replacing, closing, seeking, buffering,
  end-of-book, and combinations of network/device loss and user pause.
- Add ownership/persistence tests for library switching/removal, view recreation,
  ordered saves, restart restoration, and stale asynchronous completions.
- Use a controllable range source for timeout, truncated response, reconnect,
  authentication failure, and revision-change integration tests.
- Run optimized release checks/tests for affected backend, player, reader, and
  shell crates, including Kobo and other supported feature configurations.
- On Kobo verify dock layout, touch controls, full-player navigation, library
  switching, Wi-Fi loss/reconnect, Bluetooth disconnect, restart, and CPU/repaint
  behavior. Build/package in repository `target` directories, not `/tmp`.
- Treat SSH deployment and hardware validation as a subsequent explicit step;
  this document is a plan, not a claim of completed implementation or validation.

## Delivery boundaries

First deliver session extraction plus dock navigation; then paused restoration;
then network/device recovery; finally release regression tests and Kobo validation.
Each checkpoint should remain independently testable. The largest technical risk
is cancellation and error propagation through the synchronous decoder input, not
the dock layout. Audit that boundary during session extraction before committing
to the recovery implementation.

## Implementation checkpoint: session internals

- Transport, sleep timing, playback observation, and progress persistence now live
  in `apps/audiobook-player/src/session.rs`, separate from view-local fields.
- Source loading and timers now belong to a shared `Entity<PlaybackSession>`.
  The shell owns the session independently of either view; the dock observes it
  directly. Minimizing destroys the full view, and expanding reconstructs it
  around the same session. An application-global controller now coordinates the
  shared session across windows, with generation checks for stale open results.
- Explicit stop cancels retained resolution/load tasks and ignores late load
  completion. Interrupting an already-blocked decoder/network operation still
  requires the source cancellation work in step 4.
- Stop saves once and prevents Drop from overwriting the final position with an
  older UI sample. Reopening the active book reuses the session, including pending
  chapter targets while loading; pause during loading clears playback intent.
- The audiobook-player release suite passes (70 tests), including GPUI entity
  tests for full-view destruction/recreation and last-owner release. The shell's
  optimized Kobo-feature check passes. No deployment was performed.

## Implementation checkpoint: restoration wiring

- Added a versioned device-local active-session record with origin locator,
  session ID, metadata, confirmed position, and speed. Backend writes use
  compare-and-swap; three release tests cover round-trip restoration, rejected
  invalid writes, and stale save/close operations after replacement.
- The application controller owns an ordered save queue. It restores a paused
  session, saves changes periodically and on transport transitions, clears the
  record on explicit close, and flushes the queue on normal application quit.
- Restored sessions create no decoder, output device, or source-loading task
  until Play. A GPUI regression test verifies this with an absent library. Source
  resolution errors retain the position and allow another Play attempt.
- The controller observes the authoritative library list, rather than treating
  another window's temporarily stale browsing list as a deletion. Restoring a
  removed library clears the active record. Browser-initiated deletion now has
  pre-removal preparation, described in the lifecycle checkpoint below.
- Current release verification: 72 audiobook-player tests, 4 Kobo-feature shell
  tests, and 3 focused backend persistence tests pass. The shell ownership test
  uses an in-memory writer to stay within GPUI's deterministic scheduler; it is
  not evidence of end-to-end backend restart or shutdown behavior.
- Remaining validation includes end-to-end disk restart/quit behavior, runtime
  multi-window races (including ordering of asynchronous format probes before
  a reader is created), and other supported platform configurations. Streaming
  recovery, source cancellation, and physical Kobo validation remain unfinished.

## Implementation checkpoint: native streaming recovery

- Native desktop/Kobo audiobook ranges now preserve typed retryable versus
  terminal errors through authentication and transport. Existing document readers
  retain their single-attempt behavior. Every range remains pinned to its ETag.
- Temporary failures retry the same offset with capped exponential backoff and
  an eight-attempt budget. Pause gates retries; resume releases them. Close/drop
  cancels both retry waits and in-flight HTTP work, then releases the range thread.
- The decoder checks source failure/cancellation before declaring EOF. Exhausted
  retries stay failed across repeated decoder reads. Explicit Play can reopen the
  source at its confirmed position after a terminal transport failure.
- Output starvation exposes a static Buffering label and holds position. Remote
  playback refills six decoded blocks before leaving buffering, except at a short
  end-of-book tail or terminal failure. Kobo output loss gates retries immediately
  and is separately reported to the session as a silent pause.
- Regression tests exercise retry offsets, backoff caps, terminal errors, retry
  exhaustion, pause/resume, in-flight cancellation, buffering position, and short
  failure tails. End-to-end MP3/M4B network fault injection remains unverified.
- Current release verification passes 75 Kobo-feature player tests and 17 backend
  range-reader tests. These establish the adapter/state behavior, not real-device
  Bluetooth/network timing or full MP3/M4B decoder recovery under injected faults.
- Still unfinished: Android/web recovery integration, lifecycle edges listed
  above, and physical Kobo validation. This checkpoint does not
  establish completion of the full plan.

## Implementation checkpoint: seek interruption and request ordering

- Seek generations now invalidate both in-flight HTTP work and paused retry
  waits. The decoder acknowledges only the latest generation before seeking;
  the same source, cache, and pinned revision remain alive.
- Cancellation returns ConnectionAborted, and seek interruption returns
  WouldBlock. Neither is internally retried by Rust's `read_exact`, unlike
  Interrupted. The decoder handles these control states before EOF/error logic.
- Nineteen backend range-reader release tests pass, including cancellation
  through `read_exact`, paused seek interruption, stale acknowledgement rejection,
  and reading the new position through the same source.
- An explicitly run FFmpeg-assisted integration test generates MP3 in an M4B
  container, stalls decoder input, seeks, and compares recovered PCM exactly with
  a clean decoder seek. It passes in release mode without requiring an audio
  device. The normal Kobo-feature player suite passes 75 tests; that fixture test
  is marked ignored there because it requires FFmpeg and was run separately.
- Application-wide open tickets now preserve user request order across format
  lookups. Accepting an older result cannot override a newer audiobook choice;
  non-audio lookups do not cancel another window's audio request. Explicit close
  invalidates pending tickets. Rendering and full multi-window lifecycle behavior
  still require integration validation beyond these ordering tests.

## Implementation checkpoint: removal and shutdown barriers

- Browser-initiated library deletion now invokes an application-owned preparation
  hook. The controller cancels matching pending opens, stops matching playback,
  and waits for the original library's position writer and active-state writer
  before allowing deletion. Preparation errors leave the library undeleted;
  removing an unrelated library does not stop playback.
- The session retains its position-writing task. Explicit stop-and-flush closes
  the queue and awaits its final result; ordinary drop detaches the task so queued
  progress is not cancelled. Native stop captures the last confirmed position,
  including the rollback point of an unfinished seek.
- Normal quit waits for the progress writer as well as restoration-state writes.
  Android waits for the service's released event, which is signalled after the
  final progress event is enqueued. Missing acknowledgement times out and blocks
  deletion rather than silently discarding the final event.
- Release validation: 81 Kobo-feature player tests, 9 shell tests, and 3 browser
  preparation tests pass. Optimized application-shell checks also pass for
  WebAssembly and aarch64 Android (NDK 29.0.14206865, API 26).
- These are component/protocol tests and cross-compilation, not device tests.
  End-to-end disk restart/quit validation, the final Android restoration snapshot
  after service release, platform streaming recovery, cover restoration, and
  physical Kobo validation remain to be completed/audited.

## Implementation checkpoint: browser progress units

- Browser callbacks now convert fractional progress into the library's 0–100
  percentage convention, matching native and Android persistence. Invalid or
  unrepresentable timestamps and non-finite progress values are rejected before
  enqueueing saves; finite fractional overshoot is clamped.
- Three new release tests cover conversion, boundaries, and invalid media values.
  The default-feature player release suite passes 83 tests with one fixture test
  ignored; the WebAssembly player optimized check also passes.
- This verifies callback conversion, not browser runtime streaming recovery or
  confirmed-seek restoration. The remaining lifecycle and device gates above
  are still outstanding.

## Implementation checkpoint: Android final restoration position

- Android retains a lightweight position snapshot after the engine is dropped;
  it does not retain the source, file, or progress sender. Service release can
  therefore update the final restoration position without extending source life.
- Normal quit now awaits the position/service flush before snapshotting and
  flushing the active-session record. Initial unready events no longer reset a
  restored position to zero. Unacknowledged seek targets are excluded from live
  restoration records; stopping during a pending seek keeps the last accepted
  position rather than the requested target.
- Three new protocol tests cover final position after source release, startup
  readiness, and release during an unfinished seek. The default-feature player
  release suite passes 86 tests (one fixture test ignored), and the Kobo-feature
  shell suite passes 9. The Android optimized shell cross-check passes.
- These tests do not yet establish an end-to-end Android process quit/restart.
  Disk restart validation, cover restoration, browser/Android streaming recovery,
  and physical Kobo acceptance remain outstanding.

## Implementation checkpoint: dock cover restoration

- Source opening now requests the browse thumbnail independently of audio
  resolution. Late results update the publication and trigger dock repaint;
  playback-clock changes still do not change the dock snapshot. Stop cancels the
  thumbnail task and rejects late results.
- The active-session record retains cover bytes up to 256 KiB. Small embedded
  artwork is a fallback when no browse thumbnail is present. Restoration detects
  JPEG, PNG, or BMP bytes instead of always assuming JPEG, and renders its cached
  cover without a thumbnail request until Play. Cover changes trigger a save even
  if playback position is unchanged.
- Three added release tests cover late updates, invalid/oversized/post-close
  results, and cover-versus-clock dock snapshots. The paused restoration test now
  checks cover round-trip and absence of thumbnail work. The player suite passes
  89 tests (one fixture test ignored); the optimized WebAssembly shell check passes.
- Runtime image rendering and availability of thumbnails generated after an
  initial miss still need acceptance testing. End-to-end restart, platform
  streaming recovery, and physical Kobo validation remain outstanding.

## Implementation checkpoint: browser confirmed seeks and buffering

- Browser progress and active-session snapshots now exclude unconfirmed seeks.
  Stop uses the last confirmed position, and asynchronous initial seeks are not
  considered complete merely because metadata loaded. Stale seeked events while
  the element is still seeking do not unblock persistence.
- Waiting/playing media events expose buffering to the dock. Waiting holds the
  observed position; pausing removes the buffering transport state without
  allowing later data arrival to undo the user's pause.
- Eight JavaScript event tests pass, and the optimized WebAssembly shell check
  passes. An installed Chromium also passes the real MP3-in-M4B test, including
  initial position, media-session skips, confirmed seek persistence, speed, and
  cleanup. The generated fixture is under target/web-audio-fixture.ry4FAf.
  Chromium needed a short user-cache temporary directory because its Unix socket
  path exceeded the limit with the repository's longer temporary path.
- This is not network fault-injection coverage: browser retry/reopen and Android
  recovery still need integration, as do end-to-end restart and Kobo acceptance.

## Implementation checkpoint: Android buffering protocol

- Media3 now publishes buffering separately from audible playback, gated by play
  intent and absence of playback suppression. Android's Rust bridge retains this
  state without accepting unready positions and clears it on release.
- The shared player no longer treats entering buffering as an explicit pause;
  the dock shows Pause while buffering and uses the existing static status label.
- A Rust protocol regression covers buffering, held position, pause, stale seek
  events, and release. The release player suite passes 90 tests (one fixture test
  ignored), and the Android optimized shell check passes. All 17 Android service
  release unit tests pass, including a new buffering-versus-pause test. Java tests
  excluded buildRustRelease and do not establish JNI/device integration.
- This exposes Media3's existing loading state; bounded stream retry, cancellation,
  network fault injection, disk restart, and Kobo acceptance remain to be completed.

## Implementation checkpoint: persistence worker restart

- Added an integration test using the frontend AppClient persistence API and
  actual ordered native dispatcher against a temporary on-disk data directory.
  Each phase awaits complete worker shutdown before constructing a fresh backend.
- The test saves an initial session and final position/speed, reopens and verifies
  the latest values plus origin, cover, and chapter metadata, then clears it,
  rejects a delayed old-session save, and verifies a third startup stays cleared.
- All four active-audiobook persistence tests pass in the optimized default
  backend configuration. This strengthens the earlier context-only round-trip
  evidence but does not execute the shell quit callback, rebuild a rendered dock,
  or exercise abrupt process termination. Those application-level restart gates,
  platform network recovery, and physical Kobo acceptance remain outstanding.

## Implementation checkpoint: shutdown mutation guard

- Beginning shutdown now seals the controller against old and new open requests,
  activation, replacement, and late Close actions while final saves are pending.
  Rejected incoming sessions are stopped, and library removal reports shutdown
  instead of deleting data during the final persistence barrier.
- Shutdown detaches session observation before stopping the producer, avoiding
  intermediate snapshots while waiting for Android's final service position.
  The forced final save remains allowed after the producer flush.
- A controller regression tests request rejection, idempotent shutdown, no late
  restore-record deletion, and continued availability of the final save barrier.
  All 10 Kobo-feature shell release tests pass; the WebAssembly optimized shell
  check passes. This is controller-state evidence, not full application restart
  or device-level shutdown validation; those acceptance gates remain outstanding.

## Implementation checkpoint: explicit pause during buffering

- Shared Pause now issues an idempotent pause command regardless of whether
  samples are currently audible. Android previously skipped the command when
  Media3 was loading, leaving its play intent active. The dock's Pause action
  uses this explicit path, preventing repeated presses against a stale Android
  observation from toggling playback back on.
- Android accepts a session-scoped pause command; browser audio exposes explicit
  pause as well. Native pause keeps its source retry gate closed and saves the
  confirmed position. Stopped sessions ignore subsequent pause requests.
- Release verification passes 90 shared-player tests, 18 Android service tests,
  and 9 browser event tests. Android and WebAssembly optimized shell checks pass.
  This validates command/state behavior, not network retry completion or real
  device lifecycle. The remaining network/restart/Kobo acceptance gates still apply.

## Implementation checkpoint: typed browser audio range failures

- Added an audiobook-specific range request/reply that carries bytes, retryable
  transport failure, or terminal failure through worker serialization. Document
  range requests retain their existing behavior. The audio path checks library
  membership and format, uses authenticated revision-pinned range reads, and
  classifies its request timeout as retryable.
- Browser sources now turn retryable replies into JavaScript errors marked
  `retryable`; terminal replies are unmarked errors. This provides the source
  proxy with the distinction needed for bounded recovery without retrying a
  changed source or rejected authentication.
- The optimized WebAssembly shell check and both release wire regressions pass.
  They cover temporary versus terminal failure and lossless binary/empty payloads.
- This is error propagation, not a completed browser retry loop. Source-proxy
  cancellation/pause gating, retry budget/backoff, and network fault-injection
  coverage remain required alongside the outstanding lifecycle/device gates.

## Implementation checkpoint: browser proxy retry and pause gating

- The tab's audio proxy now retries typed temporary failures up to eight attempts,
  with 500 ms exponential backoff capped at eight seconds. It retries the same
  offset/length and validates exact response length. Terminal/exhausted failures
  latch across subsequent requests instead of restarting the budget.
- Browser controls forward pause/resume to the retained remote source, including
  media-session pause and browser pause events. Pause gates automatic retries;
  closing a source or cancelling a service-worker request rejects pending delivery
  and clears pause waiters. Cancelled late results cannot poison later requests.
- Pending requests send heartbeats, preserving the service worker's dead-tab
  timeout while permitting deliberate pause and bounded multi-attempt recovery.
- Sixteen JavaScript tests pass, including seven source recovery regressions.
  The optimized WebAssembly shell check passes. A Chromium integration test using
  the production service worker verifies injected transient failure, identical
  retried range, pause gating beyond backoff, resume, and exact partial bytes.
- Still required: full media playback under network faults, heartbeat behavior
  during long pauses, explicit Play after exhausted retries, and cancellation of
  the underlying dispatched HTTP operation (delivery cancellation is implemented;
  an already dispatched read can run until its existing timeout). Android recovery
  and the remaining application restart/Kobo acceptance gates are not complete.

## Implementation checkpoint: streamed browser media and long pause

- Chromium now exercises BrowserAudio with the production remote proxy/service
  worker and a generated MP3-in-M4B fixture. Two injected range failures during
  opening recover to playback at the saved 40-second position; an explicit pause
  holds position and closes the source retry gate.
- The range integration also passes with AUDIO_PAUSE_TEST_MS=36000: heartbeats
  keep the paused request alive past its 35-second inactivity timeout, no retry
  occurs while paused, and resume returns the exact requested bytes.
- Browser media failures remain detectable after their message is consumed.
  Explicit Play on a failed engine now drops the exhausted source and resolves a
  fresh one at the last confirmed position. This restart is not automatic, and
  an unsuccessful seek is not used as its resume point.
- Seventeen JavaScript tests and the optimized WebAssembly shell check pass.
  The explicit reopen wiring is cross-checked, but a full rendered-shell Play
  after exhaustion still needs runtime validation. Mid-playback buffer-drain
  fault testing, underlying HTTP cancellation, Android recovery, application
  restart, and physical Kobo acceptance remain outstanding.

## Implementation checkpoint: browser cancellation cleanup

- Cancelling a request or closing its source now clears an active backoff timer
  immediately, rather than leaving it running until the delay expires. A read
  that synchronously triggers close has its eventual rejection observed, avoiding
  an unhandled-promise error after cancellation.
- Twenty JavaScript tests pass. Added regressions cover timer cleanup, continued
  source usability after request cancellation, and synchronous-close rejection.
- Chromium's production service-worker test now aborts a paused range request,
  verifies its port and retry waiter are released, and successfully reads a later
  range through the same source. Existing streamed M4B recovery checks also pass.
- This validates browser-side cancellation cleanup, not cancellation of an
  already-dispatched backend HTTP operation. That remaining boundary, mid-playback
  buffer exhaustion, Android recovery, and lifecycle/device acceptance still apply.

## Implementation checkpoint: audio request cancellation across workers

- Remote read callbacks now receive an AbortSignal. The Rust browser adapter
  observes it, drops its pending audio-range request, and removes its listener.
  The page's pending-request guard sends CancelRequest only for audio ranges that
  are still awaiting a reply. Other reads and mutation requests are unchanged.
- The backend worker registers audio-read cancellation before dispatch and races
  it against the read future, so cancellation drops the authenticated range read
  rather than merely dropping the response at the tab. Disconnect also cancels
  those registered reads through the existing per-port cancellation map.
- Optimized checks pass for both application-shell and web-backend-worker. Two
  release protocol tests pass, including wire compatibility and audio-only scope.
  Twenty-two JavaScript tests pass. Chromium confirms that the source callback's
  abort signal cancels a real stalled HTTP body; this integration injects a JS
  fetch callback, not the compiled Rust backend path.
- Full compiled-Rust worker/server cancellation timing remains unverified, as do
  mid-playback buffer exhaustion, Android recovery, application restart, and Kobo
  acceptance. The broader network-worker test initially lacked its generated
  coordinator WASM fixture. Its web-release build succeeded; the matching 0.2.126
  wasm-bindgen CLI was subsequently built in release mode under
  target/wasm-bindgen-cli, and bindings were generated under target/audio-cancel-policy.

## Implementation checkpoint: optimized worker cancellation contracts

- All 11 network-worker tests now pass against the generated Rust/WASM fixture.
  They cover cancellation before dispatch, waiting for headers, and reading an
  active body; worker failure also rejects pending reads and releases HTTP work.
- Three further Rust/WASM contracts pass: coordinator request cancellation before
  dispatch/first poll, sync shutdown cancellation, and executor timer cleanup.
- These establish cancellation behavior in the existing network/coordinator
  components. Combined with the earlier audio wire tests and browser proxy test,
  they provide component-level evidence, not an end-to-end test of the new audio
  request travelling through a rendered app, database worker, and real server.
  That full-path check and the remaining playback/restart/Kobo gates remain open.

## Implementation checkpoint: Android stream-control ownership

- Android's service-owned session now retains the source's AudioStreamControl.
  Explicit pause gates it; Stop cancels it before dispatching service release,
  avoiding a future retrying loader blocking the release command. Release events
  also cancel the control.
- Service events separately publish play intent, excluding playback suppression.
  Loading with play requested permits retries, pause suppresses them, and stale
  seek-sequence events cannot reopen the gate. This does not use audible playback
  as a substitute for intent while buffering.
- Ninety-two player release tests and all 18 Android service release tests pass;
  the optimized Android shell check passes. Two new Rust regressions cover source
  gating, stale events, and stopping before service start.
- Android still uses the single-attempt reader. Enabling the retrying adapter
  requires safe seek interruption/acknowledgement across Media3's loader boundary;
  that integration remains unfinished. Browser full-path cancellation and the
  other runtime/device acceptance gates above also remain open.

## Implementation checkpoint: Android loader seek generations

- Media3 seek discontinuities now signal a native source interrupt, covering
  both application commands and media-session controls. The source tracks a
  separate read generation rather than conflating it with UI progress sequences.
- A loader read acknowledges the latest generation after acquiring the source
  lock, seeks to its supplied byte offset, and rejects bytes if another seek
  superseded it during the read. This connects to the native range adapter's
  existing in-flight/retry interruption mechanism.
- Ninety-four player release tests pass (one fixture ignored), including latest
  generation acknowledgement and rejection of superseded bytes. Nineteen Android
  service release tests pass, including application/media-control seek interrupts.
  The optimized desktop-gpui Android library check passes, covering the new JNI
  entry point as well as shared-player code.
- The Android retry reader is not enabled yet. The next integration must verify
  Media3 handling of an interrupted blocked loader, including buffered seeks,
  before treating retry/seek recovery as complete. Other documented runtime and
  physical-device acceptance gates remain outstanding.

## Implementation checkpoint: Android retry adapter enabled

- Android remote M4B sources now use the bounded native audiobook retry adapter.
  The service-owned controls from the preceding checkpoints provide pause gating,
  stop cancellation, and seek-generation interruption; source revision validation
  and the shared retry budget are preserved.
- Explicit Play after exhaustion drops the failed Android source and reopens at
  the last accepted position. A Pause while buffered audio is still playing does
  not instead reopen the failed source. Reopening an already-buffering active book
  also no longer toggles its requested playback to pause.
- A real Media3 Loader regression blocks a load, cancels it, and returns an I/O
  error; Media3 reports cancellation rather than completed loading or playback
  failure. All 20 Android service release tests pass. The 94 player and 19 backend
  range release tests pass, as does the optimized Android desktop library check.
- Adapter enablement is not proof of on-device Media3 integration. Buffered-seek
  timing, audible buffer drain/recovery, source replacement, and explicit reopen
  after exhaustion still need real media/device acceptance. The remaining browser
  full-path, application restart, and Kobo acceptance gates also remain open.

## Implementation checkpoint: active-book reactivation after failure

- Selecting the active book from browsing now uses the same failed-source cleanup
  as dock Play. Cleanup occurs before applying an explicit chapter target, so
  that target is not replaced by the old engine's confirmed position. Healthy
  sources continue to be reused, including paused metadata/engine loads.
- A regression verifies healthy source-handle retention, retry-gate reopening,
  and target preservation. The player release suite passes 95 tests (one fixture
  ignored), and the optimized WebAssembly shell check passes.
- Failed-source reactivation now shares the existing recovery implementation;
  full rendered-shell exhausted-stream reactivation still needs runtime evidence.
  This change does not close the outstanding media/device acceptance gates.

## Implementation checkpoint: browser mid-playback buffer-drain acceptance

- The Chromium production-proxy test now supports AUDIO_DRAIN_TEST=1. With a
  generated ten-minute MP3-in-M4B fixture, it cuts source reads after playback
  begins, observes buffered audio continue, and waits for actual media starvation.
  Playback runs at 4x to keep buffer-drain validation short.
- The test verifies position holds while buffering, reconnect cannot undo an
  explicit pause, and Play resumes. A second loss/reconnect cycle verifies
  automatic recovery with uninterrupted playback intent and no new Play action.
  Both cycles pass through BrowserAudio, the tab proxy, and the production service
  worker with transient failures injected at the source-read callback.
- Invocation uses backend/web/audiobook_recovery.browser-test.mjs and fixture
  target/web-audio-fixture.ry4FAf/drain.m4b. Existing opening recovery, cancellation,
  exact-range, and real HTTP-body cancellation checks pass in the same run.
- This does not use the compiled library/account backend for injected source
  reads. Full app/backend transport, Android real-media behavior, process restart,
  and Kobo acceptance remain outstanding. Explicit approval for release deployment
  to the Kobo over SSH has been requested before hardware acceptance begins.

## Implementation checkpoint: rendered dock layout and invalidation

- Added GPUI rendered-component acceptance at 320, 600, and 1200 pixel widths.
  It verifies intrinsic dock height, retained browsing area, non-overlap, and
  transport controls remaining within dock bounds despite a long title. Real
  simulated clicks verify body Expand and button Close emit separate actions.
- An observer-level regression sends 20 playback-clock notifications and verifies
  the dock emits no invalidation notifications; a metadata change emits one.
  This supplements the earlier snapshot comparison with actual entity observation.
- The default player release suite passes 97 tests and the Kobo-feature suite
  passes 98, each with one ignored fixture test. Both include the rendered test.
- These are GPUI test-window checks with an isolated host, not a launched full
  application or physical e-ink display. Full-shell navigation/restart and device
  acceptance remain open; deployment approval is still pending.

## Implementation checkpoint: removal exclusion and save identity recovery

- A browser-owned library-removal guard now spans preparation, backend deletion,
  and result reconciliation. Duplicate deletion is excluded; success, either
  failure, and cancellation release the guard. Shell open, activation, replacement,
  and installation paths reject the library while the guard is active.
- Removal also invalidates already-reserved format probes for that library, even
  when no playback session has opened yet. Their late completion cannot become
  valid when the guard drops. Other libraries' unresolved probes remain eligible.
- The restoration writer tracks selected identity separately from successfully
  committed identity. Failed initial replacement no longer rejects all subsequent
  updates for the selected session; stale updates after close remain rejected.
- Optimized validation: 13 application-shell tests with audiobooks/Kobo pass;
  six browser library-removal tests with Kobo pass; application-shell WASM release
  check passes. Guard tests exercise its lifecycle with controlled futures, not
  an actual backend deletion from a rendered application.
- The review's separate save-failure recovery issue remains open: preparation
  still closes/clears the active session before the position flush result is known.
  It must preserve a paused recovery snapshot when saving fails. Full-process
  restart, complete app/backend streaming, and physical Kobo acceptance also
  remain outstanding; this checkpoint does not establish completion.

## Implementation checkpoint: recoverable removal save failures

- Active-library removal now retains the stopped session while awaiting the
  final progress producer, the saved restoration snapshot, and the restoration
  clear. It removes the dock only after those barriers succeed.
- Failure at any save barrier rebuilds the same session entity paused from its
  confirmed snapshot and original library, without opening a media source. The
  restoration writer receives a replacement so a previously selected tombstone
  cannot cause recovery updates to be ignored. Persistent disk failure still
  cannot guarantee durable recovery; the paused in-memory session is retained.
- Generation checks prevent late failure recovery from undoing Close, replacement,
  or shutdown. Shutdown overtaking a queued clear explicitly reselects its final
  restorable session instead of issuing an update after the writer selected None.
- The refresh task is now session-owned and cancelled on replacement, preventing
  recovery on the same entity from accumulating detached refresh loops.
- Release validation: 19 shell tests pass, including controlled failures at all
  three barriers, successful barrier ordering, late failure after Close, and
  shutdown during clear. The Kobo player suite passes 98 tests with one existing
  fixture test ignored; paused restoration/recovery asserts no media/source work.
  WASM release checking passes. Tests inject writer results rather than real disk
  failures or Android release callbacks; full-process/device acceptance remains
  outstanding, including shutdown overlapping an in-flight position producer.

## Implementation checkpoint: shutdown joins in-flight removal producers

- The controller retains shared completion futures for position producers taken
  by library removal. Shutdown joins these before saving its final restore record,
  even when Close has already removed the current session. Calling stop twice no
  longer lets shutdown mistake a previously taken producer for a completed save.
- Cancelling the removal waiter does not cancel the retained progress completion.
  Shutdown waits for all retained producers and the current session, reporting
  failures only after awaiting the remaining work.
- The optimized shell suite passes 21 tests. New controlled-producer tests cover
  success/failure with and without an intervening Close, and cancellation of the
  removal waiter. The optimized WASM check passes. These establish controller
  ordering, not actual Android release-event timing or full process shutdown.


## Review follow-up: invalid restore state and notification placement

- Invalid JSON, unsupported record versions, and invalid saved speed previously
  prevented both replacement and clearing because every save loaded the invalid
  record first. Loading now preserves its exact bytes under a unique
  `active_audiobook.invalid.<uuid>.json` registry key before clearing it.
  Failed storage reads or quarantine writes propagate without clearing state.
- Seven optimized backend restoration tests pass, including corrupt/future-version
  recovery, byte preservation, failed-quarantine preservation, explicit clear,
  stale-session rejection, and worker restart. Log:
  `target/audiobook-invalid-restore-tests-final.log`.
- The review's notification-overlap finding was incorrect. The browser page is
  positioned relative to its own reduced content area, so its notification moves
  above the dock. The actual optimized browser screenshot at
  `target/audiobook-browser-notification-acceptance/opened-dock.png` shows the gap;
  real dock Pause/Play clicks also succeed with the notification left visible.
  Removed the test's unnecessary notification-dismiss workaround and added
  explicit Pause/Play assertions. No notification layout change was made.
- The complete local browser regression passes with that notification visible:
  import, dock playback, expand/minimize, paused reload, explicit Play, and durable
  Close. Log: `target/audiobook-browser-notification-acceptance.log`. This run uses
  the existing optimized web bundle; the new invalid-record recovery was tested
  in the native backend suite, not rebuilt into this browser bundle.
- These checks do not establish physical Kobo acceptance or the complete remote
  browser streaming chain. No deployment was performed.

## Browser remote-fixture preparation (not runtime acceptance)

- `BackendWorkerHost` now has feature-gated `initialize_audio_fixture(port)` and
  `make_audio_fixture_remote(library, hash)` entry points. They use a separate
  fixed fixture profile and the existing loopback-only dummy account setup.
  Preparing remote media rejects ordinary hosts and uses normal local-copy
  eviction, retaining imported metadata and progress.
- Optimized WASM checks pass for the normal `worker_probe` example and the
  backend with `runtime-fixtures`. Logs: target/audiobook-worker-probe-check.log
  and target/audiobook-browser-fixture-host-check.log.
- The fixture is not yet wired into a browser worker/test runner. These compile
  checks do not prove remote media playback, request routing, or outage recovery;
  those runtime acceptance requirements remain open.

## Full browser remote run: reload failure remains unresolved

- Wired the fixture into `web-backend-worker` behind the non-default
  `audio-runtime-fixtures` feature. The ordinary worker does not export fixture
  preparation. Both normal release checks and the optimized fixture build pass.
- Added `audiobook-remote.browser-test.mjs`: serves the optimized GPUI bundle
  with a separate fixture worker, imports one real M4B, removes its local copy,
  and serves authenticated, revision-pinned HTTP ranges on loopback.
- Actual initial remote playback, ten-minute seek, expansion and minimization
  succeed. The trace in target/audiobook-browser-remote-pinned/remote-ranges.json
  proves both injected failed ranges subsequently succeed with identical bounds
  and revision. The original attempt to inject faults after playback began was
  invalid because Firefox had already buffered the seek target; corrected
  injection happens before opening and targets pinned reads only.
- Full acceptance FAILS at explicit Play after a paused reload. No Audio object
  is created within the 30-second test timeout, although backend range probes
  succeed. Reproduced in target/audiobook-browser-remote-diagnostic.log.
- A hypothesis that redundant service-worker registration caused the stall did
  not fix it (target/audiobook-browser-remote-reload-fixed.log); that speculative
  production change was removed. Root cause is not established. The test-only
  harness is retained to reproduce the failure. No device deployment occurred.

## Requested trial: browser playback without covers

- Removed cached-cover reads and fallback M4B artwork extraction from browser
  source preparation. Browser session code also skips its separate cover task
  and clears restored artwork for display. Native/Kobo behavior is unchanged.
- Release WASM UI check passes: target/audiobook-browser-no-cover-check.log.
- Built a new optimized fixture backend without cover preparation and tested it
  with the existing optimized UI bundle. Initial remote playback and seeking
  work, but Play after paused reload still times out before any Audio object is
  constructed: target/audiobook-browser-no-cover.log. This isolates removal of
  the backend's blocking artwork path; the new UI-only cover suppression was
  compile-checked, not rebuilt into that runtime bundle.
- No claim that covers caused the failure. Direct media URLs are pending the
  user's choice between adding server-side playback authorization support and
  keeping the server unchanged with a client-only transport simplification.

## Authorized direct browser delivery implementation

The user approved server-side playback authorization. Remote browser audio now
resolves through a normal backend request, then hands an ephemeral URL directly
to HTMLAudioElement; media bytes no longer pass through Rust range callbacks or
the audio service worker. Local files keep their blob path. Covers remain off
for this trial; native/Kobo playback is unchanged.

- Authenticated POST on a book's blob `/playback` endpoint creates a 15-minute,
  book/revision-specific capability. GET/HEAD support browser byte ranges,
  including suffix ranges. DELETE revokes that capability.
- The server rechecks the originating credential and library access on reads.
  URLs contain random capabilities, not account credentials, and must not be
  persisted or logged (including reverse-proxy access logs).
- Grants are bounded and process-local. Deploy the server before this client;
  multiple replicas require routing affinity or a shared grant store. Restart
  invalidates existing grants; the player can obtain replacements.
- Renewal preserves the confirmed position and rejects changed revisions.
  Pause/Close cancel retry intent and revoke late grants. Grant requests time
  out after 30 seconds. A stalled native request without a media error remains
  browser-managed; automatic stall recovery is not yet verified.
- Server release tests: 23 passed. Player JavaScript tests: 15 passed, including
  renewal, revision rejection, and Pause/Close cancellation. Real Chromium M4B
  playback with direct URLs, seeking and expiry renewal passed in
  `target/audiobook-direct-media-browser.log`.
- The full rebuilt GPUI remote test now passes: import, direct playback, seek,
  expand/minimize continuity, paused restore, explicit Play after reload and
  durable Close. Evidence: `target/audiobook-direct-full-browser.log` and
  `target/audiobook-direct-full-browser/`. The optimized shipping bundle is
  `target/audiobook-direct-bundle.JFQFFG`; the opt-in fixture worker is separate
  and must never be deployed. Local-file Chromium playback also passes in
  `target/audiobook-direct-local-regression.log`.
- Real-server HTTP authorization integration remains pending; the full browser
  test uses a loopback grant fixture, not a deployed server. Nothing has been
  deployed. This proves the new path avoids the old reload stall, not which
  internal operation caused that old path to stall.

## Deployment — 2026-09-09

- With user authorization, built `sync-server` using the optimized release
  profile for `x86_64-unknown-linux-musl` and deployed it with the provisioned
  sync-only installer. Kept the existing systemd configuration unchanged.
- Live readiness returns 200; an unknown playback ticket returns 410 and an
  unauthenticated grant request returns 401. Installed binary SHA-256:
  `810e5ba586378bef4b671fac4d112a560857c31cd7b71344498fe89884012053`.
- Deployed the tested production web bundle `899a8b8fc03ff073c53c506b` to
  `https://app.bokheim.se/`, current release `release.nu1E7P`. Preserved the
  existing admin UI byte-for-byte and retained older immutable bundles.
- Previous web release: `release.ZYFGSQ`. Previous sync binary/config retained
  on the host in `/var/lib/bokheim-deploy/audiobook-web-sync.Gn9AUU/`.
- Catalog and metadata services were not deployed or restarted. All three
  services report active. Authenticated real-account streaming remains a user
  acceptance check; deployment probes did not access anyone's library.

## Follow-up: dock layout and review fixes

- The application retains ownership of playback and supplies the dock as a
  browser content footer. Wide layouts keep the rail full-height alongside it;
  compact layouts put the dock above bottom navigation. Notifications are
  anchored in the content area above the footer; dialogs remain window-level.
- Media play promises are generation-scoped. A stale rejection cannot cancel
  a newer recovery or undo Pause, while a current transport rejection can start
  recovery even before the media error event.
- Set both default and current playback rate so source reloads retain speed.
- Page exit pauses direct playback and revokes its capability; a retained
  back/forward-cache page renews on its next explicit Play. Visibility changes
  still only save progress, allowing background audio to continue.
- Nineteen JavaScript tests pass. Real Chromium tests now inject a 503 during
  renewal, assert recovery at the selected speed, and verify revocation on an
  actual page reload: `target/audiobook-review-fixes-browser.log`.
- These follow-up changes have not been deployed.
- Full optimized GPUI/browser regression passed, including a pixel comparison
  proving that opening the dock leaves the sidebar unchanged, wide/compact
  layout screenshots, paused reload, and revocation of every fixture grant.
  Evidence: `target/audiobook-layout-fixes-browser.log` and screenshots in
  `target/audiobook-layout-fixes-browser/`. Visually verified the full-height
  rail, dock above compact navigation, and notifications above dock controls.
  Production bundle: `target/audiobook-layout-fixes-bundle.OAe6iz`.

## Simplification follow-up

- Removed the no-op playback-control callback across Rust/JavaScript and the
  dummy remote subscription channel. Browser source construction is synchronous;
  only local sources retain a real subscription lease.
- Added `RenewRemoteAudio`, which only calls authenticated URL issuance. Renewal
  no longer reruns metadata preparation or rejects a stream because a local copy
  was downloaded meanwhile. Revision checks remain in the browser player.
- Added one presentation-only audiobook footer for active playback plus pending
  loading/error states. The application retains session/task ownership and
  handles cancellation; browser and reader layouts place the same footer view.
- Legacy service-worker retirement is deliberately deferred to a separate
  migration. No deployment is included in this simplification.
- Validation: 19 JavaScript regressions and the real Chromium media test pass;
  optimized WebAssembly check and production bundle build pass. Bundle:
  `target/audiobook-simplify-bundle.mYIdk6` (`7c3a7528d238ce278c09fae8`).
- Full-app loading/renewal integration and the new native footer test remain
  unverified: the filesystem filled during compilation. The native test was
  retried with the required `reader-ui/native-audio` feature and one build job,
  but could not compile due to disk exhaustion. The fixture worker build also
  failed for lack of disk space. Do not treat its output as a verified worker.
- Removed obsolete `target/audiobook-remote-worker`,
  `target/audiobook-no-cover-worker`, and the redundant local upload archive
  directory `.deploy-tmp/audiobook-web-sync.jIFtA5` (about 90 MB). These are
  rebuildable; source, current bundles, and server rollback copies were retained.
