# Segment Parser Phrase Grammar

The parser runs as three layers over immutable `SegmentTree` token units:

1. Token units from `SegmentTree`.
2. Parser-internal `Phrase` AST values.
3. Semantic `Detected` entities materialized into `Node`s.

Recognizers must return `Phrase`; only `Phrase::resolve` creates `Detected`.

## Phrase Families

```text
Phrase =
    NumberPhrase
  | DatePhrase
  | TimePhrase
  | MeasurementPhrase
  | CurrencyPhrase
  | WebPhrase
  | NonTranslatablePhrase
  | WordPhrase
```

## Core Numeric Grammar

```text
NumberPhrase =
    Sign? NumericCore

NumericCore =
    Integer
  | GroupedInteger
  | Decimal
  | GroupedDecimal
```

Number parsing is language-profile dependent through `NumberParsingPolicy`.
Normalized numeric payloads use nano-scale fixed-point values.

## Measurement Grammar

```text
MeasurementPhrase =
    NumberPhrase MeasurementUnitPhrase
  | NumberPhrase RangeSeparator NumberPhrase MeasurementUnitPhrase
  | NumberPhrase DimensionSeparator NumberPhrase MeasurementUnitPhrase?

RangeSeparator =
    "-" | "–" | "—"

DimensionSeparator =
    "x" | "X" | "×"
```

Whitespace-separated measurement units must be unambiguous or grounded by
known counterpart units. Glued units may use the broader unit table.

## Currency Grammar

```text
CurrencyPhrase =
    CurrencyUnitPhrase Whitespace* NumberPhrase
  | NumberPhrase Whitespace* CurrencyUnitPhrase
```

Currency words that are ambiguous with measurements are not accepted as bare
currency units.

## Date Grammar

```text
DatePhrase =
    Year Sep Month Sep Day
  | Day Sep Month Sep Year
  | MonthName Whitespace+ Day ","? Whitespace+ Year
  | Day "."? Whitespace+ MonthName Whitespace+ Year
```

Numeric date field preference is language-profile dependent.

## Time Grammar

```text
TimePhrase =
    Hour ":" Minute (":" Second)?
  | Hour Whitespace* Meridiem
```

Meridiem time is currently English-profile only and resolves to 24-hour time.

## Candidate Selection

`PHRASE_RULES` parses candidates at a token index. Scan chooses one by:

1. Phrase priority.
2. Phrase confidence.
3. Phrase ambiguity class.
4. Span length.
5. Stable rule order.

The selected phrase is resolved to `Detected` and materialized into `Node`s.
`parse_trace` exposes the candidates, their scores, and the selected rule for
parser diagnostics. Scans nested in a bracket context cannot select a phrase
whose unit range reaches beyond that context's closing unit.
