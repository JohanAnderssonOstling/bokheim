# Local SVG shaping patch

Upstream: usvg 0.46.0, https://github.com/linebender/resvg.
Licenses: Apache-2.0 OR MIT; both license files are included.

- Replace Rustybuzz with the workspace-pinned Harfrust 0.13.3, shared with Cosmic Text and Formulary.
- Keep the existing bidi runs, small-caps and kerning features, glyph clusters, and font-unit positions. Explicitly infer buffer segment properties before shaping, matching Rustybuzz's former automatic step.
- Load the same font collection face index, and reuse the Harfrust shaper across bidi runs for each font invocation.
- Depend directly on ttf-parser 0.25 for the existing glyph outlines, metrics, bitmap and color glyph handling; those implementations are unchanged.
- Keep the upstream production library, CLI, documentation, and feature flags. Harfrust and ttf-parser remain optional behind the text feature. Omit the upstream development dependency; no test code is added.
- Cargo.toml.orig retains the original upstream manifest for reference.
