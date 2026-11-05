# Local WGPU patch

Upstream: crates.io `wgpu` 29.0.4, MIT OR Apache-2.0.

`src/backend/webgpu.rs` uses `Closure::once_aborting` and
`Closure::wrap_aborting` for JavaScript callback boundaries. The upstream
callbacks capture interior mutable state or user callbacks without an
`UnwindSafe` contract. Explicit aborting constructors preserve their previous
behavior while allowing the rest of the web application to use `panic=unwind`.
Panics caught inside Rust operations still unwind normally before reaching
these boundaries. No unwind-safety assertion is added to WGPU's state.

Keep the patch until upstream offers a compatible callback policy, then remove
the `wgpu` override in the workspace's `[patch.crates-io]` table.
