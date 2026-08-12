# Linux packaging

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
