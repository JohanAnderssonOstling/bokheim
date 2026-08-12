# SVG rendering plan

Render SVG in EPUB content natively through floem's vello backend, reusing the
existing replaced-element (image) plumbing for registration, layout, and the
paint loop. No manual rasterization.

## Decisions (locked)

- **Paint via `floem_renderer::Renderer::draw_svg`.** The vello backend renders a
  `usvg::Tree` as a true vector scene (`vello_svg::render_tree`), so output is
  crisp at any zoom/font-scale. No resvg rasterization.
- **No `SoftwarePainter` support.** The software/PNG path is out of scope; its
  `draw_svg` is a documented no-op. (PNG export will not contain SVG.)
- **No new dependencies.** `usvg` is reached via the re-export
  `floem_renderer::usvg` (pinned 0.43 through floem). `resvg`/`tiny_skia` are not
  used.
- **Reuse the image pipeline.** SVG is a replaced element; it flows through the
  existing `image_idx` / `ImageResource` / `InlineKind::Image` / prefetch-window
  machinery. Only *decode* and *paint* branch on SVG-vs-raster.
- **currentColor: 90% coverage via injected stylesheet (see §5).** Full document
  CSS cascade onto SVG paths is explicitly out of scope (documented limitation).

## Cases covered

1. **SVG-wrapped raster cover** — `<svg viewBox=…><image xlink:href="cover.jpg"/></svg>`.
2. **Referenced vector SVG** — `<img src="diagram.svg">`, `<image href="x.svg">`.
3. **Inline vector SVG** — `<svg>…<path/>…</svg>` directly in the XHTML.

Cases 1 and 2 are already registered as `ImageResource`s by
`register_image_resource` (`parser/html.rs:269`, reads `src`/`href`/`xlink:href`);
they fail today only because `decode_image` can't read SVG. Case 3 needs new
registration.

---

## 1. Data model

`document/resources.rs`
- Add `ImageSource::Inline(Arc<[u8]>)` alongside `Uri(String)` for serialized
  inline SVG. `load_bytes` (`resources/images.rs:188`) returns these bytes
  directly with no provider round-trip.
- `ImageResource`: add a lightweight format hint, e.g.
  `kind: ResourceKind { Raster, Svg }`, set at registration when the
  source/extension is known, otherwise resolved by content sniff at decode.

`resources/images.rs`
- Turn `DecodedImage` into an enum:
  ```
  enum DecodedImage {
      Raster { image: PenikoImage, hash: Vec<u8>, byte_len: usize }, // unchanged
      Svg { tree: usvg::Tree, hash: Vec<u8> },
  }
  ```
  `get_decoded` and the cache (`decoded_cache`, prefetch window, `ensure_window`)
  are unchanged — they key on `image_idx` and don't care about the variant.

---

## 2. Decode path & threading

`usvg::Tree` (0.43) is **not `Send`** (uses `Rc` internally), so it cannot cross
the `decode_worker` channel. Therefore:

- `decode_worker` / `decode_image` stay **raster-only** (unchanged).
- `bytes_worker` still loads SVG bytes (real I/O, worth backgrounding).
- **SVG bytes are parsed to `usvg::Tree` on the main thread** inside
  `ImagePipeline::poll()` when bytes arrive for an SVG-kind resource, populating
  `decoded_cache` with `DecodedImage::Svg`.

This sidesteps `Send` entirely and is cheap in practice (SVG resources per
chapter are few — usually a cover and a handful of figures; a single parse is
sub-millisecond). If a future usvg version makes `Tree: Send`, parsing can move
back onto `decode_worker` with no API change.

Sniffing: an SVG is detected by leading `<?xml`/`<svg` after BOM/whitespace, or
by `.svg` extension / `image/svg+xml` data-URI at registration.

### usvg parse options
Build `usvg::Options` once per parse with:
- `image_href_resolver` wired to our `ResourceProvider` so the wrapped-cover
  case (`<image xlink:href="cover.jpg">` *inside* the SVG) resolves through the
  EPUB provider. **Without this the cover renders blank** — primary thing to
  verify.
- `fontdb` seeded with `Liberation Serif` (the forced UA family) so `<text>` in
  SVG renders.
- `style_sheet` set per §5 (currentColor).

---

## 3. Intrinsic sizing (#1 — the real work)

SVG is not a fixed-pixel raster; it has a `viewBox` aspect ratio plus possibly
`%`/`auto`/absolute `width`/`height`. The critical constraint: **vello's
`draw_svg` applies independent `scale_x`/`scale_y` to fit the target rect**
(confirmed in `floem_vello_renderer`), so if we hand it a rect whose ratio ≠ the
SVG's intrinsic ratio, the SVG is *stretched*. Sizing must therefore preserve the
intrinsic ratio end to end.

`resources/images.rs::probe_dimensions` (called from `load.rs:88`)
- Add an SVG branch: parse with usvg, return `tree.size()` rounded as the
  intrinsic `(width, height)`. usvg resolves `viewBox` + `width`/`height` (incl.
  percentages) into a concrete size that carries the correct ratio.
- This feeds the **existing** `load.rs` aspect-ratio derivation (fills a missing
  `width_attr`/`height_attr` from the other via intrinsic ratio) and
  `display_size()` / `scaled_image_size` (`render.rs:17`) unchanged. Because the
  probed size carries the true ratio, the fitted rect ratio matches and vello
  does *not* stretch.

Behavior this yields, matching CSS replaced-element rules:
- `<img src=*.svg>` with `width`/`height` attrs → those win (intentional ratio
  override, as in browsers).
- SVG with only a `viewBox` → fit to column width, height proportional.
- Wrapped cover (`width=100% height=100%`) → usvg returns the `viewBox` size;
  fits to column width preserving ratio.

Inline SVG (case 3) reuses this: capture `width`/`height` attrs on the `<svg>`
element at registration (today `register_image_resource` reads them only for
`img`/`image` — extend to `svg`), else fall back to `tree.size()`.

**Documented limitation / future:** a full-page cover currently fits column
*width* only; "contain within the page box (fit width *and* height, centered)"
is a general replaced-element-in-paginated-reader concern, not SVG-specific, and
is left for later. Recorded in §7.

---

## 4. Paint

`renderer/painter.rs`
- Add to the `Painter` trait: `fn draw_svg(&mut self, tree: &usvg::Tree, hash: &[u8], rect: Rect);`
- `PaintCx` impl: forward to `Renderer::draw_svg(Svg { tree, hash, .. }, rect, None::<peniko::BrushRef>)`
  (fill the `Svg` struct per its actual fields; brush is `None` — document SVG
  keeps its own colors).
- `SoftwarePainter` impl: no-op (documented; out of scope per decisions).

`renderer/render.rs::draw_images_for_line` (`render.rs:342`)
- Match the `DecodedImage` variant: `Raster` → `draw_image` (unchanged),
  `Svg` → `draw_svg`. Rect comes from `scaled_image_size` exactly as for rasters.

Everything else in the paint loop, hit-testing, and the prefetch window
(`prefetch_images_for_current_page`, `render.rs:507`) is unchanged.

---

## 5. currentColor (#2 — 90% coverage)

usvg resolves `currentColor` natively at parse time against the SVG's cascaded
`color`. We steer it with a one-line injected stylesheet rather than rewriting
attributes:

- Compute the element's `color` (already resolved on the `<img>`/`<svg>` DOM node).
- Set `usvg::Options.style_sheet = Some(format!("svg{{color:{hex}}}"))`.

This makes `fill="currentColor"` / `stroke="currentColor"` track our computed
text color (and become dark-mode-ready for free) instead of usvg's default black.

**Documented limitation (intentional):** the *full* document CSS cascade onto
SVG elements (e.g. external `.diagram path { fill: red }` rules in the chapter
stylesheet) is **not** applied. Reaching it would require either re-emitting our
lightningcss-parsed author CSS as text into `style_sheet` (lossy — usvg uses
`simplecss` with a different cascade) or adding SVG presentation properties
(`fill`, `stroke`, …) to `ComputedStyle`, which the style engine does not model.
This is rare in EPUB (authors inline SVG styling because reading-system support
is poor) and is deliberately out of scope. Inline `style=`/presentation
attributes and `<style>` *inside* the SVG continue to work (usvg handles them).

---

## 6. Risks / verify first

- **`image_href_resolver` wiring** — the wrapped-cover case is blank without it.
  Verify with a real SVG-wrapped cover.
- **`usvg::Tree: Send`** — confirmed assumption is "not Send"; parse on main
  thread (§2). Re-check on usvg upgrades.
- **`vello_svg` feature coverage** — static fills/strokes/gradients/clips render
  well; exotic filters/blend modes may no-op. Acceptable for EPUB.
- **Self-closed SVG tags** (`<path/>`) — internal to the serialized subtree
  scraper already parsed, so the `<a/>`-only normalizer is not involved; add a
  test with a self-closed-heavy SVG.
- **Per-paint tessellation** — `draw_svg` re-runs `vello_svg::render_tree` each
  paint. Negligible for a page-turn-driven reader; caching the vello `Scene`
  would couple `render.rs` to the vello backend and is intentionally *not* done.

## 7. Out of scope / documented limitations

- Software/PNG (`SoftwarePainter`) SVG output.
- Full document-CSS cascade onto SVG paths (§5) — only currentColor is bridged.
- "Contain cover within the page box" sizing (§3) — fits column width only.
- SVG animation / interactivity / scripting (never relevant to static EPUB).

## 8. Testing

- Unit: `decode`/parse of `<svg><rect/></svg>` yields `DecodedImage::Svg`;
  `probe_dimensions` returns the `viewBox` size; currentColor stylesheet
  injection produces the expected resolved fill.
- Integration (`bin/render_to_png.rs` drives layout even though PNG won't paint
  SVG): fixture chapter with (a) SVG-wrapped raster cover, (b) `<img src=*.svg>`,
  (c) inline vector SVG using `currentColor` → assert image fragments are
  produced at the correct (ratio-preserving) sizes.
- Manual: open in the app, confirm crispness on zoom and that currentColor tracks
  text color.
- Regression: raster (PNG/JPEG) decode + paint path unchanged.

## 9. Sequencing

1. Data model: `DecodedImage` enum, `ImageSource::Inline`, `ResourceKind` hint.
2. Decode: sniff + main-thread usvg parse in `poll()`; `Options` with
   `image_href_resolver` + fontdb.
3. Sizing: `probe_dimensions` SVG branch (#1).
4. Paint: `Painter::draw_svg` (PaintCx forward, Software no-op) + `render.rs`
   variant match. → cases 1 & 2 working end to end.
5. currentColor: `style_sheet` injection (#2, §5).
6. Inline SVG: serialize `<svg>` subtree → `ImageSource::Inline`, capture
   width/height attrs, suppress subtree from box layout. → case 3.
