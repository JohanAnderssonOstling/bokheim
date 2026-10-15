# Segment parser

Semantic recognition over `structure::SegmentTree`, using the shared catalogs
and plain-text rules from `segment-rules`. Parsing retains original text and
byte spans; consumers own rendering and any speech-specific transformations.

## Core-only consumption

Disable default features to use recognition without terminology matching,
stemming, dictionary annotation, text analysis, or localization:

```toml
segment-parser = { path = "../segment-parser", default-features = false }
```

```rust
use segment_parser::{measurement_facts, parse_text_with_language};

let tree = parse_text_with_language("The package weighs 2.5 kg.", "en-GB");
for measurement in measurement_facts(&tree) {
    let original_span = measurement.span();
    let original_text = measurement.text();
}
```

The core includes parsing entry points, locale handling, abbreviation recognition,
numbers, ranges, measurements, currencies, dates, times, ordinals, versions,
URLs/emails, placeholders, and their typed facts. It still uses ICU case folding
and Unicode normalization for existing locale and measurement recognition.
Disabling optional features does not change the core recognizers' behavior.

`measurement_unit_word_forms(unit, language)` exposes existing localized unit
vocabulary in catalog order, including SI prefixes and cataloged compound forms.
It returns spelling variants without assigning grammatical number or changing
units. `NUMBER_VALUE_SCALE` documents the fixed-point scale of numeric fact
values. These general APIs let consumers use the existing vocabulary and values
without duplicating recognition catalogs.

## Optional capabilities

| Feature | Capability | Additional dependencies |
| --- | --- | --- |
| `terminology` | Term matching, coverage, term overlays, and transferring semantic patterns between runs | Aho-Corasick |
| `stemming` | Stemming and stopword helpers | Rust stemmers |
| `dictionary` | Dictionary lookup interface, caching, and annotation | None |
| `hunspell` | Bundled Hunspell implementation; enables `dictionary` | Hunspell and bundled dictionaries |
| `text-analysis` | Content-word projections, search/source maps, completion helpers, part-of-speech annotation, consistency, and semantic comparison | None beyond the core |
| `localization` | Numeric, measurement, date, and time localization | None beyond the core |
| `analysis` | Compatibility bundle containing all optional capabilities above except `hunspell` | Combined dependencies |

Default features are `analysis` and `hunspell`, preserving the previous full API.
Public function paths remain unchanged when their feature is enabled. Consumers
that previously disabled defaults to avoid Hunspell can request `analysis` for
the previous API or select only the features they use:

```toml
segment-parser = { path = "../segment-parser", default-features = false, features = ["terminology", "dictionary"] }
```

Dictionary annotation and Hunspell do not require terminology or stemming.
Features are additive across a Cargo dependency graph: another consumer enabling
a feature in the same build makes that capability available to the shared crate.

Text normalization used by the core lives separately from optional analysis
helpers. Generic word-boundary helpers are shared by dictionary and terminology
code. These boundaries keep optional capabilities independent without changing
recognition or adding consumer-specific rules to this crate.
