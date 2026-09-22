//! Declarative sizing policies for paginator groups.
//!
//! A group declares how wide its children are, how tall a row is, and how
//! spare horizontal space is used. All grid groups expand either their children
//! or their gaps across the available width, regardless of their child count.

use app_preferences::CoverText;
use gpui::{Pixels, Rems, px};

/// The one horizontal gap, used between children and at either edge of a
/// expanded row. Groups that distribute slack into gaps use this as a minimum.
pub(super) const COLUMN_GAP: f32 = ui_components::SPACE_SM;

/// The nominal height assigned to a child. During intrinsic compaction this
/// also caps measurement, allowing a shorter child to reduce its row height.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct PaginatorSizing {
    aspect_ratio: Option<f32>,
    aspect_width_limit: Option<Pixels>,
    rems: Rems,
    pixels: Pixels,
    /// Padding the child carries on every side.
    ///
    /// Load-bearing for any group sized by aspect ratio: the ratio applies to
    /// the child's *content*, so padding narrows it and shortens it by more
    /// than the padding itself. Expressing that here keeps the arithmetic in
    /// one place instead of asking each group to pre-compensate its pixel term.
    padding: Pixels,
    /// How tall the children of a group nested inside this one resolve to, at
    /// this child's own width.
    ///
    /// A shelf row contains a second paginator, and the cards in it stretch with
    /// the window. Restating their height as a constant means reserving the
    /// height of a card at its nominal width and clipping every wider one; this
    /// asks the nested group instead, so the row is as tall as what it holds.
    nested: Option<fn(Pixels, Pixels) -> Pixels>,
}

impl PaginatorSizing {
    pub(crate) fn rems(height: Rems) -> Self {
        Self { aspect_width_limit: None, aspect_ratio: None, rems: height, pixels: Pixels::ZERO, padding: Pixels::ZERO, nested: None }
    }

    pub(crate) fn pixels(height: Pixels) -> Self {
        Self { aspect_width_limit: None, aspect_ratio: None, rems: Rems::ZERO, pixels: height, padding: Pixels::ZERO, nested: None }
    }

    pub(crate) fn aspect_ratio(ratio: f32) -> Self {
        Self { aspect_width_limit: None, aspect_ratio: Some(ratio), rems: Rems::ZERO, pixels: Pixels::ZERO, padding: Pixels::ZERO, nested: None }
    }

    /// Cap the width used for aspect-ratio height while text can grow wider.
    pub(crate) fn with_aspect_width_limit(mut self, width: Pixels) -> Self {
        self.aspect_width_limit = Some(width);
        self
    }

    pub(crate) fn with_rems(mut self, height: Rems) -> Self {
        self.rems = height;
        self
    }

    pub(crate) fn with_pixels(mut self, height: Pixels) -> Self {
        self.pixels = height;
        self
    }

    pub(crate) fn with_padding(mut self, padding: Pixels) -> Self {
        self.padding = padding;
        self
    }

    /// Adds the resolved height of a group laid out inside this child.
    pub(crate) fn with_nested(mut self, nested: fn(Pixels, Pixels) -> Pixels) -> Self {
        self.nested = Some(nested);
        self
    }

    pub(super) fn height(self, width: Pixels, rem_size: Pixels) -> Pixels {
        let padding = f32::from(self.padding).max(0.0);
        let content_width = (f32::from(width) - padding * 2.0).max(0.0);
        let content_width = self.aspect_width_limit.map_or(content_width, |limit| content_width.min(f32::from(limit)));
        let aspect_height = self.aspect_ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0).map(|ratio| content_width / ratio).unwrap_or_default();
        let nested = self.nested.map_or(0.0, |nested| f32::from(nested(width, rem_size)));
        px((aspect_height + nested + f32::from(self.rems.to_pixels(rem_size)) + f32::from(self.pixels) + padding * 2.0).max(1.0))
    }
}

/// The width a child is measured at before any stretching. It decides how
/// many columns fit; minimum column counts can force narrower children.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct PaginatorWidthPolicy {
    rems: Rems,
    fixed_pixels: Pixels,
}

impl PaginatorWidthPolicy {
    pub(crate) fn new(width: Rems) -> Self {
        Self { rems: Rems(width.0.max(0.0625)), fixed_pixels: Pixels::ZERO }
    }

    pub(crate) fn pixels(width: Pixels) -> Self {
        Self { rems: Rems::ZERO, fixed_pixels: px(f32::from(width).max(1.0)) }
    }

    /// Adds fixed chrome, such as a cover and card padding, to the rem-based
    /// text measure declared by `new`.
    pub(crate) fn with_pixels(mut self, fixed: Pixels) -> Self {
        self.fixed_pixels = px(f32::from(fixed).max(0.0));
        self
    }

    pub(super) fn width(self, rem_size: Pixels) -> Pixels {
        self.rems.to_pixels(rem_size) + self.fixed_pixels
    }
}

/// Geometry shared by all children in a group. Groups never share rows, so
/// their widths, sizing policies, and gaps resolve independently.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct PaginatorGroupPolicy {
    pub(crate) width: PaginatorWidthPolicy,
    pub(crate) intrinsic: bool,
    pub(crate) sizing: PaginatorSizing,
    pub(crate) row_gap: Pixels,
    /// Fewest columns the group will lay out, however narrow the window gets.
    pub(crate) minimum_columns: usize,
    /// Horizontal gap, defaulting to [`COLUMN_GAP`]. A group that wants its
    /// children packed tighter than the grid standard sets this; keeping it on
    /// the policy is what allows one group to change without dragging every
    /// other group along with it.
    pub(crate) column_gap: Pixels,
    /// Grow to this width, then put remaining slack into gaps. None is unbounded.
    pub(crate) maximum_width: Option<PaginatorWidthPolicy>,
    /// Whether a stretched row keeps its leftover slack at the edges. A group
    /// sitting under a heading turns this off, so the first child starts where
    /// the title does instead of a gap to the right of it.
    pub(crate) inset_edges: bool,
    /// A margin held at both edges of every row in the group, taken out of the
    /// width before the column count.
    ///
    /// Distinct from `inset_edges`, which only decides where a stretched row's
    /// leftover goes. This margin makes the row narrower rather than pushing
    /// children off the end.
    pub(crate) edge_inset: Pixels,
}

impl PaginatorGroupPolicy {
    pub(crate) fn new(width: PaginatorWidthPolicy, sizing: PaginatorSizing, row_gap: Pixels) -> Self {
        Self { intrinsic: false, width, sizing, row_gap: px(f32::from(row_gap).max(0.0)), column_gap: px(COLUMN_GAP), maximum_width: None, minimum_columns: 1, inset_edges: true, edge_inset: Pixels::ZERO }
    }

    /// Wrap measured child widths with fixed gaps and no stretching.
    pub(crate) fn intrinsic(mut self) -> Self {
        self.intrinsic = true;
        self
    }

    /// Limit card growth before expanding the gaps in wrapped groups.
    pub(crate) fn with_maximum_width(mut self, width: PaginatorWidthPolicy) -> Self {
        self.maximum_width = Some(width);
        self
    }

    /// Holds the group's rows `inset` off both edges of the container.
    ///
    /// For a group whose children are bordered boxes sitting above a card grid:
    /// a card is flush with the container and insets its cover by its own
    /// padding, so a flush group of boxes overhangs those covers by exactly that
    /// padding on each side. Giving the group the same inset puts the two on one
    /// vertical line.
    pub(crate) fn with_edge_inset(mut self, inset: Pixels) -> Self {
        self.edge_inset = px(f32::from(inset).max(0.0));
        self
    }

    /// Starts the row flush with the container, for a group whose heading sits
    /// directly above it.
    pub(crate) fn flush_edges(mut self) -> Self {
        self.inset_edges = false;
        self
    }

    /// Keeps at least `columns` children on a row even when the window is too
    /// narrow for them at their preferred width.
    pub(crate) fn with_minimum_columns(mut self, columns: usize) -> Self {
        self.minimum_columns = columns.max(1);
        self
    }

    /// Sets both gaps at once, for a group spaced equally in both directions.
    pub(crate) fn with_gap(mut self, gap: Pixels) -> Self {
        let gap = px(f32::from(gap).max(0.0));
        self.row_gap = gap;
        self.column_gap = gap;
        self
    }
}

/// A one-row horizontal shelf of ordinary book cards.
///
/// Home's carousels and the author index's shelves are the same thing seen in
/// two places, so they resolve their geometry here rather than each restating
/// it. The cards are the ordinary book cards at their ordinary width: a shelf
/// that sized its covers differently would be a second visual language for the
/// same object.
///
/// Rows start flush with their heading. Cards grow up to 25%, then spare
/// width expands the gaps.
pub(crate) fn shelf_card_policy(cover_text: CoverText) -> PaginatorGroupPolicy {
    let (extra_height, fixed_height) = text_block_height(cover_text);
    let gap = px(ui_components::BOOK_GRID_BOOK_GAP);
    PaginatorGroupPolicy::new(
        PaginatorWidthPolicy::pixels(px(ui_components::BOOK_CARD_WIDTH)),
        PaginatorSizing::aspect_ratio(ui_components::BOOK_COVER_ASPECT_RATIO).with_rems(Rems(extra_height)).with_pixels(px(fixed_height)).with_padding(px(ui_components::BOOK_CARD_PADDING)),
        gap,
    )
    .with_gap(gap)
    .with_maximum_width(PaginatorWidthPolicy::pixels(px(ui_components::BOOK_CARD_MAXIMUM_WIDTH)))
    .with_minimum_columns(2)
    .flush_edges()
}

/// The text block's contribution to a card's reserved height, under either
/// value of the "cover text" setting.
fn text_block_height(cover_text: CoverText) -> (f32, f32) {
    match cover_text {
        CoverText::Always => (ui_components::BOOK_CARD_TITLE_HEIGHT_REM, ui_components::BOOK_CARD_COVER_CONTENT_GAP),
        CoverText::CoverOnly => (ui_components::BOOK_CARD_NO_TEXT_HEIGHT_REM, ui_components::BOOK_CARD_NO_TEXT_FIXED_HEIGHT),
    }
}

/// The book grid on a browse page, in either of the card's two modes.
///
/// A detailed card is the ordinary card with a blurb opened out beside it: the
/// cover and title keep their stack, at the ordinary card's width, and the
/// doubled cell gives the gained half to the words. So both modes are the
/// same arithmetic — cover, then title and subtitle — and the only thing the
/// detailed card changes is the width the cover is a fraction of, which is
/// what its ratio carries.
///
/// Detailed cards widen their description up to a cap, then expand gaps.
/// Cover-only cards grow up to 25% before expanding gaps.
///
/// A detailed card always states its title — the blurb needs a heading, and
/// there is no separate cover-scale grid to compare it against — so
/// `cover_text` is read only in the compact case.
pub(crate) fn book_card_policy(detailed: bool, cover_text: CoverText) -> PaginatorGroupPolicy {
    let (width, maximum_width, cover_ratio, minimum_columns) = if detailed {
        (
            PaginatorWidthPolicy::new(Rems(ui_components::BOOK_CARD_DETAIL_TEXT_WIDTH_REM)).with_pixels(px(ui_components::BOOK_CARD_DETAIL_FIXED_WIDTH)),
            PaginatorWidthPolicy::new(Rems(ui_components::BOOK_CARD_DETAIL_TEXT_MAXIMUM_WIDTH_REM)).with_pixels(px(ui_components::BOOK_CARD_DETAIL_FIXED_WIDTH)),
            ui_components::BOOK_CARD_DETAIL_ASPECT_RATIO,
            // A detailed card is already two ordinary cards wide, so a narrow
            // window shows one rather than halving the covers.
            1,
        )
    } else {
        let width = PaginatorWidthPolicy::pixels(px(ui_components::BOOK_CARD_WIDTH));
        (width, PaginatorWidthPolicy::pixels(px(ui_components::BOOK_CARD_MAXIMUM_WIDTH)), ui_components::BOOK_COVER_ASPECT_RATIO, ui_components::BOOK_GRID_MINIMUM_COLUMNS)
    };
    let (extra_height, fixed_height) = text_block_height(if detailed { CoverText::Always } else { cover_text });
    let mut sizing = PaginatorSizing::aspect_ratio(cover_ratio).with_rems(Rems(extra_height)).with_pixels(px(fixed_height)).with_padding(px(ui_components::BOOK_CARD_PADDING));
    if detailed {
        sizing = sizing.with_aspect_width_limit(px(ui_components::BOOK_CARD_CONTENT_WIDTH / ui_components::BOOK_CARD_DETAIL_COVER_FRACTION));
    }
    let gap = px(ui_components::BOOK_GRID_BOOK_GAP);
    PaginatorGroupPolicy::new(width, sizing, gap).with_gap(gap).with_maximum_width(maximum_width).with_minimum_columns(minimum_columns).flush_edges()
}

/// How tall the cards on a shelf resolve to, given the width of the row they
/// sit in. The carousel fills its row, so the row's width is theirs.
///
/// Two named functions rather than one taking `cover_text`: the nested-height
/// slot on [`PaginatorSizing`] is a bare function pointer, which cannot close
/// over an argument, so the setting has to be baked in at the point
/// `shelf_rows_policy` picks which one to hand it.
fn shelf_cards_height(cover_text: CoverText, row_width: Pixels, rem_size: Pixels) -> Pixels {
    let policy = shelf_card_policy(cover_text);
    policy.sizing.height(super::geometry::group_geometry(policy, 0, row_width, rem_size).child_width, rem_size)
}

fn shelf_cards_height_always(row_width: Pixels, rem_size: Pixels) -> Pixels {
    shelf_cards_height(CoverText::Always, row_width, rem_size)
}

fn shelf_cards_height_cover_only(row_width: Pixels, rem_size: Pixels) -> Pixels {
    shelf_cards_height(CoverText::CoverOnly, row_width, rem_size)
}

/// The page those shelves stack on: one full-width row each.
///
/// A declared width wider than the window is what gives each row the line to
/// itself — the paginator clamps it to the content area before taking the column
/// count, so exactly one column fits at exactly the page's width.
///
/// A row is its heading, the gap under it, and the cards themselves. The cards
/// are asked rather than restated: they stretch up to a quarter wider than
/// nominal on a wide window, and a cover is sized by aspect ratio, so a row
/// holding their nominal height would clip the bottom of every stretched cover
/// and the title under it.
pub(crate) fn shelf_rows_policy(cover_text: CoverText, subject_rows: usize) -> PaginatorGroupPolicy {
    let cards: fn(Pixels, Pixels) -> Pixels = match cover_text {
        CoverText::Always => shelf_cards_height_always,
        CoverText::CoverOnly => shelf_cards_height_cover_only,
    };
    PaginatorGroupPolicy::new(
        PaginatorWidthPolicy::new(Rems(ui_components::SHELF_ROW_WIDTH_REM)),
        PaginatorSizing::rems(Rems(ui_components::shelf_heading_height_rem(subject_rows))).with_pixels(px(ui_components::CAROUSEL_CONTENT_GAP)).with_nested(cards),
        px(ui_components::SHELF_ROW_GAP),
    )
    .flush_edges()
}
