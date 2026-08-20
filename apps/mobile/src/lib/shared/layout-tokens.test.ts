import assert from "node:assert/strict";
import test from "node:test";
import { BROWSING_LAYOUT_CONTRACT } from "./types.ts";
import { LAYOUT_TOKEN_FIELDS, browsingLayoutStyle } from "./layout-tokens.ts";

const declarations = () =>
  new Map(
    browsingLayoutStyle()
      .split("; ")
      .map((entry) => {
        const separator = entry.indexOf(": ");
        return [entry.slice(0, separator), entry.slice(separator + 2)] as const;
      }),
  );

test("every layout contract field is classified as scaled or fixed", () => {
  const classified = new Set<string>([
    ...LAYOUT_TOKEN_FIELDS.lengths.map(([, field]) => field as string),
    ...LAYOUT_TOKEN_FIELDS.derived,
  ]);
  const unclassified = Object.keys(BROWSING_LAYOUT_CONTRACT).filter(
    (field) => !classified.has(field),
  );
  assert.deepEqual(
    unclassified,
    [],
    `add these to LENGTH_TOKENS (with a scaling) or DERIVED_FIELDS: ${unclassified.join(", ")}`,
  );
});

test("sizes are emitted in rem and spacing in px", () => {
  const emitted = declarations();
  for (const [cssVariable, , scaling] of LAYOUT_TOKEN_FIELDS.lengths) {
    const value = emitted.get(cssVariable);
    assert.ok(value, `${cssVariable} is not emitted`);
    const unit = scaling === "scaled" ? "rem" : "px";
    assert.ok(
      value.endsWith(unit),
      `${cssVariable} is ${scaling} so it must be emitted in ${unit}, got "${value}"`,
    );
  }
});

test("anything holding text scales with the reader's UI font size", () => {
  // A size left in px freezes while the text inside it grows. These are the
  // containers and glyph sizes that regressed before, kept honest here.
  const mustScale = [
    "--hierarchy-cell-height",
    "--hierarchy-touch-cell-min-height",
    "--hierarchy-cell-min-width",
    "--hierarchy-label-size",
    "--hierarchy-icon-size",
    "--navigation-item-min-height",
    "--navigation-touch-item-min-height",
    "--layout-topbar-height",
    "--layout-topbar-action-height",
    "--carousel-arrow-size",
  ];
  const emitted = declarations();
  for (const cssVariable of mustScale) {
    assert.match(
      emitted.get(cssVariable) ?? "",
      /rem$/,
      `${cssVariable} must follow the UI font size`,
    );
  }
});

test("gaps, padding and radii stay fixed", () => {
  const emitted = declarations();
  for (const [cssVariable, value] of emitted) {
    if (!/(-gap|-padding|-corner-radius|trailing-space)$/.test(cssVariable)) continue;
    assert.match(
      value,
      /px$/,
      `${cssVariable} is spacing and must not scale, got "${value}"`,
    );
  }
});

test("derived containers still equal the contract at the default font size", () => {
  const layout = BROWSING_LAYOUT_CONTRACT;
  const titleBlock = layout.book_card_title_line_height_px * layout.book_card_title_lines;
  const card =
    layout.book_cover_height_px +
    layout.book_card_cover_content_gap_px +
    titleBlock +
    layout.book_card_text_gap_px +
    layout.book_card_author_line_height_px +
    layout.book_card_text_gap_px +
    layout.book_card_progress_height_px;
  assert.equal(card, layout.book_card_height_px);
  assert.equal(
    card + layout.book_grid_row_trailing_space_px,
    layout.book_grid_row_height_px,
  );
  assert.equal(
    layout.author_row_cover_height_px +
      titleBlock +
      layout.author_row_content_gap_px +
      layout.author_row_horizontal_padding_px * 2,
    layout.author_row_height_px,
  );
});
