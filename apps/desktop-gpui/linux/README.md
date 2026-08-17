# Linux packaging

## AppImage

GitHub tag releases publish an x86-64 AppImage, its zsync delta metadata, and
its SHA-256 checksum. The AppImage embeds this update location:

```text
https://github.com/JohanAnderssonOstling/bokheim/releases/latest/download/Bokheim-x86_64.AppImage.zsync
```

Build the release binary and package it with versioned releases of
`linuxdeploy` and `appimagetool` available on `PATH`:

```sh
cargo build --locked --release -p desktop-gpui --bin desktop-gpui
apps/desktop-gpui/linux/appimage/build-appimage.sh
```

The packaging script writes `Bokheim-x86_64.AppImage`, its `.zsync` file, and
its `.sha256` file to `release/`. Set `RELEASE_DIRECTORY`, `DESKTOP_BINARY`,
`LINUXDEPLOY`, `APPIMAGETOOL`, or `UPDATE_INFORMATION` to override the defaults.
The release workflow also inspects the AppImage runtime and every packaged ELF
file and rejects an artifact that requires a glibc version newer than 2.35.

The Settings page exposes Bokheim's updater only when the `APPIMAGE`
environment variable identifies a writable file. Native development builds and
root-owned package-manager installations therefore do not check for updates.
Package launchers can explicitly disable the updater by setting
`BOKHEIM_DISABLE_SELF_UPDATE=1`; the AUR package must set this variable. Direct
AppImages check the latest public GitHub release at startup and notify the user
when a newer version exists. They download only after the user opens the update
action, verify the matching GitHub checksum, retain the previous AppImage beside
the replacement, and restart only after the user asks.

The GitHub workflow checks out the companion GPUI-Fork, HtmlEngine, and
HtmlViewCore repositories beside this repository because the Cargo workspace
currently uses sibling paths. Their pinned revisions must be updated whenever
Bokheim starts depending on newer companion commits.

## Flatpak

Flatpak builds use the `flatpak` Cargo feature. It keeps PDFium out of the
executable and loads the application-private library installed at
`/app/lib/libpdfium.so`.

The Flatpak manifest must fetch the pinned PDFium archive declared in
`../../renderers/pdf/core/build.rs`, verify its SHA-256 checksum, and make the
archive available to Cargo without network access:

```sh
PDFIUM_ARCHIVE_PATH=/run/build/bokheim/pdfium-linux-x64.tgz \
  cargo build --locked --release -p desktop-gpui --features flatpak
```

Install the artifacts into the Flatpak application prefix:

```text
/app/bin/desktop-gpui
/app/lib/libpdfium.so
/app/share/applications/se.bokheim.Bokheim.desktop
```

`PDFIUM_PACKAGED_PATH` can override the compile-time package location when a
different immutable application prefix is required. For local diagnostics,
`BOKHEIM_PDFIUM_LIBRARY_PATH` overrides the library at runtime.
