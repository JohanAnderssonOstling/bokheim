# Plan 04 — `calc()` and non-px/em/rem units

## Goal
Stop silently producing wrong values for `calc()` and for length units other
than px/em/rem/pt. Today these fall back to font-size or `auto`/`0`, which looks
plausible but is wrong.

## Current behavior (the bug)
In `src/style/resolver.rs`:
- `length_to_px` (`:921`) handles `Px/Em/Rem/Pt` and has a catch-all
  `_ => parent_font_size`, so `vw/vh/vmin/vmax/ch/ex/cm/mm/in/q` all silently
  become the font size.
- Every `LengthPercentage::Calc(_)` arm bails to a fallback:
  `length_percentage_to_px:935` → font size, `length_percentage_to_lengthpct:956`
  → `Px(0.0)`, `size_to_preferred:966` / `max_size_to_preferred:978` → `Auto`,
  `border_width_to_px:1010` → medium, `spacing`/`length_from_length` → `0`.

## Design
Split into three independently shippable pieces, easiest first.

### 4a — absolute + font-relative units (cheap, do first)
Extend `length_to_px` to cover the units resolvable at cascade time:
- Absolute: `cm` (×96/2.54), `mm` (×96/25.4), `in` (×96), `pc` (×16), `q`
  (×96/2540/... ) — lightningcss `LengthValue` already has these variants.
- Font-relative approximations: `ch ≈ 0.5 × font_size` (advance of `0`),
  `ex ≈ 0.5 × font_size` (x-height). Exact values need font metrics; the 0.5
  approximation matches what most engines use as a fallback and is far better
  than "= font_size".
- Replace the `_ => parent_font_size` catch-all with `_ => 0.0` plus a one-time
  `log_unhandled` so genuinely unknown units fail visibly small rather than
  silently large.

### 4b — static `calc()` evaluation (medium)
Evaluate `calc()` expressions composed of absolute + font-relative lengths and
numbers to a single px value at cascade time. lightningcss exposes the calc AST
(`lightningcss::values::calc::Calc`). Implement
`fn eval_calc_px(calc, font_size, root_font_size) -> Option<f32>`:
- `Calc::Value(LengthValue)` → `length_to_px`.
- `Calc::Number(n)` → n (unitless; only valid as a multiplier).
- `Calc::Sum/Product` → recurse; product requires one side unitless.
- Percentages or viewport units inside calc → `None` (defer to 4c).
Wire it into every `Calc(_)` arm: if `eval_calc_px` returns `Some`, use it; else
keep the current fallback. This covers the common `calc(100% - 2em)`-minus-fixed
patterns only partially (the % part returns None) — acceptable for v1.

### 4c — viewport units `vw/vh/vmin/vmax` (larger, follow-up)
These need the viewport, which isn't known at cascade time — same situation as
percentage margins (already deferred via `LengthPct::resolve(cb_width)`).
- Extend `LengthPct` with `Vw(f32)`/`Vh(f32)` (and `PreferredSize` likewise) and
  thread a `Viewport { width, height }` into `resolve()` and the layout calls
  that currently pass only `cb_width`.
- Resolve against the column/viewport box at layout time.
Only do this if real EPUB content in the corpus uses viewport units; otherwise
4a's small fallback is enough.

## Steps
1. `length_to_px`: add the absolute + ch/ex arms; change catch-all to `0.0` +
   log. (4a)
2. Add `eval_calc_px`; replace the `Calc(_)` fallbacks in
   `length_percentage_to_px`, `length_percentage_to_lengthpct`,
   `size_to_preferred`, `max_size_to_preferred`, `border_width_to_px`,
   `length_to_px_from_length`, `spacing_to_px`, and the `VerticalAlign::Calc`
   arm (`:439`). (4b)
3. (Deferred) `LengthPct`/`PreferredSize` viewport variants + threading. (4c)

## Tests
- `width: calc(50px + 1em)` at 16px font → `Px(66)`.
- `margin-left: 2cm` → `Px(75.6)`.
- `font-size: 2ch`-derived width sanity (≈ font_size).
- Unknown/`vw` (pre-4c) → small/zero, logged once, not = font size.

## Effort / risk
4a trivial, 4b medium (calc AST recursion + unit bookkeeping), 4c larger and
should be gated on real need. Risk is low and contained to resolver helpers; no
layout changes for 4a/4b.
