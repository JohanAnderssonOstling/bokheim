# Plan 32: Split HtmlRenderer into crates

Final step of the HtmlRenderer decoupling program (29 → 30 → 31 → 32). Do it **after** 29–31:
those plans do the design work, so this one is mostly `git mv` plus manifest edits. Splitting
first would turn it into a redesign.

## Why

~16.9k LOC in one crate. Payoffs: layout and style become testable and benchmarkable without a
windowing stack, the layering is enforced by the compiler instead of by convention, and
touching `renderer/` stops rebuilding 14k lines.

## Measured dependency graph

Production `crate::` edges today (test-only edges excluded — see step 5):

```
L5  html-renderer      view.rs, painter impl, glyph cache, signal adapter   [floem]
L4  html-view-core     mod.rs, nav, selection, highlight, render, load      ~2.6k
L3  html-pipeline      DocumentFactory                                      ~0.3k
L2  html-layout ~4.4k    html-style ~2.6k    html-parse ~0.2k    html-resources ~0.3k
L1  html-dom ~2.5k
L0  kurbo · peniko · rustc_data_structures · epub_provider
```

| Crate | Depends on | Evidence |
|---|---|---|
| `html-dom` | kurbo, peniko, rustc_data_structures | leaf |
| `html-style` | html-dom, lightningcss, parcel_selectors | 23 `crate::document` refs |
| `html-parse` | scraper **only** | `parse_html_document` (`parser/html.rs:274`) normalizes + parses, returns `scraper::Html`, never touches DOM types |
| `html-resources` | html-dom, epub_provider, image, peniko | one edge: `resources/images.rs:9` |
| `html-layout` | html-dom **only** (after plan 29) | 45 `crate::document` refs; `crate::text` gone |
| `html-pipeline` | dom, style, parse, layout | `build_pipeline_from_parsed_impl` (`parser/html.rs:62`) orchestrates DOM-build → `resolve_styles_for_dom` → `build_layout_inputs` |
| `html-view-core` | all of the above | no floem |
| `html-renderer` | + floem, floem_renderer, arboard | façade |

Note `parser/` splits in two: the scraper wrapper goes to L2, `DocumentFactory` to L3.

## Decisions

1. **Depend on `kurbo` and `peniko` directly**, not via floem re-exports. Outside `renderer/`,
   floem is used only as a re-export shim (`floem::kurbo::{Point,Size,Rect,Vec2}`,
   `floem::peniko::{Color,Blob,Image,Format}`). Pin the versions floem 0.2.0 already resolves:
   **kurbo 0.11.3, peniko 0.2.0**. This is a mechanical rename and can land before the split.
   Stop there — kurbo and peniko are Linebender leaf crates with no windowing or backend of
   their own; hand-rolling geometry and color types would be pure cost.

2. **Unit tests stay in place via a dev-dependency cycle.** 70 tests (42 layout, 28 style)
   use `DocumentFactory`, which after the split sits *above* them
   (`layout/inline.rs:936`, `box_builder.rs:595`, `box_layout.rs:223`, `style/resolver.rs:1810`).
   They also exercise `pub(crate)` internals, so relocating them to `html-pipeline/tests/`
   would mean widening visibility or rewriting them. Cargo explicitly permits cycles through
   dev-dependencies: give `html-layout` and `html-style` a `[dev-dependencies] html-pipeline
   = { path = ... }`. The production graph stays a strict DAG. State this in the crate README
   so nobody "fixes" it: **acyclic in prod, cyclic in dev, on purpose.**

3. **`default.css` belongs to `html-style`,** exposed as `pub const DEFAULT_CSS: &str`. Two
   `include_str!` sites reach across module boundaries today (`parser/html.rs:77`,
   `layout/inline.rs:992`); after the split `include_str!` cannot cross crates.

4. **`html-renderer` is a façade.** It re-exports so `UI/Cargo.toml:14` keeps its single
   dependency line. While doing this, narrow `pub mod style` (`lib.rs`) unless `UI/` actually
   needs it — it is the only wide export today.

5. **Rename the package.** It is `Html-Prototype`, aliased `html_renderer` in `UI/`, in a
   directory called `HtmlRenderer`, with `#![allow(non_snake_case)]` at the top of `lib.rs` —
   three names for one thing. Standardize on `html-renderer` and delete the `allow`. Doing it
   later means editing seven manifests instead of one.

6. **On-disk layout:** crates live in `HtmlRenderer/crates/*`. Add them explicitly to
   `[workspace] members` (`HtmlRenderer` is currently only an implicit member via `UI/`'s path
   dep) and hoist `kurbo`, `peniko`, `lightningcss`, `parcel_selectors`, and `scraper` into
   `[workspace.dependencies]` so seven manifests cannot drift.

7. **`epub_provider` does not move.** It stays under `HtmlRenderer/`; `Backend/Cargo.toml:40`
   depends on it by path and works fine. Moving it is churn with no payoff.

8. **Delete the unused `cosmic-text` dependency** from the manifest, or use it — it is
   declared at 0.12.1 and never referenced (all shaping goes through floem's text stack).
   Note floem 0.2.0 resolves the same 0.12.1, so a cosmic-text-native cache stays feasible
   later; after plan 30 nothing above the painter can see the glyph representation anyway, so
   it is not urgent.

## Order

Bottom-up, each step compiling on its own: dom → style / parse / resources → layout →
pipeline → view-core → façade.

## Acceptance

- `cargo tree -p html-view-core | grep floem` is empty. This is the whole point of making it a
  crate rather than a module: the boundary becomes impossible to violate by accident.
- `UI/` builds with an unchanged dependency list (modulo the package rename).
- All 86 tests pass; `cargo test --workspace` is green.
