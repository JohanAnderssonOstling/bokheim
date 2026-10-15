# Segment Parser Architecture Refactor Plan

## Objective

Replace the parser's overlapping structural pipelines with a grammar-driven candidate parser that has explicit semantic resolution, candidate arbitration, and a single node-materialization stage.

The refactor should preserve observable parsing behavior unless a behavior change is documented by a focused regression test. It should make formatting tags transparent to recognition, keep ambiguity decisions explainable through `ParseTrace`, and make the phrase grammar the source of truth for recognizers.

## Current State

The parser currently uses three structural mechanisms:

1. `PHRASE_RULES` produces and scores phrase candidates during a left-to-right scan.
2. Chemical formulas and placeholders replace already materialized nodes afterward.
3. Number ranges drain and regroup already materialized nodes afterward.

Recognition, semantic construction, candidate policy, and node construction are consequently mixed across recognizers. Raw `usize` values also represent both unit indices and byte offsets, while formatting-tag traversal is handled independently by several parsers.

The existing `grammar.md` describes a parser-internal `Phrase` layer, but recognizers currently return `Detection` or `Detected` directly. The implemented parser therefore does not yet follow its documented layering.

Measured baseline:

| Area | Current size |
|---|---:|
| Production parser code | 3,460 lines |
| Parser tests and shared test support | 2,553 lines |
| Entire parser directory | 6,013 lines |

## Target Architecture

```text
SegmentTree
    ↓
ParserInput and delimiter scopes
    ↓
Typed grammar recognizers
    ↓
PhraseMatch<Phrase>
    ↓ semantic validation and grounding
Candidate<ResolvedPhrase>
    ↓ overlap and precedence resolution
ParsePlan
    ↓ single materialization
Vec<Node>
    ↓ non-structural annotation
Final nodes
```

The architecture has four strict responsibility boundaries:

1. Recognizers parse grammar and return typed phrases. They do not create or mutate nodes.
2. Resolution applies locale rules, source grounding, and semantic validation. It does not select winners.
3. Candidate arbitration resolves precedence and overlap. It does not perform parsing.
4. Materialization creates nodes once. Annotation passes do not replace or regroup nodes.

There is one grammar architecture. "Generic grammar" in this plan means the shared typed recognizer pipeline above; it does not mean a second pattern-interpreter or parser-combinator subsystem beside it. Grammar families may use small data tables internally when demonstrated repetition makes them simpler, but those tables must still produce the same typed phrases and pass through the same resolution and arbitration stages.

## Core Types

### ParserInput

`ParserInput` should own the immutable inputs and derived structural context required during one parse:

- `SegmentTree`
- `ParseLocale`
- `ParseGrounding`
- delimiter scopes
- unit-to-text span mapping

It should expose focused cursor operations rather than the underlying fields.

`ParseLocale` owns regional conventions such as numeric date ordering, separators, and locale-grounded unit interpretations. Language profiles own language lexemes and membership in shared lexical catalogs, such as month names, weekday names, abbreviations, and unit words. Grammar recognizers consume both through `ParserInput`; they must not branch on raw language-code strings.

### Typed indices and spans

Introduce distinct types for unit positions and text positions:

```rust
struct UnitIndex(usize);
struct UnitSpan { start: UnitIndex, end: UnitIndex }
struct TextSpan(Range<usize>);
```

Conversions between unit spans and text spans should occur through `ParserInput`. This prevents accidental mixing of token indices and byte offsets.

### Cursor

A bounded cursor should centralize navigation and preserve the distinction between formatting adjacency and whitespace separation:

- `skip_tags`: advance across formatting tags only.
- `skip_layout`: advance across tags and whitespace.
- `next_visible` and `previous_visible`: inspect non-tag units.
- `take_kind`: consume a required unit kind.
- `take_text`: consume required punctuation or lexical text.
- `gap`: return the observed whitespace span and kind.
- `text`: return text for a validated unit span.

Formatting tags remain available for materialization but do not independently create grammatical boundaries.

### PhraseMatch

```rust
struct PhraseMatch<T> {
    units: UnitSpan,
    phrase: T,
}
```

`PhraseMatch` represents syntax only. It contains no candidate score and performs no node construction.

### Phrase

The grammar-backed phrase model should cover every structural result produced by the parser:

```rust
enum Phrase {
    Number(NumberPhrase),
    NumberRange(NumberRangePhrase),
    Measurement(MeasurementPhrase),
    Currency(CurrencyPhrase),
    Date(DatePhrase),
    Time(TimePhrase),
    Ordinal(OrdinalPhrase),
    Version(VersionPhrase),
    Protected(ProtectedPhrase),
    Word(WordPhrase),
}
```

Each variant should contain typed grammar components rather than partially constructed `NodeSemantic` values.

### ResolvedPhrase

`Phrase::resolve` should apply context-sensitive validation and produce a semantic-ready `ResolvedPhrase`. Resolution includes:

- number normalization and observed formatting
- calendar and clock validation
- locale-specific unit and currency resolution
- source grounding
- ambiguity classification
- measurement normalization
- agreement predicates such as a stated weekday matching a resolved calendar date

Resolution failures remove invalid syntax candidates before arbitration.

### Candidate

One internal candidate type should replace the overlapping responsibilities of `Detection`, `PhraseCandidate`, and their duplicated score projections:

```rust
struct Candidate {
    phrase: ResolvedPhrase,
    units: UnitSpan,
    score: CandidateScore,
    reason: &'static str,
}
```

Public trace objects should be read-only projections of this candidate rather than parallel representations maintained by recognizers.

### ParsePlan

`ParsePlan` should contain the selected, non-overlapping candidates and structural delimiter information. It is the complete immutable input to materialization.

## Grammar Model

Grammar defines valid phrase composition. It must not silently encode ambiguity precedence through ordered alternatives.

The numeric family should begin with shared primitives:

```text
NumericPhrase ::=
      Measurement
    | Currency
    | Date
    | Time
    | Ordinal
    | Version
    | NumberRange
    | Number

Measurement ::=
      Number Adjacent Unit
    | Number Gap Unit
    | Number Gap? RangeSeparator Gap? Number Gap? Unit
    | Number Gap? DimensionSeparator Gap? Number (Gap? Unit)?

Adjacent ::= Tag*
Gap      ::= Tag* Whitespace (Tag | Whitespace)*
```

All valid alternatives should be produced as candidates. Candidate policy, not grammar order, decides which interpretation wins.

Grammar structure should normally be expressed as typed functions and enums. Language-specific spellings do not define separate grammars: language profiles provide lexemes and catalog membership, while `ParseLocale` supplies regional conventions used during resolution. A data-driven sequence description is acceptable only as a private implementation detail of one grammar family when several implemented languages otherwise require duplicated control flow. It must not expose untyped capture maps or bypass `PhraseMatch`, `ResolvedPhrase`, or candidate arbitration.

For example, weekday-aware dates should follow this division:

```text
DatePhrase ::= WeekdayPrefix? DateBody
WeekdayPrefix ::= WeekdayName Punctuation? Gap
```

The date recognizer captures the optional typed weekday. Resolution validates the date and, when all fields are available, verifies that the stated weekday agrees with it. Language profiles provide weekday names; they do not implement calendar validation or node construction.

Before implementation, `grammar.md` must be expanded to include:

- number ranges
- ordinals
- versions
- URLs and email addresses
- placeholders and chemical formulas
- opaque protected phrases
- compound words and abbreviations
- delimiter scopes
- formatting-tag transparency
- attached versus separated unit syntax
- locale-provided separator catalogs
- semantic predicates such as valid calendar dates and time bounds
- optional weekday date forms and weekday/date agreement
- the actual `10^10` fixed-point scale

## Recognition Organization

Recognizers should be grouped by grammar root rather than by final node construction.

### Numeric candidates

`numeric_candidates_at` parses `NumberPhrase` once at a starting position and derives applicable extensions from that parsed value:

- numeric date
- time
- ordinal
- measurement
- measurement range
- dimension
- currency suffix
- version
- number range
- fallback number

Prefix currency remains a separate root because it starts with a currency unit, but it should reuse the same number recognizer.

### Textual candidates

`textual_candidates_at` should cover:

- textual dates
- number words
- URLs
- email addresses
- language words
- abbreviations
- compound words
- source-grounded opaque text

### Protected candidates

Protected span detectors should produce ordinary high-priority candidates:

- placeholders
- chemical formulas
- UUIDs
- IP addresses
- paths
- code identifiers
- compact alphanumeric codes
- hex colors

They must participate in the same overlap arbitration as other phrases. They must not replace nodes after materialization.

## Delimiter Scopes

Delimiter matching should run before phrase recognition and produce immutable parse scopes. Recognizers receive a bounded cursor and cannot create matches crossing the current scope.

Delimiter leaves and their annotations remain structural materialization inputs. Apostrophe disambiguation and locale-specific quotation pairs remain context-sensitive but should be resolved once during scope construction.

## Candidate Arbitration

Candidate selection must be deterministic and explicit. The current ordering dimensions should be retained initially:

1. priority
2. confidence
3. ambiguity class
4. span length
5. stable rule order

Overlap resolution should operate within delimiter scopes. Higher-priority protected candidates must be able to exclude lower-priority candidates that begin earlier but overlap their span.

The first implementation should preserve current outcomes and trace ordering. Any later scoring changes should be isolated from grammar changes and covered by conflict tests.

## Materialization and Annotation

Materialization should consume `ParsePlan` once and create the complete node sequence in text order. It should:

- emit selected semantic and lexical phrase nodes
- emit unmatched lexical units as leaves
- preserve formatting tags as children or neighboring leaves according to their spans
- attach delimiter structure
- preserve stable text ordering and non-overlap invariants

After materialization, annotation passes may add facts that do not alter node ownership or hierarchy:

- mathematical-sign annotations
- punctuation typography
- whitespace typography
- delimiter annotations that depend on materialized context

The following structural mutation mechanisms should be removed:

- `wrap_semantic_span`
- post-materialization placeholder wrapping
- post-materialization chemical-formula wrapping
- `wrap_number_ranges`
- node draining and reinsertion for semantic recognition

## Migration Plan

### Stage 1: Lock behavior and update the grammar

- Expand `grammar.md` to describe every existing phrase family.
- Add focused precedence tests for ambiguous inputs.
- Add formatting-tag invariance tests for every numeric family.
- Record representative `ParseTrace` output for conflict cases.
- Document intentional existing inconsistencies before deciding whether to preserve or fix them.
- Document the boundary between language lexemes, locale conventions, typed grammar, and semantic predicates.

Completion condition: the documented grammar and policy cover every entry in `PHRASE_RULES` and every post-materialization structural detector.

### Stage 2: Introduce ParserInput, typed spans, and Cursor

- Add `UnitIndex`, `UnitSpan`, and `TextSpan`.
- Add bounded cursor navigation.
- Build delimiter scopes through `ParserInput`.
- Migrate existing recognizers one family at a time without changing their return types.
- Remove superseded raw-index navigation helpers after the last caller migrates.

Completion condition: recognizers no longer perform independent tag/layout traversal, and boundary behavior is formatting-invariant.

### Stage 3: Convert the numeric vertical slice

- Implement `NumberPhrase` and shared number recognition.
- Implement typed numeric extensions for measurement, range, dimension, currency, date, time, ordinal, and version.
- Introduce `PhraseMatch`, `Phrase`, `ResolvedPhrase`, and the unified internal candidate.
- Keep an adapter that converts new candidates into the existing scan pipeline during migration.
- Represent textual and numeric date alternatives, including optional weekdays, as typed `DatePhrase` variants resolved by the same calendar logic.

Completion condition: numeric syntax is parsed once per starting position and all numeric-family tests pass through the new recognizers.

### Stage 4: Convert textual and protected recognizers

- Convert word, abbreviation, compound, URL, email, and opaque recognizers.
- Convert formula and placeholder detectors into protected candidate producers.
- Map character-based detector spans to validated unit and text span types once.

Completion condition: every structural recognizer returns a typed phrase candidate and no recognizer constructs a `Node`.

### Stage 5: Introduce unified overlap resolution

- Build a candidate index per delimiter scope.
- Implement deterministic precedence and overlap selection.
- Project `ParseTrace` from the unified candidate set.
- Compare selected candidates and traces against the compatibility tests.

Completion condition: all structural precedence is decided by one component.

### Stage 6: Materialize once

- Introduce `ParsePlan`.
- Materialize selected phrases, delimiters, leaves, and tags in one pass.
- Convert mathematical signs and typography to annotation-only passes.
- Remove semantic wrapping, node draining, and node reinsertion.

Completion condition: materialization is the only code that creates parser output nodes, and later passes cannot change node structure.

### Stage 7: Remove compatibility layers and streamline modules

- Remove `Detected` and adapters retained during migration.
- Collapse duplicated candidate and trace metadata.
- Move measurement-unit scanning into the unit phrase subsystem.
- Store `ParseGrounding` directly in `ParserInput`.
- Split tests by grammar family and policy conflict rather than legacy source file.
- Remove unreachable defensive branches and obsolete helpers.

Completion condition: no old pipeline remains and the production parser has one documented control flow.

## Testing Strategy

Every migration stage must keep the complete `segment-parser` test suite passing.

Required focused coverage:

- equivalent results with and without formatting tags at every legal boundary
- all locale decimal and grouping policies
- all locale range separators
- attached, separated, grounded, and ambiguous measurement units
- date/version/number conflicts
- language-specific weekday spellings and weekday/date disagreement
- time/number conflicts
- range/minus conflicts
- currency/measurement conflicts
- URL/email/version/opaque conflicts
- formulas and placeholders overlapping lower-priority phrases
- delimiter scope boundaries
- candidate trace stability
- ordered, non-overlapping materialized nodes

Tests should assert typed semantic results and selected candidate kinds. Large debug dumps should be reserved for failure messages.

## Invariants

The new parser should maintain these invariants:

- Recognition never mutates `SegmentTree` or nodes.
- A candidate is contained by exactly one delimiter scope.
- Candidate unit and text spans are non-empty and mutually consistent.
- Selected structural candidates never overlap in visible text.
- Formatting tags do not change semantic recognition.
- Every selected phrase has one deterministic explanation and score.
- Materialization preserves source text order.
- Annotation cannot change node spans, ownership, or hierarchy.
- Locale and grounding access occurs through `ParserInput`.

## Estimated Simplification

Expected steady-state result:

| Area | Current | Estimated target |
|---|---:|---:|
| Production parser code | 3,460 lines | 2,750–2,950 lines |
| Parser infrastructure and orchestration | approximately 1,100 lines | approximately 650–800 lines |
| Parser tests | 2,553 lines | 2,650–2,850 lines |
| Entire parser directory | 6,013 lines | approximately 5,400–5,800 lines |

This represents an estimated 15–20% production-line reduction and a 35–50% reduction in architectural complexity. Test code may increase because grammar productions and precedence conflicts will receive more focused coverage.

During migration, the parser may temporarily grow by 10–15% while adapters allow old and new recognizers to coexist. Each stage should remove its compatibility layer before the following broad conversion begins where practical.

## Risks and Controls

### Precedence changes

Moving post-processing detectors into candidate arbitration may expose previously implicit precedence. Preserve behavior through conflict tests and trace comparisons before changing policy.

### Span and tag regressions

Typed spans and cursors change pervasive navigation code. Migrate incrementally and add formatting-invariance tests before moving each grammar family.

### Excessive abstraction

Do not introduce a general parser-combinator framework, dynamic recognizer traits, a top-level pattern interpreter, or generated grammar unless concrete repetition in implemented grammar families justifies them. Reassess small family-local pattern tables only after additional languages expose duplicated recognizer control flow. Prefer typed functions and enums for the current grammar size.

### Transitional duplication

Adapters are acceptable only while a grammar family is migrating. Mark each adapter with the stage that removes it and do not build new behavior on compatibility types.

### Performance

Parse shared primitives once per starting position. Candidate collection should be bounded by delimiter scope and indexed by unit start. Add representative parser benchmarks before changing the selection algorithm.

## Completion Criteria

The architecture refactor is complete when:

- `grammar.md` describes all implemented structural recognition.
- Every recognizer returns a typed phrase match.
- Locale and source grounding are applied during one resolution stage.
- All structural candidates use one scoring and overlap resolver.
- `ParseTrace` is projected from the unified candidates.
- Nodes are created in one materialization stage.
- Later passes only add annotations.
- Formatting tags are semantically transparent.
- Raw unit indices and byte offsets are not interchangeable in parser APIs.
- The old `Detected` pipeline and structural wrapping passes are removed.
- All parser tests and compatibility tests pass.
- No new external dependency is required.
