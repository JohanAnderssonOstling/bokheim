//! Declarative sizing policies for paginator groups.
//!
//! A group declares how wide its children are, how tall a row is, and how
//! spare horizontal space is used. All grid groups expand either their children
//! or their gaps across the available width, regardless of their child count.

use gpui::{Pixels, Rems, px};

/// The one horizontal gap, used between children and at either edge of a
/// expanded row. Groups that distribute slack into gaps use this as a minimum.
pub(super) const COLUMN_GAP: f32 = 8.0;

/// The nominal height assigned to a child. During intrinsic compaction this
/// also caps measurement, allowing a shorter child to reduce its row height.
#[derive(Clone, Copy)]
pub struct PaginatorSizing {
    aspect_ratio: Option<f32>,
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
    /// Section and shelf cards contain a second paginator. Resolve its height
    /// with the same rem size so the outer row reserves space for its text.
    nested: Option<fn(Pixels, Pixels) -> Pixels>,
}

impl PaginatorSizing {
    pub fn rems(height: Rems) -> Self {
        Self { aspect_ratio: None, rems: height, pixels: Pixels::ZERO, padding: Pixels::ZERO, nested: None }
    }

    pub fn pixels(height: Pixels) -> Self {
        Self { aspect_ratio: None, rems: Rems::ZERO, pixels: height, padding: Pixels::ZERO, nested: None }
    }

    pub fn aspect_ratio(ratio: f32) -> Self {
        Self { aspect_ratio: Some(ratio), rems: Rems::ZERO, pixels: Pixels::ZERO, padding: Pixels::ZERO, nested: None }
    }

    pub fn with_rems(mut self, height: Rems) -> Self {
        self.rems = height;
        self
    }

    pub fn with_pixels(mut self, height: Pixels) -> Self {
        self.pixels = height;
        self
    }

    pub fn with_padding(mut self, padding: Pixels) -> Self {
        self.padding = padding;
        self
    }

    /// Adds the resolved height of a group laid out inside this child.
    pub fn with_nested(mut self, nested: fn(Pixels, Pixels) -> Pixels) -> Self {
        self.nested = Some(nested);
        self
    }

    pub fn height(self, width: Pixels, rem_size: Pixels) -> Pixels {
        let padding = f32::from(self.padding).max(0.0);
        let content_width = (f32::from(width) - padding * 2.0).max(0.0);
        let aspect_height = self.aspect_ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0).map(|ratio| content_width / ratio).unwrap_or_default();
        let nested = self.nested.map_or(0.0, |nested| f32::from(nested(width, rem_size)));
        px((aspect_height + nested + f32::from(self.rems.to_pixels(rem_size)) + f32::from(self.pixels) + padding * 2.0).max(1.0))
    }
}

/// The width a child is measured at before any stretching. It decides how
/// many columns fit; minimum column counts can force narrower children.
#[derive(Clone, Copy, PartialEq)]
pub struct PaginatorWidthPolicy {
    rems: Rems,
    fixed_pixels: Pixels,
}

impl PaginatorWidthPolicy {
    pub fn new(width: Rems) -> Self {
        Self { rems: Rems(width.0.max(0.0625)), fixed_pixels: Pixels::ZERO }
    }

    pub fn pixels(width: Pixels) -> Self {
        Self { rems: Rems::ZERO, fixed_pixels: px(f32::from(width).max(1.0)) }
    }

    pub fn width(self, rem_size: Pixels) -> Pixels {
        self.rems.to_pixels(rem_size) + self.fixed_pixels
    }
}

/// Geometry shared by all children in a group. Groups never share rows, so
/// their widths, sizing policies, and gaps resolve independently.
#[derive(Clone, Copy)]
pub struct PaginatorGroupPolicy {
    pub width: PaginatorWidthPolicy,
    pub intrinsic: bool,
    pub sizing: PaginatorSizing,
    pub row_gap: Pixels,
    /// Fewest columns the group will lay out, however narrow the window gets.
    pub minimum_columns: usize,
    /// Horizontal gap, defaulting to [`COLUMN_GAP`]. A group that wants its
    /// children packed tighter than the grid standard sets this; keeping it on
    /// the policy is what allows one group to change without dragging every
    /// other group along with it.
    pub column_gap: Pixels,
    /// Grow to this width, then put remaining slack into gaps. None is unbounded.
    pub maximum_width: Option<PaginatorWidthPolicy>,
    /// Whether a stretched row keeps its leftover slack at the edges. A group
    /// sitting under a heading turns this off, so the first child starts where
    /// the title does instead of a gap to the right of it.
    pub inset_edges: bool,
    /// A margin held at both edges of every row in the group, taken out of the
    /// width before the column count.
    ///
    /// Distinct from `inset_edges`, which only decides where a stretched row's
    /// leftover goes. This margin makes the row narrower rather than pushing
    /// children off the end.
    pub edge_inset: Pixels,
    /// Keep the gaps at their width and leave a row's slack after its last
    /// child, rather than spreading it into the gaps.
    pub pack_start: bool,
    /// Paint a shadow on the cell itself, outside its clipped child contents.
    pub item_shadow: bool,
    /// Stack this group's rows into columns of this fraction of the viewport.
    /// A strip containing such a group lays subsequent groups alongside it.
    pub column_fraction: Option<f32>,
}

impl PaginatorGroupPolicy {
    pub fn new(width: PaginatorWidthPolicy, sizing: PaginatorSizing, row_gap: Pixels) -> Self {
        Self {
            intrinsic: false,
            width,
            sizing,
            row_gap: px(f32::from(row_gap).max(0.0)),
            column_gap: px(COLUMN_GAP),
            maximum_width: None,
            minimum_columns: 1,
            inset_edges: true,
            edge_inset: Pixels::ZERO,
            pack_start: false,
            item_shadow: false,
            column_fraction: None,
        }
    }

    /// Let the outer cell cast a shadow into the gaps between its neighbors.
    pub fn with_item_shadow(mut self) -> Self {
        self.item_shadow = true;
        self
    }

    /// Wrap measured child widths with fixed gaps and no stretching.
    pub fn intrinsic(mut self) -> Self {
        self.intrinsic = true;
        self
    }

    /// Packs children against the row's start at their own width, with fixed
    /// gaps; whatever width is left over stays empty after the last child.
    pub fn pack_start(mut self) -> Self {
        self.pack_start = true;
        self.inset_edges = false;
        self
    }

    /// Limit card growth before expanding the gaps in wrapped groups.
    pub fn with_maximum_width(mut self, width: PaginatorWidthPolicy) -> Self {
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
    pub fn with_edge_inset(mut self, inset: Pixels) -> Self {
        self.edge_inset = px(f32::from(inset).max(0.0));
        self
    }

    /// Starts the row flush with the container, for a group whose heading sits
    /// directly above it.
    pub fn flush_edges(mut self) -> Self {
        self.inset_edges = false;
        self
    }

    /// Keeps at least `columns` children on a row even when the window is too
    /// narrow for them at their preferred width.
    pub fn with_minimum_columns(mut self, columns: usize) -> Self {
        self.minimum_columns = columns.max(1);
        self
    }

    /// Sets both gaps at once, for a group spaced equally in both directions.
    pub fn with_gap(mut self, gap: Pixels) -> Self {
        let gap = px(f32::from(gap).max(0.0));
        self.row_gap = gap;
        self.column_gap = gap;
        self
    }
}
