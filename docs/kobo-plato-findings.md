# Kobo findings from `reference/plato` (as source for planning)

This document captures only what was observed in the `reference/plato` project and
does not propose implementation steps.

## High-level identity

- `reference/plato/README.md` identifies Plato as a **document reader for Kobo e-readers**.
- The same file lists supported Kobo devices (e.g., Libra, Aura, Clara, Forma, Elipsa) and supported firmware families.
- Supported formats include PDF, CBZ, FB2, MOBI, XPS, TXT (via MuPDF), ePUB (built-in renderer), and DJVU (DjVuLibre).
- The project is explicitly a **standalone Kobo reader application**, not a framework or SDK.

## Build/deployment context

- `reference/plato/BUILD.md` states:
  - Build toolchain is tailored for Kobo/embedded deployment (`arm-unknown-linux-gnueabihf` target appears in `reference/plato/build.sh`).
  - There is a separate developer emulator path requiring SDL2.
- `reference/plato/build.sh` uses `cargo build --release --target=arm-unknown-linux-gnueabihf -p plato`.
- Runtime dependencies for embedded build are managed via `thirdparty` tooling and a `libs` bundle.

## Where “UI” code lives in Plato

- Main app entry: `reference/plato/crates/plato/src/main.rs`.
- Core view/event abstraction: `reference/plato/crates/core/src/view/mod.rs`.
  - Defines custom `View` trait and manual event/render/children handling.
  - Includes `handle_event`, render queue, traversal, and ID-based popup/menu handling.
  - No reference to Qt/GTK/GTK-rs, SDL2, Nickel, Nickel UI APIs, or web-based UI framework.
- Core platform modules:
  - `reference/plato/crates/core/src/framebuffer/mod.rs` defines a platform framebuffer abstraction.
  - Concrete framebuffer implementations exist in:
    - `reference/plato/crates/core/src/framebuffer/kobo1.rs`
    - `reference/plato/crates/core/src/framebuffer/kobo2.rs`
- Emulator path:
  - `reference/plato/crates/emulator/src/main.rs` uses SDL2 and wraps the same core view layer for host simulation.
  - This is a development target, separate from Kobo device rendering.

## Input and device handling

- `reference/plato/crates/core/src/input.rs` implements raw input handling with Linux evdev-like constants (`EV_KEY`, `ABS_MT_*`, key codes, etc.) and file-based device event parsing.
- The on-device app wires raw device streams through:
  - `raw_events(...)`
  - `/dev/input` paths in `reference/plato/crates/plato/src/main.rs`.
- Gesture parsing also exists in the core path (`crate::gesture`), again integrated into the custom event loop.

## Rendering pipeline shape

- Plato has document modules for format-specific rendering under `reference/plato/crates/core/src/document/*`.
- Styling hooks include `css/*.css` (e.g., `epub.css`, `html.css`, dictionary styles).
- Rendering is done through the custom `Framebuffer`/`View` stack rather than a separate widget toolkit.
- Update strategy includes framebuffer update modes (`Gui`, `Partial`, `Full`, `Fast`, `FastMono`) in the framebuffer abstraction.

## Library browsing behavior

- Home/library browsing, search, menus, and reader chrome are implemented in custom view modules under `reference/plato/crates/core/src/view/` (home, reader, menu, dialog, notification, top_bar, etc.).
- The reader and browser are both part of the same custom runtime, not separated as external UI framework screens.

## Packaging and codebase signals that are relevant

- Workspace is purely Rust crates:
  - `reference/plato/Cargo.toml`
  - `reference/plato/crates/core/Cargo.toml`
  - `reference/plato/crates/plato/Cargo.toml`
  - `reference/plato/crates/emulator/Cargo.toml`
- No separate “UI toolkit crate” dependency is declared for the Kobo runtime beyond custom code and emulation dependencies.

## License implications

- `reference/plato/LICENSE-AGPLv3` indicates AGPL licensing for Plato upstream.
- Any reuse strategy must account for this if copying or deriving code.

## Practical summary of findings

- The on-device Plato experience is based on:
  - custom view/event model,
  - custom framebuffer rendering,
  - raw input event ingestion,
  - Rust crate ecosystem assembled specifically for Kobo.
- It is not a thin wrapper over Nickel or a known general UI framework; it is effectively its own Kobo-specific UI/runtime stack.
