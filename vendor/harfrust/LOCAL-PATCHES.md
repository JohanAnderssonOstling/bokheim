# Local font allocation patches

Upstream: Harfrust 0.13.3, https://github.com/harfbuzz/harfrust.
License: MIT; the upstream license is included.

- Reserve the declared OpenType lookup subtable count exactly. Each entry is large, and most lookups have fewer than the geometric minimum of four entries. This avoids over-allocation and the subsequent shrink allocation for valid tables. Invalid or unsupported subtables still use the existing filtering and final shrink behavior.
- Add `GlyphBuffer::clear_with_capacity` so callers can reduce oversized glyph information and position buffers before reusing them. A safe byte-slice callback lets the caller discard the initialized storage that is being released; Harfrust keeps its unsafe-code prohibition. The existing clear method retains its original behavior.
- Retain upstream shaping, cache, feature flags, and production sources.

- Box the two large contextual lookup-cache variants so unrelated OpenType subtable records do not carry their inline storage. Contextual cache behavior and capacities stay the same.

- Expose UnicodeBuffer spare storage through a raw-pointer visitor so the caller can release unused physical pages at batch boundaries without changing capacity or touching initialized entries. Harfrust adds no unsafe code.

- Resolve subtable application functions from the existing kind, sharing the dispatch metadata instead of retaining two function pointers in every subtable record. Cached and uncached functions and all kind mappings are preserved.
