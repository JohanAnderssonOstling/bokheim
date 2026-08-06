# Plan 31: Host + event/command boundary (retire the signal mesh)

Third step of the HtmlRenderer decoupling program (29 → 30 → 31 → 32). The largest of the
floem cuts. Land it in the three sub-steps below; each compiles and ships on its own.

## Problem

`HTMLRenderer` carries **21 `RwSignal` fields** (`renderer/mod.rs:140-217`) plus ~20 public
`*_signal()` accessors. They are the entire API surface between the renderer and `UI/`.

The key observation: **every inbound read is `get_untracked()`** — all 14 of them. The
renderer polls; it never subscribes. The reactive graph does nothing for the renderer's own
logic, so the signals are functioning purely as a shared mailbox. They can be replaced 1:1
with no behavior change.

The rest of the floem surface outside `view.rs` is tiny: `ViewId::{request_paint,
request_style, get_size}` (6 sites), `exec_after` (`render.rs:496`), and `floem::Clipboard`
(`mod.rs:230`, already behind the `ClipboardWriter` closure at `mod.rs:200`).

## Change

### 31a — `RendererHost` (no public API change)

```rust
pub trait RendererHost {
    fn request_repaint(&self);
    fn request_style(&self);
    fn viewport_size(&self) -> Size;
    fn schedule(&self, delay: Duration, f: Box<dyn FnOnce() + Send>);
    fn set_clipboard(&self, text: &str) -> Result<(), String>;
}
```

Covers `highlight.rs:75,81`, `view.rs:56,69,99,119,326`, `mod.rs:575`, `render.rs:496`, and
the clipboard path. The floem impl lives next to the `View` impl. `arboard` stays a direct
dependency of the floem side, so the fallback at `mod.rs:230` is unaffected.

### 31b — `RendererEvent` outbound (mechanical, 19 sites)

Replace the 19 `.set()` calls with `host.emit(RendererEvent::…)`:

```rust
pub enum RendererEvent {
    PositionChanged { doc: usize, glyph: Option<u32> },
    CfiChanged(Option<String>),
    TitleChanged(Option<String>),
    TocChanged(Vec<TocEntry>),
    MatchInfo { current: usize, total: usize },
    HistoryAvailability { back: bool, forward: bool },
    /* … one variant per outbound signal */
}
```

The floem adapter holds all 21 `RwSignal`s exactly as they are today and `set()`s the matching
one. **`UI/` does not change.**

### 31c — `RendererCommand` inbound (touches `UI/`, do last)

Replace the 14 `get_untracked()` polls with an explicit entry point:

```rust
pub fn apply(&mut self, cmd: RendererCommand);
```

covering navigation (`nav_request_signal`, `nav_anchor_signal`, `history_nav_signal`,
`position_update`, `cfi_position`), settings (`current_column_width`, `current_font_size`,
`current_scale`), and search (`search_active`,
`search_query_pending`, `search_nav`). The floem adapter keeps its `create_effect`s and
translates each into an `apply` call.

This is the only sub-step that changes `UI/`'s wiring, which is why it goes last — 31a and 31b
are pure internal refactors.

## Decision: `view.rs` stays floem-native

Do **not** abstract `Event`, `Style`, `PaintCx`, or `Decorators`. `view.rs` (328 lines) is
already exactly the right shape — a `View` impl and nothing else — and becomes the entire
shim. Translate floem events into an internal `InputEvent` enum at that boundary and stop
there.

## Acceptance

- After 31a+31b: `grep -rn 'RwSignal' HtmlRenderer/src/renderer` hits only the adapter module.
- After 31c: no `get_untracked()` remains in renderer logic; `UI/` compiles against `apply`
  + an event callback.
- Reader behavior (nav, search, selection, settings) unchanged at each sub-step — verify by
  running the app, not just tests.
