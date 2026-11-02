# Local font-stack patch

Upstream: cosmic-text 0.19.0, https://github.com/pop-os/cosmic-text.
Licenses: MIT OR Apache-2.0; both license files are included.

- Inherit Skrifa 0.46.2 and Harfrust 0.13.3 from the application workspace. This shares font parsing and shaping with Formulary and the patched Swash dependency.
- Adapt shaping to Harfrust's `ShapeOptions` API, retaining the cached shape plan and requested OpenType features. Default scaling still returns positions in font units.
- Forward the `no_std` feature to Harfrust's `libm` feature for its floating-point operations.
- Borrow font features in attribute views instead of cloning their vector on glyph lookups; owned styles still retain their own feature lists.
- Reuse converted Harfrust feature storage between shaping calls, and grow shaping spans by actual directional runs rather than reserving one slot per input byte.
- Drop unused directional spans at the end of a rebuild while retaining the span-cache vector capacity. Their glyph and word buffers were already discarded before reuse at the next rebuild. On Linux, discard whole unused pages in buffers of at least 16 KiB before freeing them, so allocator free space need not remain resident.
- Clear temporary visual-line ranges after producing owned layout output. Keep 64 KiB of range-vector capacity warm across layout scratch lines and advise whole unused pages in larger buffers on Linux. Allocations remain available for reuse, so outlier paragraphs need not rebuild large vectors; ordinary paragraphs retain their warm scratch storage.
- Traverse span and word index ranges directly in layout, using their forward or backward ends as needed. This avoids temporary index vectors while preserving bidi ordering and partial-word ellipsizing boundaries.
- Apply the existing L1 whitespace adjustment once to the owned bidi level array, avoiding a full copy and repeated full-text scans per paragraph. Auto-detected base levels are 0 or 1, and the existing same-direction requirement means supported paragraphs use the same adjustment level.
- Shape provably pure left-to-right Unicode paragraphs directly at level zero without allocating bidi class, processing-class or level arrays. Match unicode-bidi's pure-LTR condition conservatively: RTL characters, Arabic-number classes, explicit embeddings/overrides and isolate controls retain the full bidi path, including unmatched PDF/PDI controls. Keep ASCII classification allocation-free and preserve all B-class paragraph separators: LF, CR, U+001C–U+001E, U+0085 and U+2029, including consecutive and terminal separators.
- Store shaped glyphs in one reusable buffer per directional span. Words retain ranges into that buffer, avoiding a separate allocation and minimum-capacity waste for each word and whitespace character. Wrapping and RTL processing still use the same word boundaries.
- Reuse one temporary fallback glyph vector while trying faces for a run, then release it when the run ends. No fallback result cache is retained.
- Clear reusable Harfrust buffers with a capacity limit of 4096 glyph entries per vector, reducing memory retained after unusually long shaping runs. Smaller buffers keep their allocation. On Linux, release whole unused pages within the owned glyph storage before shrinking, so freed heap space can become nonresident without a global allocator trim.
- Store per-glyph monospace fallback widths only when the `monospace_fallback` feature is enabled. Other builds omit the always-empty metric, reducing glyph-buffer allocation size while preserving their layout behavior.
- Retain the remaining upstream production sources and feature flags. The vendored subset omits external tests, benchmarks, and their development dependencies.
- `Cargo.toml.orig` retains the original upstream manifest for reference.

- Release unused Harfrust buffer pages at explicit shaping batch boundaries, including the reusable prefix left after an oversized word. Keep capacities and cached shape plans, and preserve warm pages between shaping calls.

- Expose explicit no-ellipsis line construction and rebuild methods for callers that lay out with `Ellipsize::None`. GPUI uses this path to skip unused ellipsis shaping, temporary glyph/style storage, and fallback font work. The normal constructors and rebuilds still prepare ellipses.
- Keep the closest face in the requested family when a static font lacks an exact weight. Intermediate reader weights must not discard the selected family and fall through to a different fallback font.
