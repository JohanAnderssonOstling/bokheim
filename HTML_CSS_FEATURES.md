# HTML + CSS Feature Support Audit

This audit is based on the current implementation in `HtmlRenderer`, mainly:

- `HtmlRenderer/src/parser/html.rs`
- `HtmlRenderer/src/style/resolver.rs`
- `HtmlRenderer/src/style/selectors.rs`
- `HtmlRenderer/src/style/selectors_dom.rs`
- `HtmlRenderer/src/layout/box_builder.rs`
- `HtmlRenderer/src/layout/box_layout.rs`
- `HtmlRenderer/src/layout/inline.rs`
- `HtmlRenderer/src/layout/table_layout.rs`
- `HtmlRenderer/src/renderer/load.rs`
- `HtmlRenderer/src/renderer/render.rs`

Notes:

- This distinguishes between features that are parsed, styled, and actually laid out/rendered.
- `svg` and list rendering are not implemented in practice, even if a few related tags or CSS defaults exist.

## Summary

The strongest implemented areas are:

- Basic HTML DOM parsing
- Block and inline layout
- Text shaping and line breaking
- Inline styling
- Links
- Inline images
- Table layout

The biggest missing areas are:

- SVG rendering
- Lists and markers
- Flexbox, grid, positioning, float
- Advanced selector/pseudo support
- Background images and other visual effects

## Feature Matrix

| Feature | Implemented | Not implemented / partial |
|---|---|---|
| HTML parsing | Generic DOM tree for elements, text nodes, attributes, ids, classes, namespaces. Arbitrary tags are parsed. | Comments and other non-element/non-text nodes are ignored. |
| Root handling | Uses `html`, `body`, or `svg` as root if present. | `svg` can be selected as root, but SVG rendering is not implemented. |
| Block/inline layout | `display: block`, `inline`, `none`, `list-item`, and table display types are recognized. Layout is display-driven, not tag-driven. | No flex, grid, float, positioning, z-index, overflow layout, or multicolumn layout. |
| Text content | Text nodes render; `<br>` creates hard line breaks. | No bidi/direction support found. |
| Whitespace | `normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line` are handled in shaping and line breaking. | `break-spaces` is mapped to `pre-wrap`, so behavior is not exact. |
| Text alignment | `left`, `right`, `center`, `justify`, `text-align-last` supported. | No advanced justification or hyphenation behavior. |
| Text indent | `text-indent`, including `hanging` and `each-line`, is implemented. | No special page-break interaction. |
| Text transform | `uppercase`, `lowercase`, `capitalize` supported. | None noted. |
| Fonts | `font`, `font-size`, `font-weight`, `font-style`, `font-family`, `line-height`, `font-variant-caps: small-caps` supported. | Authored font families are passed to the active rendering backend. |
| Spacing | `letter-spacing`, `word-spacing` supported. | `word-break`, `overflow-wrap`, `word-wrap` unsupported. |
| Vertical align | Inline and table-cell `vertical-align` support for `baseline`, `sub`, `super`, `top`, `text-top`, `middle`, `bottom`, `text-bottom`, length, and percent values. | Likely incomplete versus browser behavior, but core cases exist. |
| Text decoration | Underline, overline, and line-through are parsed and painted. | No richer decoration styles or colors. |
| Links | `<a href>` is parsed, styled, clickable, and internal navigation works. `:link` and `:any-link` matching exists. | `:visited` intentionally does not match. No hover/focus/active styling support found. |
| Images | `<img>` and `<image>` sources are loaded, intrinsic dimensions are probed, `width` and `height` attrs are used, aspect ratio is preserved when one dimension is supplied. Images render inline. | No background images. No SVG document rendering. No object-fit or object-position. |
| Tables | `table`, `caption`, `col`, `colgroup`, `thead`, `tbody`, `tfoot`, `tr`, `td`, `th` all have real box and layout support. | `inline-table` is parsed but built as block, so it is not truly supported. |
| Table spans | `colspan` and `rowspan` are implemented. | No evidence of browser-grade malformed-table recovery. |
| Table sizing | Intrinsic min/max sizing, width hints from cells and `col`/`colgroup`, percent and px widths, and min/max widths on cells are used. | No `table-layout` property support. |
| Table captions | Top and bottom captions supported via `caption-side`. | Nothing beyond top/bottom. |
| Table borders | `border-collapse: separate/collapse`, `border-spacing`, collapsed border conflict handling, and cell/table borders all exist. | Renderer only paints `solid` borders. `dashed` and `dotted` are parsed but not rendered distinctly. |
| Empty cells | `empty-cells: show/hide` supported. | None obvious beyond that. |
| Backgrounds | `background-color` works on boxes. | `background-image`, `background-size`, `background-position`, `background-repeat` unsupported. |
| Box model | `width`, `height`, `min-width`, `min-height`, `max-width`, `max-height`, margins, padding, border widths, border colors, and border styles supported. | No `box-sizing`. No border radius. |
| CSS selectors | Type, class, id, universal, attribute selectors, descendant, child, adjacent sibling, and general sibling combinators are implemented. | Pseudo-elements unsupported. Most pseudo-classes are missing. Structural selectors like `:nth-child` are not implemented. |
| Cascade | Specificity and source order, `!important`, inline `style=""`, `<style>`, linked stylesheets, custom properties, and `var(...)` support exist. | Custom property substitution appears limited compared with full browser support. |
| CSS loading | Default UA stylesheet, local stylesheet files, `<style>`, and `<link rel="stylesheet">` are loaded. | No browser-like remote fetching beyond the project resource provider. |
| Page breaking | Property names for `page-break-*` and `break-*` are recognized. | Explicitly no-op. |
| Hyphenation | `hyphens` is recognized. | Explicitly no-op. |
| Lists | No real list implementation. | `list-style`, `list-style-type`, `list-style-position`, list counters, and marker generation are unsupported. `li { display: list-item; }` exists, but there is no actual marker/counter layout. |
| SVG | Not implemented in practice. | Only minor parsing conveniences exist, plus `<image>` source handling. No SVG layout or paint engine. |
| Forms/media | Generic tags can exist in the DOM. | No real rendering support for form controls, `canvas`, `video`, `iframe`, or similar replaced/interactive elements beyond generic boxes. |

## HTML Features

### Implemented

- Generic element parsing
- Text nodes
- Attributes
- `id`
- `class`
- `href`
- `src`
- `width` and `height` attributes for images
- `<br>`
- Inline and block content mixing
- `<a>`
- `<img>`
- Table elements:
  - `<table>`
  - `<caption>`
  - `<col>`
  - `<colgroup>`
  - `<thead>`
  - `<tbody>`
  - `<tfoot>`
  - `<tr>`
  - `<td>`
  - `<th>`

### Partially implemented

- `<svg>` may be chosen as a root node, but it is not actually rendered as SVG.
- `<image>` is only useful as an image resource hook, not as SVG rendering support.
- Arbitrary unknown tags still parse and may get generic layout if styled.

### Not implemented

- SVG rendering
- List markers/counters
- Real form control rendering
- Canvas/video/iframe/object-style replaced element behavior
- Comment node handling in output/layout

## CSS Property Support

### Text and font

Implemented:

- `font-size`
- `font-weight`
- `font-style`
- `font-family`
- `font`
- `line-height`
- `letter-spacing`
- `word-spacing`
- `text-align`
- `text-align-last`
- `text-indent`
- `text-transform`
- `font-variant-caps`
- `text-decoration`
- `text-decoration-line`
- `vertical-align`
- `white-space`
- `color`

Partial:

- `font-family` is parsed and passed to the active rendering backend for font selection.

Not implemented:

- `word-break`
- `overflow-wrap`
- `word-wrap`
- `text-shadow`
- `text-rendering`
- `direction`

### Background and paint

Implemented:

- `background-color`
- `background` only for color extraction

Not implemented:

- `background-image`
- `background-size`
- `background-position`
- `background-repeat`
- `opacity`
- `filter`
- `box-shadow`
- `border-radius`

### Box model

Implemented:

- `width`
- `height`
- `min-width`
- `min-height`
- `max-width`
- `max-height`
- `margin-*`
- `margin`
- `margin-block*`
- `margin-inline*`
- `padding-*`
- `padding`
- `border-width`
- `border-color`
- `border-style`
- `border`
- side-specific border shorthands

Partial:

- Border styles are parsed as `solid`, `dashed`, and `dotted`, but layout/painting only treats `solid` as visibly rendered in practice.

Not implemented:

- `box-sizing`
- `outline`
- `outline-style`
- `overflow`
- `overflow-x`
- `position`
- `top`
- `right`
- `bottom`
- `left`
- `z-index`
- `transform`
- `transition`
- `animation`

### Tables

Implemented:

- `display` values needed for table model
- `border-collapse`
- `border-spacing`
- `empty-cells`
- `caption-side`

Not implemented:

- `table-layout`

### Fragmentation and hyphenation

Recognized but no-op:

- `hyphens`
- `page-break-before`
- `page-break-after`
- `page-break-inside`
- `break-before`
- `break-after`
- `break-inside`

## CSS Selector Support

### Implemented

- Type selectors
- Class selectors
- ID selectors
- Universal selector
- Attribute existence selectors
- Attribute value selectors
- Descendant combinator
- Child combinator
- Adjacent sibling combinator
- General sibling combinator
- `:link`
- `:any-link`

### Not implemented or effectively missing

- `:visited` matching is disabled
- Pseudo-elements
- Structural pseudo-classes such as `:nth-child`
- Most interactive/state pseudo-classes
- Most language/direction-sensitive pseudo-classes

## Table Subfeatures

### Implemented

- Table box tree construction
- Caption extraction and placement
- Top and bottom captions
- Row groups:
  - `thead`
  - `tbody`
  - `tfoot`
- Column groups and columns:
  - `colgroup`
  - `col`
- Cell placement grid
- `colspan`
- `rowspan`
- Intrinsic width measurement
- Column width hint propagation from `col` and `colgroup`
- Cell width hints from px and percent widths
- Cell min/max width hints
- Border spacing in separate border mode
- Border collapsing mode
- Collapsed border conflict resolution
- Cell vertical alignment
- `empty-cells: hide`

### Partial

- `inline-table` is not truly inline.
- Only solid borders visibly render.
- Behavior may differ from browsers on malformed or highly complex tables.

### Not implemented

- `table-layout`
- Full browser compatibility for all legacy HTML table quirks

## Lists

Status: not implemented.

What exists:

- UA CSS assigns `display: list-item` to `li`
- UA CSS includes `list-style-type` defaults for `ul`/`ol`

What is missing:

- Marker generation
- Counter generation
- `list-style-type`
- `list-style`
- `list-style-position`
- `list-style-image`
- Ordered list numbering
- Bullet shape handling
- Nested list marker changes

## SVG

Status: not implemented.

What exists:

- Root selection may fall back to `svg`
- `xlink:href` and `<image>` can feed image resource resolution

What is missing:

- SVG layout
- SVG painting
- SVG shapes
- SVG text
- SVG transforms
- SVG styling model

## Practical Bottom Line

Today this renderer is best described as:

- A document-oriented HTML renderer
- Good for text-heavy EPUB-like content
- Strongest on paragraphs, inline formatting, links, images, and tables
- Not a general web browser engine

It is not yet suitable for:

- Modern web app layout
- SVG-heavy documents
- Correct list rendering
- Advanced visual CSS
