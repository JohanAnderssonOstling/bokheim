# Desktop compilation

Build the Windows or macOS app and browser helper together with Cargo:

```sh
cargo desktop --release --timings
```

The alias selects only the two desktop executables and their desktop and release
logging features. Cargo can share compatible dependencies between them. Add
`--target aarch64-apple-darwin` or another supported desktop target when needed.
Without `--release`, Cargo uses its development profile. Use the explicit Cargo
package commands if diagnostic builds need to omit `release-logging`.

Cargo owns Rust compilation. Existing crate `build.rs` scripts generate code,
validate prepared PDFium inputs and configure native linking. The browser helper's
`build.rs` applies its Linux linker exception only to that executable. Build
scripts do not invoke another Cargo build, download voice models or package apps.

## Native inputs and runtime files

Prepare the checksum-pinned PDFium inputs once before compiling. For example,
on Windows:

```sh
python shared/pdfium/prepare.py --target x86_64-pc-windows-msvc
cargo desktop --release --timings
python apps/reader/crates/speech/prepare.py --voice all --dest target/release/tts
```

Native speech libraries are handled by `sherpa-onnx-sys`; set
`SHERPA_ONNX_LIB_DIR` when using prepared libraries, as the release workflows do.
On macOS, substitute the macOS target when preparing PDFium and add the same
`--target` to Cargo. With an explicit target, stage voices under
`target/<target>/release/tts` instead. To run an unpackaged macOS executable,
also prepare PDFium into `target/<target>/release/pdfium`.

Change those output paths if using `CARGO_TARGET_DIR` or a different profile.
Keep the target directory for subsequent builds so Cargo can reuse it.
Windows and macOS workflows call Cargo directly, then stage assets before their
existing installer/bundle packaging steps. Python remains in native dependency
preparation and packaging; there is no Python desktop compilation wrapper.

## Linux releases

Follow [the Linux instructions](linux/README.md#appimage). The app uses a pinned
glibc 2.17 SDK; the helper also needs host GTK/WebKit development headers and an
isolated library directory. Prepare those environments, then run
`cargo desktop-app --release` and `cargo desktop-browser --release` separately.
Do not use the combined alias for Linux releases: it would merge their native
library environments. Both executables and packaged libraries retain the glibc
2.17 compatibility checks.
