# Segment rules

General text recognition extracted from `segment-parser`. The crate owns the
existing English, German, Swedish, and shared abbreviation catalogs, their case
and context policies, month catalogs, and character classes. It depends only on `segment-lexer`,
whose tokens and byte spans are re-exported for consumers.

`abbreviations` provides catalog lookup and context recognition over plain
UTF-8 text. `contains` checks catalog membership; `recognizes` also checks the
context evidence for a candidate byte span. The original parser's normalized
lookup variants remain available for callers that already compute lowercase
spellings. Rule metadata is available through read-only accessors.

```rust
use segment_rules::abbreviations::ENGLISH_POLICY;

let listed = ENGLISH_POLICY.contains("Gen.");
let recognized = ENGLISH_POLICY.recognizes("Gen. Smith", 0..4);
```

`segment-parser` supplies the segment-tree adapter and retains its public entry
points. Language profiles use these shared policies, and the parser's
`char_class` module re-exports the character classes for compatibility.

`months` supplies the existing English, German, and Swedish month spellings and
their month numbers. The parser keeps its existing locale-specific lookup and
date interpretation; consumers can reuse the catalogs without segment trees.

Consumers own rendering, pronunciation, and any transformation of recognized
text. This crate has no dependency on segment trees, dictionaries, UI, or audio.
