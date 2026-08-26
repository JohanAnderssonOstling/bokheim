import { BROWSING_LAYOUT_CONTRACT } from "./types.ts";

/**
 * Turns the browsing layout contract into CSS custom properties.
 *
 * Whether a measurement follows the reader's UI font size is a property of the
 * measurement itself, so it is recorded here once rather than decided again at
 * every use. `application-appearance.ts` sets the root font size from
 * `ui_font_size_px`, which means `rem` tracks that preference and `px` does not.
 *
 *   scaled — is, or contains, text: row heights, control heights, icon sizes,
 *            minimum widths that a label has to fit inside, type sizes.
 *   fixed  — gaps, margins, padding and corner radii. These have no coupling to
 *            the text, and scaling them only makes dense views looser.
 *
 * Leaving a size fixed while the text inside it grows is what once clipped book
 * cards and truncated subject names against a frozen column width.
 */
type Scaling = "scaled" | "fixed";

const ROOT_FONT_SIZE_PX = 16;

/** Contract fields emitted verbatim as a single length. */
const LENGTH_TOKENS: [
  cssVariable: string,
  field: keyof typeof BROWSING_LAYOUT_CONTRACT,
  scaling: Scaling,
][] = [
  ["--navigation-label-size", "navigation_label_size_px", "scaled"],
  ["--navigation-icon-size", "navigation_icon_size_px", "scaled"],
  ["--navigation-icon-label-gap", "navigation_icon_label_gap_px", "fixed"],
  ["--navigation-item-min-height", "navigation_item_min_height_px", "scaled"],
  ["--navigation-touch-item-min-height", "navigation_touch_item_min_height_px", "scaled"],
  ["--layout-topbar-height", "topbar_height_px", "scaled"],
  ["--layout-topbar-action-height", "topbar_action_height_px", "scaled"],
  ["--layout-topbar-action-gap", "topbar_action_gap_px", "fixed"],
  [
    "--layout-topbar-action-horizontal-padding",
    "topbar_action_horizontal_padding_px",
    "fixed",
  ],
  ["--layout-content-padding", "content_padding_px", "fixed"],
  ["--surface-corner-radius", "surface_corner_radius_px", "fixed"],
  ["--book-cover-width", "book_cover_width_px", "fixed"],
  ["--book-cover-height", "book_cover_height_px", "fixed"],
  ["--book-grid-column-gap", "book_grid_column_gap_px", "fixed"],
  ["--book-card-title-size", "book_card_title_size_px", "scaled"],
  ["--book-card-author-size", "book_card_author_size_px", "scaled"],
  ["--book-card-cover-content-gap", "book_card_cover_content_gap_px", "fixed"],
  ["--book-card-text-gap", "book_card_text_gap_px", "fixed"],
  ["--carousel-title-size", "carousel_title_size_px", "scaled"],
  ["--carousel-section-gap", "carousel_section_gap_px", "fixed"],
  ["--carousel-content-gap", "carousel_content_gap_px", "fixed"],
  ["--carousel-control-gap", "carousel_control_gap_px", "fixed"],
  ["--carousel-arrow-size", "carousel_arrow_size_px", "scaled"],
  ["--author-row-cover-width", "author_row_cover_width_px", "fixed"],
  ["--author-row-cover-height", "author_row_cover_height_px", "fixed"],
  ["--author-row-cover-gap", "author_row_cover_gap_px", "fixed"],
  ["--author-row-content-gap", "author_row_content_gap_px", "fixed"],
  ["--author-row-horizontal-padding", "author_row_horizontal_padding_px", "fixed"],
  ["--author-row-font-size", "author_row_label_size_px", "scaled"],
  ["--hierarchy-cell-height", "hierarchy_cell_height_px", "scaled"],
  ["--hierarchy-touch-cell-min-height", "hierarchy_touch_cell_min_height_px", "scaled"],
  ["--hierarchy-cell-min-width", "hierarchy_cell_min_width_px", "scaled"],
  ["--hierarchy-cell-gap", "hierarchy_cell_gap_px", "fixed"],
  ["--hierarchy-icon-size", "hierarchy_icon_size_px", "scaled"],
  ["--hierarchy-label-size", "hierarchy_label_size_px", "scaled"],
];

/**
 * Fields that are not emitted as a plain length: unitless ratios and weights,
 * text blocks measured in lines, and containers summed from their own parts.
 * Listed so that `layout-tokens.test.ts` can prove every contract field is
 * accounted for, and none was added without a scaling decision.
 */
const DERIVED_FIELDS = [
  "sidebar_width_px",
  "navigation_label_weight",
  "topbar_action_label_weight",
  "book_card_height_px",
  "book_grid_row_height_px",
  "book_card_title_weight",
  "book_card_title_line_height_px",
  "book_card_title_lines",
  "book_card_author_line_height_px",
  "carousel_title_weight",
  "author_row_height_px",
  "author_row_label_size_px",
  "detailed_row_thumbnail_width_px",
  "detailed_row_thumbnail_height_px",
  "detailed_row_height_px",
  "detailed_row_text_measure_px",
  "detailed_row_description_lines",
  "detailed_row_actions_breakpoint_px",
  "metadata_review_item_height_px",
  "large_viewport_breakpoint_px",
  "author_row_cover_count",
] as const;

export const LAYOUT_TOKEN_FIELDS = {
  lengths: LENGTH_TOKENS,
  derived: DERIVED_FIELDS,
};

function length(value: number, scaling: Scaling) {
  return scaling === "scaled" ? `${value / ROOT_FONT_SIZE_PX}rem` : `${value}px`;
}

export function browsingLayoutStyle() {
  const layout = BROWSING_LAYOUT_CONTRACT;
  const titleBlock =
    (layout.book_card_title_line_height_px * layout.book_card_title_lines) /
    ROOT_FONT_SIZE_PX;

  const declarations = [
    // Retained for compatibility with native and older web consumers. The web
    // navigation rail itself is sized by its buttons.
    `--layout-sidebar-width: max(${layout.sidebar_width_px}px, ${layout.sidebar_width_px / ROOT_FONT_SIZE_PX}rem)`,
    `--navigation-label-weight: ${layout.navigation_label_weight}`,
    `--topbar-action-label-weight: ${layout.topbar_action_label_weight}`,
    `--book-card-title-weight: ${layout.book_card_title_weight}`,
    `--carousel-title-weight: ${layout.carousel_title_weight}`,
    // Unitless ratios: a line height relative to its own font size.
    `--book-card-title-line-height: ${layout.book_card_title_line_height_px / layout.book_card_title_size_px}`,
    `--book-card-author-line-height: ${layout.book_card_author_line_height_px / layout.book_card_author_size_px}`,
    `--book-card-title-lines: ${layout.book_card_title_lines}`,
    // Text blocks: a count of lines, so they follow the font.
    `--book-card-title-block-height: ${titleBlock}rem`,
    `--book-card-author-block-height: ${layout.book_card_author_line_height_px / ROOT_FONT_SIZE_PX}rem`,
    // Containers summed from their parts, so the fixed art and the scaling text
    // each contribute on their own terms. At 16px these equal the contract's
    // stated book_card_height_px, book_grid_row_height_px and author_row_height_px.
    "--book-card-height: calc(var(--book-cover-height) + var(--book-card-cover-content-gap) + var(--book-card-title-block-height) + var(--book-card-text-gap) + var(--book-card-author-block-height))",
    "--book-grid-row-height: var(--book-card-height)",
    "--author-row-height: calc(var(--author-row-cover-height) + var(--book-card-title-block-height) + var(--author-row-content-gap) + var(--author-row-horizontal-padding) * 2)",
  ];

  for (const [cssVariable, field, scaling] of LENGTH_TOKENS) {
    declarations.push(`${cssVariable}: ${length(layout[field] as number, scaling)}`);
  }

  return declarations.join("; ");
}
