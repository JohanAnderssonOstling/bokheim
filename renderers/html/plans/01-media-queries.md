# Plan 01 — Honor `@media` / `@supports` (and detect `@import`)

## Goal
CSS rules nested inside `@media`/`@supports` blocks currently never apply. Make
the cascade descend into those at-rules so their style rules participate in
selector matching and the cascade.

## Current behavior (the bug)
- `SelectorIndex::from_stylesheets` (`src/style/selectors.rs:83`) iterates only
  `stylesheet.rules.0` at the top level and indexes `CssRule::Style` exclusively
  (`:88`). Anything inside `CssRule::Media`, `CssRule::Supports`, etc. is never
  indexed, so it never becomes a candidate.
- `RuleKey { stylesheet_idx, rule_idx, source_order }` (`:56`) addresses a rule
  by a single flat index into `rules.0`.
- `stylesheet_rule` (`src/style/resolver.rs:1085`) re-fetches the rule via
  `stylesheets.get(key.stylesheet_idx)?.rules.0.get(key.rule_idx)` — also
  top-level only.

Net effect: a book whose base typography lives under `@media all { … }` or a
reading-system media query renders unstyled, silently.

## Design
Make rule addressing **path-based** and **evaluate media conditions once at
index-build time** against a fixed EPUB media environment, indexing only rules
whose enclosing conditions pass. This keeps per-element matching unchanged and
costs nothing at match time.

### v1 scope (pragmatic)
- Descend into `CssRule::Media` and `CssRule::Supports`.
- Evaluate the condition **permissively**: index nested rules unless the query
  clearly excludes a reflowable screen reader (e.g. media type `print`,
  `speech`; an `@supports` for an obviously unsupported feature can still be
  indexed — being permissive only over-applies rarely and is safer than
  dropping styling).
- Detect `CssRule::Import` and log once (`eprintln!`) — actually fetching the
  imported sheet via the `ResourceProvider` is out of scope here (see Follow-up).

### v2 (follow-up, only if needed)
- Real media-feature evaluation (`width`/`max-width`/`min-width`) against the
  current column width, with a re-resolve when the column width crosses a
  query boundary. This couples style resolution to viewport width, so gate it
  behind the existing column-width change path.

## Steps
1. **`selectors.rs` — path-based `RuleKey`.**
   - Change `rule_idx: usize` → `rule_path: smallvec::SmallVec<[u16; 3]>` (or
     `Vec<u16>`; paths are 1–2 deep in practice).
   - In `from_stylesheets`, replace the flat loop with a recursive
     `fn index_rules(&mut self, rules: &CssRuleList, stylesheet_idx, path: &mut Vec<u16>, source_order: &mut usize)`:
     - `CssRule::Style(s)` → build `RuleKey { stylesheet_idx, rule_path: path+idx, source_order }`, `add_selector` for each selector, bump `source_order`.
     - `CssRule::Media(m)` if `media_condition_applies(&m.query)` → recurse into `m.rules`.
     - `CssRule::Supports(s)` if `supports_applies(&s.condition)` → recurse into `s.rules`.
     - `CssRule::Import(_)` → record-for-logging.
   - Add `fn media_condition_applies(query: &MediaList) -> bool` (v1: false only
     for explicitly non-screen media types; true otherwise).
2. **`resolver.rs` — path-based fetch.**
   - Rewrite `stylesheet_rule` to walk the path: start at
     `stylesheets[stylesheet_idx].rules`, for each segment except the last
     descend into the nested rule's child `.rules` (`CssRule::Media(m) => &m.rules`,
     `CssRule::Supports(s) => &s.rules`), then `get(last)` and expect
     `CssRule::Style`.
   - `RuleKey` is used as a `HashMap`/`HashSet` key (`MatchedRule`, `seen`); keep
     it `Eq + Hash` (derive still works for the path field).
3. **Source order.** `source_order` must increase in document order across the
   *flattened* sequence (so cascade ties break correctly). The recursive walk
   already yields that if incremented at each `Style` rule in traversal order.

## Edge cases
- Nested `@media` inside `@media` — handled by recursion.
- Empty media blocks — no rules indexed, fine.
- A selector that appears both top-level and inside `@media` — both get distinct
  `RuleKey`s and distinct `source_order`s; later one wins on a tie. Correct.

## Tests (`#[cfg(test)]` in resolver or a new `tests/media.rs`)
- `p { color:black } @media all { p { color:red } }` → `p` resolves red.
- `@media print { p { color:green } }` → `p` does **not** get green (v1
  excludes print).
- `@supports (display:flex) { .x { color:blue } }` → `.x` blue (permissive).
- Nested `@media all { @media all { h1 { font-weight:700 } } }` applies.

## Effort / risk
Small–medium. Mechanical change to two functions plus a recursion helper. Main
risk is the `RuleKey` type change rippling through `MatchedRule` sorting and the
`seen` set — both only require the key stay `Ord`/`Eq`/`Hash`.

## Follow-up (separate work)
- `@import` fetching through `ResourceProvider` (resolve relative URL, parse,
  splice into the stylesheet list in source order).
- v2 width-based media evaluation tied to column width.
