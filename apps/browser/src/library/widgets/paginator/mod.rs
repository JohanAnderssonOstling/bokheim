//! Group-aware paginator with declared child sizes.

use std::cell::{Cell, RefCell};
mod element;
mod geometry;
mod identity;
mod policy;

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::Duration;

#[cfg(feature = "mobile")]
use gpui::TouchPhase;
use gpui::prelude::*;
use gpui::{AnyElement, App, Bounds, Context, ElementId, Font, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, SharedString, Window, div, ease_in_out};
use ui_components::HierarchyItemWidth;
use web_time::Instant;

use element::PaginatorElement;
use identity::ItemPosition;
pub(crate) use identity::PaginatorPosition;
pub(crate) use policy::{PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy, book_card_policy, browse_section_card_policy, browse_section_item_policy, browse_shelf_card_policy, graph_cover_policy};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PaginatorAxis {
    Horizontal,
    Vertical,
}

/// One step of the keyboard cursor over the paginator's children.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorMove {
    Left,
    Right,
    Up,
    Down,
}

impl CursorMove {
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        match key {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            _ => None,
        }
    }
}

type RenderChild = dyn Fn(&PaginatorState, &mut Window, &mut App) -> AnyElement;
type PrepareChild = dyn Fn(&mut Window, &mut App);
type ActivateChild = dyn Fn(&mut Window, &mut App);
type ContextMenuHandler = dyn Fn(Option<SharedString>, Point<Pixels>, &mut Window, &mut App);

const PAGE_TRANSITION_DURATION: Duration = Duration::from_millis(140);
/// How far a press has to travel before it is a rubber band rather than a
/// click. Without it, every click on empty space would flicker a band.
const MARQUEE_THRESHOLD: f32 = 4.0;
#[cfg(feature = "mobile")]
const SWIPE_AXIS_LOCK_DISTANCE: f32 = 12.0;
#[cfg(feature = "mobile")]
const SWIPE_AXIS_DOMINANCE: f32 = 1.25;
#[cfg(feature = "mobile")]
const SWIPE_VIEWPORT_FRACTION: f32 = 0.12;
#[cfg(feature = "mobile")]
const SWIPE_MINIMUM_DISTANCE: f32 = 48.0;
#[cfg(feature = "mobile")]
const SWIPE_MAXIMUM_DISTANCE: f32 = 120.0;

#[cfg(feature = "mobile")]
#[derive(Clone, Copy, Default)]
enum SwipeAxis {
    #[default]
    Pending,
    Horizontal,
    Vertical,
}

#[cfg(feature = "mobile")]
impl SwipeAxis {
    fn paginator_axis(self) -> Option<PaginatorAxis> {
        match self {
            Self::Pending => None,
            Self::Horizontal => Some(PaginatorAxis::Horizontal),
            Self::Vertical => Some(PaginatorAxis::Vertical),
        }
    }
}

#[cfg(feature = "mobile")]
#[derive(Default)]
struct PaginatorSwipe {
    x: f32,
    y: f32,
    axis: SwipeAxis,
}

#[cfg(feature = "mobile")]
impl PaginatorSwipe {
    fn add(&mut self, x: f32, y: f32) {
        self.x += x;
        self.y += y;
        if !matches!(self.axis, SwipeAxis::Pending) {
            return;
        }

        let x = self.x.abs();
        let y = self.y.abs();
        if x.max(y) < SWIPE_AXIS_LOCK_DISTANCE {
            return;
        }
        if x >= y * SWIPE_AXIS_DOMINANCE {
            self.axis = SwipeAxis::Horizontal;
        } else if y >= x * SWIPE_AXIS_DOMINANCE {
            self.axis = SwipeAxis::Vertical;
        }
    }
}

/// What the paginator knows about its children this frame, handed to each child
/// as it is drawn. The paginator owns the cursor, the selection and the drag
/// state; a child only has to look.
#[derive(Default)]
pub(crate) struct PaginatorState {
    pub(crate) cursor: Option<usize>,
    pub(crate) selection: BTreeSet<usize>,
    /// Whether a rubber band is sweeping; children suppress hover during it.
    pub(crate) marquee: bool,
    data: Rc<PaginatorData>,
}

impl PaginatorState {
    pub(crate) fn is_selected(&self, id: &SharedString) -> bool {
        self.data.indices.get(id).is_some_and(|index| self.selection.contains(index))
    }
    pub(crate) fn is_focused(&self, id: &SharedString) -> bool {
        self.cursor.and_then(|index| self.data.children.get(index)).and_then(|child| child.id.as_ref()) == Some(id)
    }
    pub(crate) fn target_ids(&self, id: &SharedString) -> Vec<SharedString> {
        self.data.target_ids(id, &self.selection)
    }
}

struct CachedItemWidth {
    rem_size: Pixels,
    font: Font,
    width: Pixels,
}

struct IntrinsicWidth {
    content: HierarchyItemWidth,
    cache: RefCell<Option<CachedItemWidth>>,
}

impl IntrinsicWidth {
    fn measure(&self, window: &mut Window, cx: &mut App) -> Pixels {
        let rem_size = window.rem_size();
        let font = window.text_style().font();
        if let Some(cached) = self.cache.borrow().as_ref()
            && cached.rem_size == rem_size
            && cached.font == font
        {
            return cached.width;
        }
        let width = self.content.measure(window, cx);
        *self.cache.borrow_mut() = Some(CachedItemWidth { rem_size, font, width });
        width
    }
}

#[derive(Clone)]
pub(crate) struct PaginatorChild {
    id: Option<SharedString>,
    pub(super) activate: Option<Rc<ActivateChild>>,
    pub(super) prepare: Option<Rc<PrepareChild>>,
    pub(super) render: Rc<RenderChild>,
    intrinsic_width: Option<Rc<IntrinsicWidth>>,
    measured_width: Rc<Cell<Pixels>>,
}

impl PaginatorChild {
    pub(crate) fn new(render: impl Fn(&PaginatorState, &mut Window, &mut App) -> AnyElement + 'static) -> Self {
        Self { id: None, activate: None, prepare: None, render: Rc::new(render), intrinsic_width: None, measured_width: Rc::new(Cell::new(Pixels::ZERO)) }
    }

    /// Stable identity used to retain selection and focus when results reorder.
    pub(crate) fn with_id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Measures content without constructing off-page interactive children.
    pub(crate) fn with_intrinsic_width(mut self, content: HierarchyItemWidth) -> Self {
        self.intrinsic_width = Some(Rc::new(IntrinsicWidth { content, cache: RefCell::new(None) }));
        self
    }

    pub(crate) fn with_activate(mut self, activate: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.activate = Some(Rc::new(activate));
        self
    }

    /// Starts work needed to make this child ready before it becomes visible.
    /// The callback may be invoked repeatedly and must therefore be idempotent.
    pub(crate) fn with_prepare(mut self, prepare: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.prepare = Some(Rc::new(prepare));
        self
    }
}

pub(crate) struct PaginatorGroup {
    policy: PaginatorGroupPolicy,
    children: Vec<PaginatorChild>,
}

impl PaginatorGroup {
    pub(crate) fn new(policy: PaginatorGroupPolicy, children: impl IntoIterator<Item = PaginatorChild>) -> Self {
        Self { policy, children: children.into_iter().collect() }
    }
}

#[derive(Clone, Copy)]
pub(super) struct GroupSpan {
    pub(super) policy: PaginatorGroupPolicy,
    pub(super) start: usize,
    pub(super) end: usize,
}

impl GroupSpan {
    pub(super) fn len(self) -> usize {
        self.end - self.start
    }
}

#[derive(Default)]
pub(super) struct PaginatorData {
    indices: HashMap<SharedString, usize>,
    pub(super) groups: Vec<GroupSpan>,
    pub(super) children: Vec<PaginatorChild>,
}

impl PaginatorData {
    fn target_ids(&self, id: &SharedString, selection: &BTreeSet<usize>) -> Vec<SharedString> {
        if self.indices.get(id).is_some_and(|index| selection.contains(index)) { selection.iter().filter_map(|&index| self.children.get(index)?.id.clone()).collect() } else { vec![id.clone()] }
    }

    fn position(&self, index: usize) -> Option<ItemPosition> {
        Some(ItemPosition { id: self.children.get(index)?.id.clone(), index })
    }
    fn resolve(&self, position: &ItemPosition) -> Option<usize> {
        match &position.id {
            Some(id) => self.indices.get(id).copied(),
            None => (!self.children.is_empty()).then_some(position.index.min(self.children.len().saturating_sub(1))),
        }
    }

    fn new(groups: impl IntoIterator<Item = PaginatorGroup>) -> Self {
        let mut spans = Vec::new();
        let mut children = Vec::new();
        for group in groups {
            if group.children.is_empty() {
                continue;
            }
            let start = children.len();
            children.extend(group.children);
            spans.push(GroupSpan { policy: group.policy, start, end: children.len() });
        }
        let indices = children.iter().enumerate().filter_map(|(index, child)| child.id.clone().map(|id| (id, index))).collect();
        Self { indices, groups: spans, children }
    }

    pub(super) fn group_index_for_child(&self, child_index: usize) -> Option<usize> {
        self.groups.iter().position(|group| child_index >= group.start && child_index < group.end)
    }
}

/// A rubber band in flight. Both points are in window coordinates, which is
/// what the element reports its child bounds in.
#[derive(Clone, Copy)]
struct Marquee {
    anchor: Point<Pixels>,
    current: Point<Pixels>,
    /// A press only becomes a band once it has travelled far enough that it
    /// cannot be an ordinary click.
    active: bool,
}

impl Marquee {
    /// Corners are ordered here rather than by the caller: a band drawn up or
    /// to the left would otherwise have a negative size and intersect nothing.
    fn bounds(&self) -> Bounds<Pixels> {
        let top_left = gpui::point(self.anchor.x.min(self.current.x), self.anchor.y.min(self.current.y));
        let bottom_right = gpui::point(self.anchor.x.max(self.current.x), self.anchor.y.max(self.current.y));
        Bounds::from_corners(top_left, bottom_right)
    }
}

pub(crate) struct Paginator {
    id: SharedString,
    data: Rc<PaginatorData>,
    maximum_rows: Option<usize>,
    child_offset: usize,
    /// Index of the child holding the keyboard cursor, in flat child order.
    /// An index rather than a row and column, so it survives the window being
    /// resized under it — which changes the column count but not the order.
    cursor: Option<usize>,
    /// Height of the rows the visible page actually draws. Pages routinely come
    /// up short of the container — whole rows only — and the transition has to
    /// pan by this, not by the container, or the two pages separate.
    content_height: Option<Pixels>,
    direction: AssignmentDirection,
    visible_page: Option<VisiblePage>,
    children_revision: usize,
    prefetched: Option<PrefetchKey>,
    animate_page_transitions: bool,
    flow_axis: PaginatorAxis,
    allow_vertical_navigation: bool,
    transition_sequence: usize,
    transition: Option<PageTransition>,
    /// Opt-in: pages that have nothing to do with a selection should not grow
    /// mouse handlers for one.
    selectable: bool,
    /// Answers a secondary click on the selection, or on the space between
    /// children. The paginator knows where its children are; what they mean,
    /// and what a menu should offer, is the page's.
    on_context_menu: Option<Rc<ContextMenuHandler>>,
    marquee: Option<Marquee>,
    /// Children the band has picked up, in flat child order. Indices are valid
    /// only for the current children; every path that replaces them clears it.
    selection: BTreeSet<usize>,
    /// Where the last painted frame put each visible child, in window
    /// coordinates. The element is the only place this is known, so it reports
    /// it back for hit-testing between frames and for anchoring a menu on the
    /// keyboard cursor.
    child_bounds: Rc<Vec<(usize, Bounds<Pixels>)>>,
    #[cfg(feature = "mobile")]
    swipe: Option<PaginatorSwipe>,
}

#[derive(Clone)]
struct PageTransition {
    sequence: usize,
    direction: AssignmentDirection,
    axis: PaginatorAxis,
    outgoing_page: VisiblePage,
    outgoing_content_height: Option<Pixels>,
    started_at: Option<Instant>,
}

impl Paginator {
    pub(crate) fn new(id: impl Into<SharedString>, groups: impl IntoIterator<Item = PaginatorGroup>) -> Self {
        let data = Rc::new(PaginatorData::new(groups));
        Self {
            id: id.into(),
            data,
            maximum_rows: None,
            child_offset: 0,
            cursor: None,
            content_height: None,
            direction: AssignmentDirection::Forward,
            visible_page: None,
            children_revision: 0,
            prefetched: None,
            animate_page_transitions: !cfg!(feature = "kobo"),
            flow_axis: PaginatorAxis::Vertical,
            allow_vertical_navigation: true,
            transition_sequence: 0,
            transition: None,
            selectable: false,
            on_context_menu: None,
            marquee: None,
            selection: BTreeSet::new(),
            child_bounds: Rc::new(Vec::new()),
            #[cfg(feature = "mobile")]
            swipe: None,
        }
    }

    /// Enables rubber-band and keyboard selection over the children.
    pub(crate) fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    /// Handles a secondary click on the selection, with the child clicked, or
    /// on the space between children, with `None`.
    pub(crate) fn on_context_menu(mut self, handler: impl Fn(Option<SharedString>, Point<Pixels>, &mut Window, &mut App) + 'static) -> Self {
        self.on_context_menu = Some(Rc::new(handler));
        self
    }

    pub(crate) fn max_rows(mut self, maximum_rows: usize) -> Self {
        self.maximum_rows = Some(maximum_rows.max(1));
        self
    }

    pub(crate) fn flow_axis(mut self, axis: PaginatorAxis) -> Self {
        self.flow_axis = axis;
        self
    }

    pub(crate) fn horizontal_only(mut self) -> Self {
        self.allow_vertical_navigation = false;
        self
    }

    pub(crate) fn target_ids(&self, id: &SharedString) -> Vec<SharedString> {
        self.data.target_ids(id, &self.selection)
    }

    pub(crate) fn child_id(&self, index: usize) -> Option<&SharedString> {
        self.data.children.get(index)?.id.as_ref()
    }

    pub(crate) fn position(&self) -> PaginatorPosition {
        PaginatorPosition { cursor: self.cursor.and_then(|index| self.data.position(index)), anchor: self.data.position(self.child_anchor()), selection: self.selection.iter().filter_map(|&index| self.child_id(index).cloned()).collect() }
    }

    pub(crate) fn restore_position(&mut self, position: &PaginatorPosition, cx: &mut Context<Self>) {
        self.cursor = position.cursor.as_ref().and_then(|cursor| self.data.resolve(cursor));
        self.child_offset = position.anchor.as_ref().and_then(|anchor| self.data.resolve(anchor)).unwrap_or_else(|| position.anchor.as_ref().map_or(0, |anchor| anchor.index.min(self.data.children.len().saturating_sub(1))));
        self.selection = position.selection.iter().filter_map(|id| self.data.indices.get(id).copied()).collect();
        cx.notify();
    }

    /// Replaces the children while staying where the reader was.
    ///
    /// A background refresh — a finished download, a scanner batch — is not a
    /// navigation, so it must not send the reader back to the first page.
    /// Identified children retain their cursor and selection across reordering.
    /// Other children retain the positional offset and cursor.
    pub(crate) fn refresh_groups(&mut self, groups: impl IntoIterator<Item = PaginatorGroup>, cx: &mut Context<Self>) {
        let position = self.position();
        let direction = self.direction;
        // A page turn already in flight survives too, for the same reason. The
        // keystroke that walks the cursor onto the next page is followed by a
        // refresh — rebuilding the rows is what carries the move into their
        // headings — and cancelling here would leave that turn the only one
        // that never animates. The outgoing page's height rides along with it,
        // so a backward turn still pans by a measured page rather than a guess.
        let transition = self.transition.take();
        let content_height = self.content_height;
        self.set_groups(groups, cx);
        self.restore_position(&position, cx);
        self.direction = direction;
        self.transition = transition;
        self.content_height = content_height;
    }

    pub(crate) fn set_groups(&mut self, groups: impl IntoIterator<Item = PaginatorGroup>, cx: &mut Context<Self>) {
        self.data = Rc::new(PaginatorData::new(groups));
        self.child_offset = 0;
        // The children are different books now, so a retained index would point
        // the cursor at something the reader never put it on.
        self.cursor = None;
        self.selection.clear();
        self.marquee = None;
        self.child_bounds = Rc::new(Vec::new());
        self.content_height = None;
        self.direction = AssignmentDirection::Forward;
        self.visible_page = None;
        self.children_revision = self.children_revision.wrapping_add(1);
        self.prefetched = None;
        self.cancel_page_transition();
        #[cfg(feature = "mobile")]
        {
            self.swipe = None;
        }
        cx.notify();
    }

    pub(crate) fn previous_with_axis(&mut self, axis: PaginatorAxis, cx: &mut Context<Self>) -> bool {
        self.turn_page(AssignmentDirection::Backward, axis, cx)
    }

    pub(crate) fn next_with_axis(&mut self, axis: PaginatorAxis, cx: &mut Context<Self>) -> bool {
        self.turn_page(AssignmentDirection::Forward, axis, cx)
    }

    fn turn_page(&mut self, direction: AssignmentDirection, axis: PaginatorAxis, cx: &mut Context<Self>) -> bool {
        if !self.allows_axis(axis) || self.transition.is_some() {
            return false;
        }
        let Some(page) = self.visible_page else { return false };
        let Some(offset) = page.adjacent_offset(direction, self.data.children.len()) else { return false };
        self.clear_selection(cx);
        self.capture_page_transition(direction, page);
        self.child_offset = offset;
        self.direction = direction;
        self.visible_page = None;
        self.prefetched = None;
        cx.notify();
        true
    }

    pub(crate) fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub(crate) fn selection(&self) -> &BTreeSet<usize> {
        &self.selection
    }

    /// A keyboard menu acts on the selection before the previous cursor.
    pub(crate) fn context_menu_index(&self) -> Option<usize> {
        self.selection.iter().next().copied().or_else(|| self.cursor())
    }

    /// Where the last painted frame put a child, for anchoring a menu opened
    /// from the keyboard rather than from a pointer.
    pub(crate) fn child_bounds(&self, child_index: usize) -> Option<Bounds<Pixels>> {
        self.child_bounds.iter().find(|(index, _)| *index == child_index).map(|(_, bounds)| *bounds)
    }

    /// Drops the selection, reporting whether there was one. Every path that
    /// invalidates the indices — new children, a page turn, the keyboard cursor
    /// moving — goes through here.
    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if self.selection.is_empty() && self.marquee.is_none() {
            return false;
        }
        self.selection.clear();
        self.marquee = None;
        cx.notify();
        true
    }

    pub(super) fn set_child_bounds(&mut self, bounds: Vec<(usize, Bounds<Pixels>)>) {
        // Reported every frame from prepaint, so this deliberately does not
        // notify: it is a record of the frame that was just drawn, not a change
        // to anything drawn from.
        self.child_bounds = Rc::new(bounds);
    }

    pub(super) fn begin_marquee(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.clear_selection(cx);
        self.marquee = Some(Marquee { anchor: position, current: position, active: false });
    }

    pub(super) fn drag_marquee(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.as_mut() else { return };
        marquee.current = position;
        if !marquee.active {
            let travelled = f32::from(position.x - marquee.anchor.x).abs().max(f32::from(position.y - marquee.anchor.y).abs());
            if travelled < MARQUEE_THRESHOLD {
                return;
            }
            marquee.active = true;
        }
        let band = marquee.bounds();
        self.selection = self.child_bounds.iter().filter(|(_, bounds)| bounds.intersects(&band)).map(|(index, _)| *index).collect();
        cx.notify();
    }

    /// Ends the drag, keeping whatever it picked up.
    pub(super) fn end_marquee(&mut self, cx: &mut Context<Self>) {
        if self.marquee.take().is_some() {
            cx.notify();
        }
    }

    fn marquee_bounds(&self) -> Option<Bounds<Pixels>> {
        self.marquee.filter(|marquee| marquee.active).map(|marquee| marquee.bounds())
    }

    /// Whether a rubber-band drag is sweeping, suppressing child hover.
    pub(crate) fn marquee_active(&self) -> bool {
        self.marquee.is_some_and(|marquee| marquee.active)
    }

    fn child_anchor(&self) -> usize {
        self.visible_page.map_or(self.child_offset, |page| page.first_child)
    }

    /// Columns a group is laid out at, for the width the element last used.
    ///
    /// Geometry is a pure function of width and rem size, and the prefetch key
    /// the element already reports carries both, so cursor movement needs
    /// nothing published beyond what the element publishes today.
    fn columns_for_group(&self, group_index: usize) -> usize {
        let Some(key) = self.prefetched.as_ref() else {
            return 1;
        };
        geometry::group_geometries(&self.data, key.width, key.rem_size).get(group_index).map_or(1, |geometry| geometry.columns.max(1))
    }

    /// Moves the keyboard cursor, turning the page when it walks off the edge.
    /// Returns whether anything moved, so the caller knows to swallow the key.
    pub(crate) fn move_cursor(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        if self.data.children.is_empty() || self.transition.is_some() {
            return false;
        }
        // Arrow keys hand the page back to the cursor: one item, moved
        // deliberately, rather than whatever a band happened to sweep up.
        self.clear_selection(cx);
        let Some(current) = self.cursor else {
            // The first arrow key places the cursor rather than moving it, so
            // no keystroke is spent adopting a position the reader can see.
            return self.place_cursor(self.visible_page.map_or(0, |page| page.first_child), cx);
        };
        let Some(target) = self.cursor_target(current, movement) else {
            return false;
        };
        self.place_cursor(target, cx)
    }

    /// Moves the cursor and toggles only the destination, keeping other picks.
    pub(crate) fn move_and_toggle_selection(&mut self, movement: CursorMove, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.toggle_cursor_selection(movement) else {
            return false;
        };
        // Cursor-driven page turns normally clear selection. Keep it while
        // revealing the destination so keyboard picks can span pages.
        let selection = std::mem::take(&mut self.selection);
        self.reveal_cursor(target, cx);
        self.selection = selection;
        cx.notify();
        true
    }

    fn toggle_cursor_selection(&mut self, movement: CursorMove) -> Option<usize> {
        if !self.selectable || self.data.children.is_empty() || self.transition.is_some() {
            return None;
        }
        let target = match self.cursor {
            Some(current) => self.cursor_target(current, movement)?,
            None => self.visible_page.map_or(0, |page| page.first_child),
        };
        self.cursor = Some(target);
        self.marquee = None;
        if !self.selection.remove(&target) {
            self.selection.insert(target);
        }
        Some(target)
    }

    fn cursor_target(&self, current: usize, movement: CursorMove) -> Option<usize> {
        let last = self.data.children.len().checked_sub(1)?;
        match movement {
            // Left and right walk the flat child order, so stepping right off
            // the last folder lands on the first book: reading order, and the
            // order the groups were declared in.
            CursorMove::Left => current.checked_sub(1),
            CursorMove::Right => (current < last).then_some(current + 1),
            CursorMove::Up | CursorMove::Down => self.vertical_cursor_target(current, movement, last),
        }
    }

    /// Vertical movement is by the column count of the cursor's own group.
    ///
    /// Groups carry their own widths — folder chips are wider than book cards —
    /// so their rows do not line up, and crossing a boundary cannot be a fixed
    /// stride. The column within the row is preserved as closely as the group
    /// being entered allows.
    fn vertical_cursor_target(&self, current: usize, movement: CursorMove, last: usize) -> Option<usize> {
        if self.data.groups.iter().any(|group| group.policy.intrinsic) {
            let key = self.prefetched.as_ref()?;
            let geometries = geometry::group_geometries(&self.data, key.width, key.rem_size);
            // Match the visible page's packing, including a restored partial row.
            let start = self.visible_page.map_or(0, |page| page.first_child);
            let mut rows = if start == 0 { Vec::new() } else { geometry::page_rows(&self.data, &geometries, AssignmentDirection::Backward, start - 1, self.maximum_rows, key.height, key.rem_size) };
            rows.extend(geometry::forward_rows_in_range(&self.data, &geometries, start, self.data.children.len()));
            let row_index = rows.iter().position(|row| row.items.iter().any(|item| item.child_index == current))?;
            let current_item = rows[row_index].items.iter().find(|item| item.child_index == current)?;
            let center = current_item.x + current_item.width / 2.0;
            let target_row = match movement {
                CursorMove::Up => row_index.checked_sub(1)?,
                CursorMove::Down => row_index + 1,
                _ => return None,
            };
            return rows
                .get(target_row)?
                .items
                .iter()
                .min_by(|a, b| {
                    let distance = |item: &geometry::PageItem| f32::from(item.x + item.width / 2.0 - center).abs();
                    distance(a).total_cmp(&distance(b))
                })
                .map(|item| item.child_index);
        }
        let group_index = self.data.group_index_for_child(current)?;
        let group = self.data.groups[group_index];
        let columns = self.columns_for_group(group_index);
        let column = (current - group.start) % columns;
        match movement {
            CursorMove::Up => {
                if current >= group.start + columns {
                    return Some(current - columns);
                }
                let previous_index = group_index.checked_sub(1)?;
                let previous = *self.data.groups.get(previous_index)?;
                let previous_columns = self.columns_for_group(previous_index);
                let last_row_start = previous.start + (previous.len().div_ceil(previous_columns) - 1) * previous_columns;
                Some((last_row_start + column.min(previous_columns - 1)).min(previous.end - 1))
            }
            CursorMove::Down => {
                let target = current + columns;
                if target < group.end {
                    return Some(target);
                }
                match self.data.groups.get(group_index + 1) {
                    Some(next) => {
                        let next_columns = self.columns_for_group(group_index + 1);
                        Some((next.start + column.min(next_columns - 1)).min(next.end - 1))
                    }
                    // Nothing below: settle on the final child rather than
                    // refusing, so a short last row is still reachable.
                    None => (current < last).then_some(last),
                }
            }
            CursorMove::Left | CursorMove::Right => None,
        }
    }

    /// Runs the cursor child's activation — the same callback a click runs, so
    /// Enter and a click cannot diverge.
    /// What Enter on the cursor would do, handed out rather than run.
    ///
    /// Activating a child navigates, and navigation can complete synchronously —
    /// a location that needs no query, such as a book's own page — which comes
    /// straight back through the page to the paginator to install new groups.
    /// Running the activation while the paginator is leased is a panic, so the
    /// caller takes this, lets go, and only then calls it.
    pub(crate) fn cursor_activation(&self) -> Option<Rc<ActivateChild>> {
        self.data.children.get(self.cursor?).and_then(|child| child.activate.clone())
    }

    fn place_cursor(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let Some(last) = self.data.children.len().checked_sub(1) else {
            return false;
        };
        let index = index.min(last);
        if self.cursor == Some(index) {
            return false;
        }
        self.cursor = Some(index);
        self.reveal_cursor(index, cx);
        cx.notify();
        true
    }

    fn reveal_cursor(&mut self, index: usize, cx: &mut Context<Self>) {
        // Walking off the page is what turns it: one step past the last row
        // lands on the first row of the next page, so paging is a consequence
        // of moving rather than a second gesture.
        if let Some(page) = self.visible_page {
            if index < page.first_child {
                self.previous_with_axis(self.flow_axis, cx);
            } else if index > page.last_child {
                self.next_with_axis(self.flow_axis, cx);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.data.children.is_empty()
    }

    /// Whether a measured page exists before the page currently on screen.
    /// Unlike `can_previous`, this remains stable while a transition runs.
    pub(crate) fn has_previous(&self) -> bool {
        self.visible_page.or_else(|| self.transition.as_ref().map(|transition| transition.outgoing_page)).is_some_and(|page| page.first_child > 0)
    }

    /// Whether a measured page exists after the page currently on screen.
    /// Unlike `can_next`, this remains stable while a transition runs.
    pub(crate) fn has_next(&self) -> bool {
        self.visible_page.or_else(|| self.transition.as_ref().map(|transition| transition.outgoing_page)).is_some_and(|page| page.last_child.saturating_add(1) < self.data.children.len())
    }

    /// Whether the page on screen is measured and holds the last child.
    pub(crate) fn on_last_page(&self) -> bool {
        self.visible_page.is_some_and(|page| page.last_child.saturating_add(1) >= self.data.children.len())
    }

    fn set_visible_page(&mut self, children_revision: usize, visible_page: Option<VisiblePage>, cx: &mut Context<Self>) {
        if !self.update_visible_page(children_revision, visible_page) {
            return;
        }
        self.maybe_start_page_transition(cx);
        cx.notify();
    }

    fn allows_axis(&self, axis: PaginatorAxis) -> bool {
        matches!(axis, PaginatorAxis::Horizontal) || self.allow_vertical_navigation
    }

    fn capture_page_transition(&mut self, direction: AssignmentDirection, page: VisiblePage) {
        if !self.animate_page_transitions {
            return;
        }
        self.transition_sequence = self.transition_sequence.wrapping_add(1);
        self.transition = Some(PageTransition { sequence: self.transition_sequence, direction, axis: self.flow_axis, outgoing_page: page, outgoing_content_height: self.content_height, started_at: None });
    }

    fn maybe_start_page_transition(&mut self, cx: &mut Context<Self>) {
        // The element reports a visible page only once it has laid one out,
        // so this being set is the signal that the incoming page is ready.
        if self.visible_page.is_none() {
            return;
        }
        let Some(transition) = self.transition.as_mut() else {
            return;
        };
        if transition.started_at.is_some() {
            return;
        }
        if cx.reduce_motion() {
            self.cancel_page_transition();
            return;
        }

        transition.started_at = Some(Instant::now());
    }

    fn cancel_page_transition(&mut self) {
        self.transition = None;
    }

    #[cfg(not(feature = "mobile"))]
    fn handle_mouse_wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.flow_axis != PaginatorAxis::Vertical || !self.allows_axis(PaginatorAxis::Vertical) {
            return;
        }
        let delta = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y),
            ScrollDelta::Lines(delta) => delta.y,
        };
        if !delta.is_finite() || delta == 0.0 {
            return;
        }

        // One page per completed animation. A wheel event that lands while a
        // turn is still playing is ignored outright rather than remembered:
        // the reader sees the page they asked for, and scrolling on simply
        // takes effect the next time an event arrives with nothing in flight.
        //
        // Continuous scrolling therefore advances at the animation's pace,
        // which is what paces it — not a timer.
        if self.transition.is_some() {
            cx.stop_propagation();
            return;
        }
        match wheel_direction(delta) {
            Some(AssignmentDirection::Forward) => {
                self.next_with_axis(PaginatorAxis::Vertical, cx);
            }
            Some(AssignmentDirection::Backward) => {
                self.previous_with_axis(PaginatorAxis::Vertical, cx);
            }
            None => {}
        }
        cx.stop_propagation();
    }

    fn update_visible_page(&mut self, children_revision: usize, visible_page: Option<VisiblePage>) -> bool {
        if self.children_revision != children_revision {
            return false;
        }
        let consumed_backward = matches!(self.direction, AssignmentDirection::Backward) && visible_page.is_some();
        if consumed_backward {
            self.child_offset = visible_page.expect("checked above").first_child;
            self.direction = AssignmentDirection::Forward;
        }
        if self.visible_page == visible_page && !consumed_backward {
            return false;
        }
        self.visible_page = visible_page;
        // A cursor the reader can no longer see is not a selection. Turning the
        // page with the wheel leaves it behind, so it is dropped here rather
        // than dragged along invisibly; walking the cursor off the edge turns
        // the page to follow it, so that cursor lands inside the new page and
        // is kept.
        if let (Some(cursor), Some(page)) = (self.cursor, visible_page)
            && (cursor < page.first_child || cursor > page.last_child)
        {
            self.cursor = None;
        }
        true
    }

    fn set_content_height(&mut self, children_revision: usize, content_height: Pixels, cx: &mut Context<Self>) {
        if self.children_revision != children_revision || self.content_height == Some(content_height) {
            return;
        }
        self.content_height = Some(content_height);
        cx.notify();
    }

    fn set_prefetched(&mut self, key: PrefetchKey, cx: &mut Context<Self>) {
        if !self.update_prefetched(key) {
            return;
        }
        cx.notify();
    }

    fn update_prefetched(&mut self, key: PrefetchKey) -> bool {
        if self.children_revision != key.children_revision || self.prefetched.as_ref() == Some(&key) {
            return false;
        }
        self.prefetched = Some(key);
        true
    }

    #[cfg(feature = "mobile")]
    fn handle_swipe(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ScrollDelta::Pixels(delta) = event.delta else {
            if matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.swipe = None;
            }
            return;
        };
        let x = f32::from(delta.x);
        let y = f32::from(delta.y);

        match event.touch_phase {
            TouchPhase::Started => {
                let mut swipe = PaginatorSwipe::default();
                swipe.add(x, y);
                let consume = swipe.axis.paginator_axis().is_some_and(|axis| self.allows_axis(axis));
                self.swipe = Some(swipe);
                if consume {
                    cx.stop_propagation();
                }
            }
            TouchPhase::Moved => {
                let Some(swipe) = self.swipe.as_mut() else {
                    return;
                };
                swipe.add(x, y);
                if swipe.axis.paginator_axis().is_some_and(|axis| self.allows_axis(axis)) {
                    cx.stop_propagation();
                }
            }
            TouchPhase::Ended => {
                let Some(swipe) = self.swipe.take() else {
                    return;
                };
                let Some(axis) = swipe.axis.paginator_axis().filter(|axis| self.allows_axis(*axis)) else {
                    return;
                };

                let (distance, viewport_extent) = match axis {
                    PaginatorAxis::Horizontal => (swipe.x, f32::from(window.viewport_size().width)),
                    PaginatorAxis::Vertical => (swipe.y, f32::from(window.viewport_size().height)),
                };
                let threshold = (viewport_extent * SWIPE_VIEWPORT_FRACTION).clamp(SWIPE_MINIMUM_DISTANCE, SWIPE_MAXIMUM_DISTANCE);
                if distance.abs() >= threshold {
                    if distance < 0.0 {
                        self.next_with_axis(axis, cx);
                    } else {
                        self.previous_with_axis(axis, cx);
                    }
                }
                cx.stop_propagation();
            }
            TouchPhase::Cancelled => {
                if self.swipe.take().and_then(|swipe| swipe.axis.paginator_axis()).is_some_and(|axis| self.allows_axis(axis)) {
                    cx.stop_propagation();
                }
            }
        }
    }
}

impl Render for Paginator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = ElementId::from(self.id.clone());
        let child_count = self.data.children.len();
        let child_offset = self.child_offset.min(child_count.saturating_sub(1));
        let rendered_transition = self.transition.clone().and_then(|transition| {
            let progress = transition.started_at.map(|started_at| started_at.elapsed().as_secs_f32() / PAGE_TRANSITION_DURATION.as_secs_f32());
            if progress.is_some_and(|progress| progress >= 1.0) {
                self.transition = None;
                cx.notify();
                None
            } else {
                if progress.is_some() {
                    window.request_animation_frame();
                }
                Some((transition, progress.map(|progress| ease_in_out(progress.clamp(0.0, 1.0)))))
            }
        });
        // Vertical transitions pan: both pages move by the height of the page
        // that lies between the two viewport positions, so they stay butted
        // together. Horizontal ones still slide by a fraction of the container,
        // where a page always fills the axis it moves along.
        let pan = rendered_transition.as_ref().and_then(|(transition, progress)| {
            let progress = (*progress)?;
            if transition.axis != PaginatorAxis::Vertical {
                return None;
            }
            let travel = match transition.direction {
                AssignmentDirection::Forward => transition.outgoing_content_height,
                AssignmentDirection::Backward => self.content_height,
            }?;
            let (outgoing, incoming) = page_transition_offsets(transition.direction, progress);
            Some((travel * outgoing, travel * incoming))
        });
        let incoming_offset = match pan {
            Some(_) => (0.0, 0.0),
            None => rendered_transition.as_ref().and_then(|(transition, progress)| progress.map(|progress| page_transition_vector(transition.axis, page_transition_offsets(transition.direction, progress).1))).unwrap_or((0.0, 0.0)),
        };
        let state = Rc::new(PaginatorState { cursor: self.cursor, selection: self.selection.clone(), marquee: self.marquee_active(), data: self.data.clone() });
        let incoming = PaginatorElement::new(
            ElementId::named_usize(format!("{id}-layout"), self.children_revision),
            cx.entity().downgrade(),
            self.visible_page,
            self.children_revision,
            self.data.clone(),
            self.maximum_rows,
            child_offset,
            self.direction,
            self.prefetched.clone(),
            None,
            window.rem_size(),
            self.flow_axis,
            state.clone(),
        )
        .with_selection(self.selectable, self.marquee_bounds())
        .with_page_offset(incoming_offset.0, incoming_offset.1)
        .with_page_pixels(pan.map(|(_, incoming)| incoming).unwrap_or(Pixels::ZERO), self.content_height);

        let mut content = div().relative().w_full().min_h_0().min_w_0().overflow_hidden().when(self.maximum_rows.is_none(), |content| content.h_full().flex_1());
        if let Some((transition, progress)) = rendered_transition {
            let outgoing_offset = match pan {
                Some(_) => (0.0, 0.0),
                None => progress.map(|progress| page_transition_vector(transition.axis, page_transition_offsets(transition.direction, progress).0)).unwrap_or((0.0, 0.0)),
            };
            let outgoing = PaginatorElement::new(
                ElementId::named_usize(format!("{id}-outgoing-layout"), transition.sequence),
                cx.entity().downgrade(),
                Some(transition.outgoing_page),
                self.children_revision,
                self.data.clone(),
                self.maximum_rows,
                transition.outgoing_page.first_child,
                AssignmentDirection::Forward,
                None,
                Some(transition.outgoing_page),
                window.rem_size(),
                self.flow_axis,
                state,
            )
            .passive()
            .with_page_offset(outgoing_offset.0, outgoing_offset.1)
            .with_page_pixels(pan.map(|(outgoing, _)| outgoing).unwrap_or(Pixels::ZERO), transition.outgoing_content_height);

            let incoming_page = div().relative().w_full().min_h_0().min_w_0().when(self.maximum_rows.is_none(), |page| page.h_full().flex_1()).child(incoming);
            let outgoing_page = div().absolute().inset_0().size_full().min_h_0().min_w_0().child(outgoing);
            if progress.is_some() {
                content = content.child(outgoing_page).child(incoming_page);
            } else {
                // Keep laying out the incoming page so it can start cover
                // preparation, but do not reveal it before the transition.
                content = content.child(outgoing_page).child(incoming_page.invisible());
            }
        } else {
            content = content.child(incoming);
        }

        let paginator = div().id(id).w_full().min_h_0().min_w_0().flex().flex_col().when(self.maximum_rows.is_none(), |paginator| paginator.h_full().flex_1());
        #[cfg(not(feature = "mobile"))]
        let paginator = paginator.on_scroll_wheel(cx.listener(Self::handle_mouse_wheel));
        #[cfg(feature = "mobile")]
        let paginator = paginator.on_scroll_wheel(cx.listener(Self::handle_swipe));
        paginator.child(content)
    }
}

#[cfg(not(feature = "mobile"))]
fn wheel_direction(delta: f32) -> Option<AssignmentDirection> {
    if delta < 0.0 {
        Some(AssignmentDirection::Forward)
    } else if delta > 0.0 {
        Some(AssignmentDirection::Backward)
    } else {
        None
    }
}

fn page_transition_offsets(direction: AssignmentDirection, progress: f32) -> (f32, f32) {
    match direction {
        AssignmentDirection::Forward => (-progress, 1.0 - progress),
        AssignmentDirection::Backward => (progress, progress - 1.0),
    }
}

fn page_transition_vector(axis: PaginatorAxis, offset: f32) -> (f32, f32) {
    match axis {
        PaginatorAxis::Horizontal => (offset, 0.0),
        PaginatorAxis::Vertical => (0.0, offset),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AssignmentDirection {
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VisiblePage {
    pub(super) first_child: usize,
    pub(super) last_child: usize,
}

impl VisiblePage {
    fn adjacent_offset(self, direction: AssignmentDirection, child_count: usize) -> Option<usize> {
        match direction {
            AssignmentDirection::Backward => self.first_child.checked_sub(1),
            AssignmentDirection::Forward => {
                let offset = self.last_child.checked_add(1)?;
                (offset < child_count).then_some(offset)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PrefetchKey {
    pub(super) children_revision: usize,
    pub(super) page: VisiblePage,
    pub(super) width: Pixels,
    pub(super) height: Pixels,
    pub(super) rem_size: Pixels,
}

#[cfg(test)]
mod tests {
    use super::{AssignmentDirection, CursorMove, Paginator, PaginatorChild, PaginatorData, PaginatorGroup, PaginatorGroupPolicy, PaginatorSizing, PaginatorWidthPolicy, PrefetchKey, VisiblePage};
    use gpui::px;
    use std::collections::BTreeSet;
    use std::rc::Rc;

    fn selection_grid(count: usize) -> Paginator {
        let policy = PaginatorGroupPolicy::new(PaginatorWidthPolicy::pixels(px(100.0)), PaginatorSizing::pixels(px(150.0)), px(0.0)).with_minimum_columns(4);
        let children = (0..count).map(|index| PaginatorChild::new(|_, _, _| unreachable!()).with_id(index.to_string()));
        let mut paginator = Paginator::new("selection-test", [PaginatorGroup::new(policy, children)]).selectable();
        let page = VisiblePage { first_child: 0, last_child: count.saturating_sub(1) };
        paginator.visible_page = Some(page);
        paginator.prefetched = Some(PrefetchKey { children_revision: 0, page, width: px(400.0), height: px(600.0), rem_size: px(16.0) });
        paginator
    }

    #[test]
    fn keyboard_selection_vertical_moves_toggle_only_destination() {
        let mut paginator = selection_grid(12);
        paginator.cursor = Some(1);
        paginator.selection = BTreeSet::from([1, 10]);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Down), Some(5));
        assert_eq!(paginator.cursor(), Some(5));
        assert_eq!(paginator.selection(), &BTreeSet::from([1, 5, 10]));
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Up), Some(1));
        assert_eq!(paginator.selection(), &BTreeSet::from([5, 10]));
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Down), Some(5));
        assert_eq!(paginator.selection(), &BTreeSet::from([10]));
    }

    #[test]
    fn keyboard_selection_horizontal_moves_toggle_existing_picks() {
        let mut paginator = selection_grid(12);
        paginator.cursor = Some(1);
        paginator.selection = BTreeSet::from([10]);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Right), Some(2));
        assert_eq!(paginator.selection(), &BTreeSet::from([2, 10]));
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Left), Some(1));
        assert_eq!(paginator.selection(), &BTreeSet::from([1, 2, 10]));
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Right), Some(2));
        assert_eq!(paginator.selection(), &BTreeSet::from([1, 10]));
    }

    #[test]
    fn keyboard_selection_boundaries_leave_selection_unchanged() {
        let mut paginator = selection_grid(12);
        paginator.cursor = Some(0);
        paginator.selection = BTreeSet::from([0, 8]);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Left), None);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Up), None);
        assert_eq!(paginator.cursor(), Some(0));
        assert_eq!(paginator.selection(), &BTreeSet::from([0, 8]));
        paginator.cursor = Some(11);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Right), None);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Down), None);
        assert_eq!(paginator.selection(), &BTreeSet::from([0, 8]));
    }

    #[test]
    fn keyboard_selection_starts_on_visible_page_and_handles_short_rows() {
        let mut paginator = selection_grid(10);
        paginator.visible_page = Some(VisiblePage { first_child: 4, last_child: 9 });
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Right), Some(4));
        assert_eq!(paginator.selection(), &BTreeSet::from([4]));
        paginator.cursor = Some(7);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Down), Some(9));
        assert_eq!(paginator.selection(), &BTreeSet::from([4, 9]));
    }

    #[test]
    fn keyboard_selection_requires_selectable_nonempty_grid() {
        let mut paginator = selection_grid(0);
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Right), None);
        let mut paginator = selection_grid(4);
        paginator.selectable = false;
        assert_eq!(paginator.toggle_cursor_selection(CursorMove::Right), None);
        assert_eq!(paginator.cursor(), None);
        assert!(paginator.selection().is_empty());
    }

    #[test]
    fn keyboard_menu_targets_selection_instead_of_previous_cursor() {
        let mut paginator = Paginator::new("menu-test", []);
        let ids = ["a", "b", "c"].map(gpui::SharedString::from);
        paginator.data = Rc::new(PaginatorData {
            indices: ids.iter().cloned().enumerate().map(|(index, id)| (id, index)).collect(),
            children: ids.iter().cloned().map(|id| PaginatorChild::new(|_, _, _| unreachable!()).with_id(id)).collect(),
            groups: Vec::new(),
        });
        paginator.cursor = Some(2);
        paginator.selection = BTreeSet::from([0, 1]);
        let index = paginator.context_menu_index().unwrap();
        assert_eq!(paginator.target_ids(paginator.child_id(index).unwrap()), ids[..2]);
        paginator.selection.clear();
        assert_eq!(paginator.context_menu_index(), Some(2));
        paginator.cursor = None;
        assert_eq!(paginator.context_menu_index(), None);
    }

    #[test]
    fn adjacent_pages_stop_at_first_and_last_children() {
        let first = VisiblePage { first_child: 0, last_child: 2 };
        assert_eq!(first.adjacent_offset(AssignmentDirection::Backward, 8), None);
        assert_eq!(first.adjacent_offset(AssignmentDirection::Forward, 8), Some(3));
        let last = VisiblePage { first_child: 6, last_child: 7 };
        assert_eq!(last.adjacent_offset(AssignmentDirection::Backward, 8), Some(5));
        assert_eq!(last.adjacent_offset(AssignmentDirection::Forward, 8), None);
    }

    #[test]
    fn adjacent_page_offsets_follow_measured_page_boundaries() {
        let page = VisiblePage { first_child: 3, last_child: 5 };
        assert_eq!(page.adjacent_offset(AssignmentDirection::Backward, 8), Some(2));
        assert_eq!(page.adjacent_offset(AssignmentDirection::Forward, 8), Some(6));
    }

    #[test]
    fn overflowing_page_boundary_has_no_next_page() {
        let page = VisiblePage { first_child: 0, last_child: usize::MAX };
        assert_eq!(page.adjacent_offset(AssignmentDirection::Forward, usize::MAX), None);
    }
}
