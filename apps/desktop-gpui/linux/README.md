# Linux packaging

## AppImage

Linux desktop and Kobo installer releases target glibc 2.17. The main app,
bundled native libraries and AppImage runtime are checked against that ceiling.
The optional WebKitGTK browser helper retains its separate glibc 2.39 ceiling.

The release tooling builds an x86-64 AppImage and its SHA-256 checksum.
GitHub can store these artifacts, while in-app update discovery uses the signed
Bokheim feed. The package does not embed a separate zsync update endpoint.

Build the release binary and package it with versioned releases of
`linuxdeploy` and `appimagetool` available on `PATH`. The build script prepares
the checksum-pinned Zig 0.15.2/native SDK automatically; it does not use the
host's glibc headers or C++ runtime for the main executable. Install Python
3.11+, `cpio`, `zstd`, `pkg-config`, `file`, `binutils`, and the native GPUI
development packages on the build host. Docker and an old distribution are not
required. SDK downloads and generated files stay in `.linux-toolchain/`; set
`BOKHEIM_LINUX_SDK` to use another cache directory. `--offline` requires those
verified archives to have been downloaded already.

```sh
python3 apps/desktop-gpui/scripts/build.py
apps/desktop-gpui/linux/appimage/build-appimage.sh
```

The webview helper uses GPUI for browser controls and WebKitGTK 4.1 for
webpages. Browser windows embed WebKitGTK using X11, including XWayland in a
Wayland session; OPDS verification uses a GTK window with native Wayland
support. Build hosts need
`libgtk-3-dev` and `libwebkit2gtk-4.1-dev`. The AppImage does not bundle
WebKitGTK: the helper is copied without linuxdeploy and uses the system's
`libwebkit2gtk-4.1` at runtime, so WebKit security updates come from the
distribution. Without it, the rest of Bokheim works: browser verification
reports that WebKitGTK is missing, and stores are not offered.

The browser helper is optional in the AppImage. By default packaging includes
the executable beside `bokheim` if it exists. `BOKHEIM_BROWSER=required` makes a
missing helper an error; an explicit `OPDS_BROWSER` path also requires that file.
To build and package without a browser (and without WebKitGTK development
packages), use both options so a stale helper is not accidentally included:

```sh
python3 apps/desktop-gpui/scripts/build.py --without-browser
BOKHEIM_BROWSER=disabled apps/desktop-gpui/linux/appimage/build-appimage.sh
```

For one AppImage with different compatibility baselines, build the main app
with `--without-browser` on the core build host, build `opds-webview` on the
browser build host, then set `OPDS_BROWSER=/path/to/bokheim-opds-webview` when
packaging. Only that executable is copied; its system libraries are not bundled.
The main app and all its bundled libraries must satisfy glibc 2.17, while the
optional helper may require up to glibc 2.39 (Ubuntu 24.04). Its build uses the
host toolchain and its process restores the system library path from before
AppRun. The main app's compatibility libraries do not override WebKitGTK's
dependencies.

The SDK pins its GNU C++ headers and runtime together. Packaging includes a
fallback C++ runtime for older hosts, but prefers a usable system runtime so
modern graphics drivers retain their matching C++ ABI and X11 libraries. The
same fallback directory includes the SDK's XCB core/XKB and xkbcommon libraries;
XCB graphics-driver extensions are not bundled. Fontconfig, FreeType, ALSA, WebKitGTK and graphics
drivers come from the distribution. A 2.17 glibc baseline does not remove their
own API and hardware requirements; old kernels and drivers still need runtime
validation.

The Linux Kobo installer uses the same SDK and packaging checks:

```sh
python3 scripts/linux/toolchain.py -- cargo build --release --manifest-path apps/kobo-installer/Cargo.toml
apps/kobo-installer/linux/build-appimage.sh
```

At runtime the app probes the helper once per launch, with a five-second timeout.
A missing helper, incompatible glibc, absent WebKitGTK or failed probe hides the
browser button and stores. Store windows also require X11/XWayland. The probe
checks that the helper can load its runtime; actual window creation can still
fail and is handled when opening a browser. Ordinary OPDS requests and the
reader do not use the helper. Feeds requiring browser verification remain
unavailable when the helper cannot run.

The embedded browser uses a GTK-owned native surface parented inside the
GPUI window. GTK handles its native focus events directly; the helper routes
keyboard focus between the page and address bar and restores page focus after
switching windows. The GTK surface follows the GPUI viewport when resizing.
Address history appears in a dropdown attached to the shared search-field
component. While it is open, a native clipping surface reveals that GPUI
surface and routes clicks to it without changing the webpage's viewport. Its
shape is independent of GTK's painting. Closing the
dropdown restores the full native surface.

Browser windows receive the application's browsing preferences at launch and
use the shared palette, fonts, and toolbar components. Since the helper runs
in a separate process, its theme remains that launch snapshot. On Linux the
same light/dark preference is exposed natively to websites through
`prefers-color-scheme`; websites choose their own styling in response. Download
notifications reuse the main application’s toasts at the bottom right, including
the filename, active transfers, completion, and failure. On Linux, downloads
with a reported size show a native progress bar and percentage; unknown sizes
keep the loading indicator. Successful downloads
auto-dismiss; failures stay visible until dismissed. On Linux, native page
clipping keeps toasts and address suggestions visible together without resizing
the webpage.
Back and Forward use native history state to disable unavailable navigation.
The link context menu omits “Download Linked File”. Opening a link in a new
window creates another GPUI browser window with the same profile, preferences,
and download handling; the original page stays open.

Magnet links and `.torrent` downloads start the library's Rust torrent worker.
It downloads all files with default networking, then moves the completed folder
into the library for its normal scan. Torrent metadata and book associations
stay in the device's library database. Seeding follows whole-folder moves made
in the app; the book detail actions stop or resume seeding, or remove the torrent
while keeping its files. Transfers run while the application is open and resume
when the library is reopened.

HTTP and torrent transfers use the same download notifications and progress bars.
Each torrent has a separate entry, updated once per second. Downloaded `.torrent`
metadata hands its notification to the payload transfer; completion reports
whether seeding continues. Both kinds show the same waiting and error states.

Settings → Transfers has one "Allow automatic transfers on metered connections"
setting, off by default. Automatic uploads, cover/catalog updates and completed
torrents' seeding wait and resume when allowed. Opening and reading books,
explicit downloads from the library, catalog, OPDS or browser, and approved app
updates proceed on metered connections. Pending user-requested downloads retain
this exemption across retries and restarts. Linux uses NetworkManager's reported
cost; unavailable cost information is treated as unknown and allowed.

The globe button, store links, and feed verification share the original globe
browser profile under `opds-webview/stores` in the application's data directory.
Cookies and website storage persist between launches. On Linux, cookies from
older per-site profiles are imported once into `cookies.sqlite`, preserving
their expiration and security attributes. SQLite keeps concurrent windows from
overwriting each other's saved cookies; the old cookie files are retained.

The focus regression uses a local page and an isolated X11 display. With
Python GTK 3 bindings, `xdotool`, and Xvfb installed, run it after building
the helper:

```sh
xvfb-run -a python3 apps/opds-webview/tests/native_focus.py --display-env
xvfb-run -a python3 apps/opds-webview/tests/profile.py --display-env
xvfb-run -a python3 apps/opds-webview/tests/responsiveness.py --display-env
```

The responsiveness regression hangs a page with an infinite JavaScript loop
and checks address editing, navigation, Back, Forward, Reload, and shutdown.

The UI regression also needs ffmpeg, tesseract with English language data, and
Python Pillow. It checks dropdown selection with both mouse and keyboard,
unchanged page dimensions, the native color-scheme hint, toast lifecycle,
simultaneous overlays, and navigation availability using a local server:

```sh
xvfb-run -a python3 apps/opds-webview/tests/ui.py --display-env --appearance dark
xvfb-run -a python3 apps/opds-webview/tests/ui.py --display-env --appearance light
```

The packaging script writes `Bokheim-x86_64.AppImage` and
its `.sha256` file to `release/`. Set `RELEASE_DIRECTORY`, `DESKTOP_BINARY`,
`LINUXDEPLOY`, or `APPIMAGETOOL` to override the defaults.
Packaging and local release checks inspect the AppImage runtime and every
packaged ELF file. The runtime, main executable and bundled libraries must
require no newer than glibc 2.17. Only `usr/bin/bokheim-opds-webview` is checked
separately against glibc 2.39; no other executable or library is exempted.

Update discovery is enabled only when the build contains a valid public signing
key. Settings shows one row when an update is available. Explicit Update consent
persists approval and stages verified packages; activation waits for a later
launch. Restart is an optional explicit action. Database/taxonomy changes and the
AppImage replacement use a recoverable startup transaction.

The `APPIMAGE` environment variable identifies the package that can be replaced.
Without it, the host can offer compatible taxonomy updates only. Package launchers
can also disable application replacement using `BOKHEIM_DISABLE_SELF_UPDATE=1`;
the AUR package must set this variable. Installation permissions and actual storage
deficits appear only when action is required. See `servers/updates/README.md` for
public-key configuration, release preparation and signed feed publication.

The hosted platform workflows check out the companion GPUI-Fork, HtmlEngine, and
HtmlViewCore repositories beside this repository because the Cargo workspace
currently uses sibling paths. Their pinned revisions must be updated whenever
Bokheim starts depending on newer companion commits.

## Flatpak

Flatpak builds use the `flatpak` Cargo feature. It keeps PDFium out of the
executable and loads the application-private library installed at
`/app/lib/libpdfium.so`.

The Flatpak manifest must fetch the pinned PDFium archive declared in
`shared/pdfium/artifacts.toml`, verify its SHA-256 checksum, and make the
archive available to Cargo without network access:

```sh
python3 shared/pdfium/prepare.py --target x86_64-unknown-linux-gnu \
  --archive /run/build/bokheim/pdfium-linux-x64.tgz --offline
python3 scripts/linux/toolchain.py --offline -- cargo build --locked --release -p desktop-gpui --features flatpak
cargo build --locked --release -p opds-webview
```

The Flatpak source inputs must also include the checksum-pinned SDK archives
from `scripts/linux/toolchain.json` in `.linux-toolchain/downloads/`. The core
executable uses the same 2.17 target; the browser builds against Flatpak's own
WebKitGTK runtime.

Install the artifacts into the Flatpak application prefix:

```text
/app/bin/bokheim
/app/bin/bokheim-opds-webview
/app/lib/libpdfium.so  # from .pdfium/x86_64-unknown-linux-gnu/
/app/share/applications/se.bokheim.Bokheim.desktop
```

`PDFIUM_PACKAGED_PATH` can override the compile-time package location when a
different immutable application prefix is required. For local diagnostics,
`BOKHEIM_PDFIUM_LIBRARY_PATH` overrides the library at runtime.
