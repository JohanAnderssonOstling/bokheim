# Bokheim GPUI for Web

This is the browser build of the same GPUI application used by the native
targets. It renders GPUI into a browser canvas and keeps browser-specific
storage behind the SQLite OPFS VFS used by the shared Rust backend.

Build the release bundle with:

```sh
./apps/web-gpui/scripts/build.sh
```

The build requires the `brotli` command and generates verified `.br` and `.gz`
sidecars for WebAssembly, JavaScript, and other text assets, including PDFium.
The local server and the deployment Caddy configuration negotiate Brotli first,
then gzip, using the regular asset URLs and `Content-Encoding`. Browsers decode
responses before WebAssembly streaming compilation; no JavaScript Brotli decoder
or `DecompressionStream` support is required. Hosts that do not serve sidecars
continue to serve the original assets. Production Brotli delivery requires
installing and reloading the updated Caddy configuration as well as deploying
the assets; the web-only deployment script does not reload Caddy.

Then serve `apps/web-gpui/dist` with cross-origin isolation headers enabled.
For an isolated test bundle, set `BOKHEIM_WEB_OUTPUT_DIR` to an absolute
task-specific directory when building, then pass that path to the server's
`--directory` option. `--port` selects a separate local test port.
For local development, the included server configures the required headers:

```sh
python3 apps/web-gpui/scripts/serve.py
```

Before downloading the application module, the page checks browser version,
secure context, cross-origin isolation, shared WASM memory, workers, private
storage, and Web Locks. Unsupported browsers receive a short message with the
required browser version; technical diagnostics are logged to the console.
The version policy is Chromium desktop 109+, Chromium Android 148+, Firefox
114+, and Safari/iOS 16.4+; these are necessary version floors, not a claim that
every device running those versions has been validated. Unknown browser brands
are checked by capability. Media Session is optional.

The startup probe checks that a shared module worker can load. Tabs create the
dedicated database and CPU workers, which communicate directly with the shared
coordinator through MessagePorts. A tab lifetime lock detects a lost host; the
coordinator selects a replacement and restores database connections. An
exclusive database-worker lock prevents overlapping ownership. Interrupted
requests fail without automatically replaying writes; subscriptions reconnect.
Network requests execute asynchronously in the shared coordinator.

The release bundle includes a content-addressed, gzip-compressed BISAC reference
SQLite asset. Workers verify the compressed digest and decode directly into
SQLite's fixed-size installation buffer, then cache the smaller database in
their separate OPFS storage pools. The schema derives parent labels and path
components instead of storing duplicate strings. Queries use a 128 KiB SQLite
page-cache setting. Cached files track the full database artifact digest so
schema and normalization changes are picked up on the next worker start.

Browser Back dispatches the same `DismissSettings` action as native Android:
the focused page dismisses its modal/search or climbs one breadcrumb level,
the shell handles its overlays and global pages, and the application closes
the reader. An unhandled Back leaves Bokheim for the preceding browser page.
Sidebar selections do not create navigation history. Forward does not reopen
dismissed screens.

Compact browser windows and the mobile app use a shared Back button and current
page title instead of breadcrumbs. Mobile EPUB/PDF readers show that same header
above their reading controls. The header, controls, and search overlay the full
book viewport, so showing or hiding them does not repaginate the book. On Android,
the system status and navigation bars follow reader-control visibility and also
overlay the page.

The web shell arms a history guard after pointer or keyboard interaction and
returns to it with `history.forward()` while GPUI consumes Back. It does not
push replacement entries during `popstate`, which Chromium would mark as
skippable by the next toolbar Back. Reloads reuse an existing guard. Browser
history jumps can bypass it, and a directly opened tab with no preceding page
stays open when Back is unhandled.

The web build includes the shared audiobook player. Audiobooks open in a bottom
dock; clicking its title expands the full player, and Back minimizes it without
stopping playback. The session survives browsing and library switching while
retaining its originating library for progress saves. A browser audio element
plays M4B files, while Media Session exposes title, author, artwork, play/pause,
15-second rewind, 30-second forward, and position/seek controls where supported.
The browser and operating system choose the lock-screen layout. If automatic
playback is blocked, tap Play in the dock or full player. No PWA installation is required.

Playback position is saved from audio events (including external seeks and
pause), rather than relying on the GPUI redraw timer. Hiding the page also
requests a save. Explicitly closing the dock stops playback and releases its
source. Reload restores the active dock paused; Play opens the media source.
Closing or unloading the browser page stops playback; background
behavior and supported M4B codecs depend on the browser.

Local M4B playback uses a Blob URL backed by the stored file. Its worker
subscription retains the file generation until playback closes. Remote M4B
playback uses range requests through the audio service worker.

Document readers receive a source descriptor rather than complete file bytes.
Local sources use bounded range requests and retain their file generation for
the subscription lifetime. After a database worker replacement, reopen the
document to acquire a new source; its old capability is not restored.

Audio checks, from the repository root:

```sh
node --test apps/audiobook-player/src/web_audio.test.mjs
# Requires Playwright (or PLAYWRIGHT_MODULE) and a real M4B fixture:
node apps/audiobook-player/src/web_audio.browser-test.mjs /path/to/book.m4b
RUSTC_BOOTSTRAP=1 cargo check --release -p web-gpui --target wasm32-unknown-unknown
```

## PDF reading

The web build uses PDFium WASM with the regular GPUI PDF reader, including page
navigation, zoom, margin trimming, text selection, search, and saved positions.
PDFium loads lazily in one dedicated worker per tab. That worker owns PDFium's
JavaScript bindings and document handles; the UI receives page pixels, text
geometry, and search results through the application's shared Rust memory.
Closing a reader releases its document on the worker.

Library PDF thumbnails use the existing CPU worker and cover storage pipeline.
Each CPU worker lazily loads its own isolated PDFium instance when a PDF cover
is requested. The first page produces 300px browse and 600px high-density JPEG
covers.

After building the web bundle, test thumbnail extraction with:

```sh
node client/app/scripts/test-pdf-thumbnails.mjs
```

This requires Playwright Chromium and Python Pillow and accepts
`PLAYWRIGHT_MODULE`, `CHROMIUM_PATH`, and `PDF_THUMBNAIL_TEST_DIST` (the bundle
directory containing `cpu_worker_loader.js`). It checks lazy loading, recovery
from a blocked PDFium download, both cover sizes, and repeated extraction.

The current upstream WASM API opens complete byte buffers. PDFs therefore load
fully before reading, and their bytes occupy memory in both the Rust and PDFium
WASM instances. This is not PDF range streaming.

The build packages checksum-verified PDFium 7902 assets in the immutable release
bundle. The worker adapts a character-box output argument ordering bug in
`pdfium-render` 0.9.3; the Rust dependency is pinned so an upgrade cannot silently
invalidate that adapter. Native PDFium loading remains unchanged.

## Remote EPUB reading

Undownloaded EPUBs open through authenticated HTTP byte ranges. The backend
returns a transient remote-source descriptor; the EPUB parser reads ZIP entries
on demand on a GPUI background worker. Downloaded EPUBs continue to use local
bytes. Each open remote reader retains at most 64 cached blocks of 64 KiB (4 MiB),
and closing it discards that cache. Sequential cache misses grow HTTP batches
from 64 to 128 to at most 256 KiB; non-contiguous seeks reset them to 64 KiB.
Batches stop before already cached blocks and at EOF. Nearby bytes fetched in a
batch share the same 4 MiB cache budget and can serve subsequent reads.
Streaming does not mark the book downloaded; use Download to make the complete
book available offline.

Opening fetches the first 64 KiB (or the whole file if smaller), using its
Content-Range total as the file length and caching the validated prefix. The
worker response carries that prefix once; subsequent requests carry only the
file identity and requested bounds. No separate HEAD request is needed.

The server must support byte ranges, Content-Range, and an ETag matching
the book's content hash. Range reads reject changed identities or wrong bounds.
File descriptors contain only content identity and length. The backend supplies
current credentials for each range and refreshes/retries once after HTTP 401,
including the initial prefix request.
The UI thread never performs synchronous network reads. Chapter navigation,
TOC loading, and opening image resources acquire file data on parser workers;
failed chapter reads leave the current page available for retry.

The UI build uses shared WebAssembly memory and a GPUI parser-worker pool. Its
standard library and C dependencies must be built with atomics and bulk-memory
support, with the TLS exports required by wasm-bindgen; `scripts/build.sh`
supplies these flags. Backend workers remain
separate instances. The browser must support `Atomics.waitAsync`. The file bridge
uses that API to serve parser requests on the existing UI event loop, which
forwards authenticated byte reads to the backend. It creates no nested network
workers and transfers no JS objects between parser threads.

Validate the actual Rust file source across browser parser workers with:

```sh
bash client/app/scripts/test-epub-range-reader.sh
```

Set `PLAYWRIGHT_MODULE` and `CHROMIUM_PATH` if Playwright or Chromium are installed
outside their default locations.

The test builds a small shared-memory WASM harness from the production file
source. It checks seeking, cache reuse across workers, credential changes, and
rejection/cancellation of full, stale, misaligned, truncated, and oversized
responses. It also checks that slow reads keep the UI event loop responsive,
that network loss and expired credentials report errors and recover, and that
the harness discards openings and reads completed after closure. The HTTP session test below separately exercises the
real authentication refresh endpoint for both metadata and range requests.

```sh
cargo test --release -p app --no-default-features --features watcher --lib remote_file_refreshes_expiring_and_rejected_tokens_over_http
```

Rust's `rust-src` component and a matching wasm-bindgen CLI are
required. `REMOTE_FILE_TEST_TARGET`, `REMOTE_FILE_TEST_PKG`, and `WASM_BINDGEN`
can override their default locations.

To also benchmark actual EPUB opening and chapter reads in WASM parser workers:

```sh
REMOTE_FILE_BENCHMARK=1 bash client/app/scripts/test-epub-range-reader.sh
```

The benchmark injects 80 ms per HTTP request and reports elapsed time, request
count, and bytes transferred over three trials. `REMOTE_FILE_BENCHMARK_LATENCY_MS`
changes that delay; `REMOTE_FILE_BENCHMARK_REPORT` saves the detailed JSON report.
See [the benchmark fixture and recorded comparison](../../client/library-backend/crates/book-access/tests/remote-file-browser/BENCHMARK.md).

## Tabs across web deployments

Each immutable release bundle has its own coordinator URL. Tabs running the same
bundle share that coordinator. A newer bundle waits for the user to close older
tabs before it can acquire database ownership and migrate local data. Existing
tabs keep running; deployment retains their immutable assets for later requests.

## Authentication

The web Settings page opens a real HTML authentication dialog so password
managers can discover the username and password fields. It submits through the
existing Rust account client.

## Mobile browser input and layout

The browser uses the shared phone controls when its primary pointer is coarse
(`(pointer: coarse)`). Control density is independent of the existing window
width classes: a tablet can keep its sidebar while using touch-sized controls.
Changes to the primary pointer refresh cached views without reloading. The web
always retains its on-screen Back button; it does not enable the native mobile
Cargo feature.

Canvas layout uses CSS pixel dimensions even on high-DPI displays. Physical
backing dimensions use the browser's device-pixel box when it agrees with DPR,
and otherwise use CSS size times DPR (including browser device emulation).

The GPUI web platform dispatches raw touch contacts and recognizes taps,
scrolling, long presses, and two-finger pinch gestures. A claimed contact, a
cancelled gesture, and the remaining finger after a pinch cannot produce a
click. Lost pointer capture and a hidden tab cancel active contacts. Mouse and
hardware keyboard input keep their existing paths.

Safe-area CSS values and VisualViewport changes feed GPUI's window insets.
Browser toolbar changes resize the canvas; keyboard occlusion moves overlays
and controls above the keyboard without changing the book viewport. Tapping a
text field finishes its focus frame synchronously before requesting the browser
keyboard. Other taps retain hardware-keyboard focus with `inputmode=none`.
Mobile `beforeinput` and composition events feed the focused GPUI text handler.

To exercise the actual backend and shared controls in Chromium with desktop,
phone, and tablet input emulation, build the small fixture:

```sh
RUSTC_BOOTSTRAP=1 cargo build --release -p web-gpui --example mobile_probe --target wasm32-unknown-unknown
python3 apps/web-gpui/scripts/test-mobile-browser.py target/mobile-browser-probe --wasm target/wasm32-unknown-unknown/release/examples/mobile_probe.wasm
```

The runner packages the fixture with `wasm-bindgen` (from `PATH`,
`WASM_BINDGEN`, or the repository's `target/wasm-bindgen-cli/bin` installation),
creates its test page from the production HTML shell, and serves it locally
with cross-origin isolation headers.

Pass `--browser webkit` to run the fixture with an installed Playwright WebKit.
`WEBKIT_PATH` can select another Playwright-compatible WebKit executable. Chromium
uses browser-generated touch gestures; WebKit exercises the same production
listeners with injected PointerEvents.

The fixture needs no account, backend workers, library, or book downloads. It
checks density and width classes, capability changes, tap/drag separation,
scrolling, holds, pinch, cancellation, text input/composition, and viewport
insets. Python Playwright and Chromium are required. `CHROMIUM_PATH` overrides
the Chromium executable. Hardware keyboards, virtual keyboards, browser bars,
and OS gestures should also be checked on physical Android and iOS devices.
