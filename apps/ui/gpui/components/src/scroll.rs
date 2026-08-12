use std::panic::Location;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Div, Element, ElementId, Global, InteractiveElement, IntoElement, ParentElement, RenderOnce, ScrollHandle, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Subscription, UniformList, UniformListScrollHandle, Window,
    div,
};
use gpui_component::scroll::{Scrollbar, ScrollbarAxis};
use gpui_component::{StyledExt, VirtualListScrollHandle};

struct RegisteredScroll {
    id: ElementId,
    handle: ScrollHandle,
}

#[derive(Default)]
struct PageScrollRegistry {
    scrolls: Vec<RegisteredScroll>,
    _keystroke_subscription: Option<Subscription>,
}

impl Global for PageScrollRegistry {}

/// Installs the fallback that routes unhandled left/right keys to the most
/// recently rendered vertical scroll surface. Reader key bindings run first,
/// so their page-turn semantics remain authoritative.
pub fn init_page_scrolling(cx: &mut App) {
    if cx.has_global::<PageScrollRegistry>() {
        return;
    }
    cx.set_global(PageScrollRegistry::default());
    let subscription = cx.observe_keystrokes(|event, window, cx| {
        if event.action.is_some() || event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }
        let Some(direction) = page_direction(&event.keystroke.key) else {
            return;
        };
        let handle = cx.global::<PageScrollRegistry>().scrolls.iter().rev().find_map(|registered| {
            let bounds = registered.handle.bounds();
            (bounds.size.height > gpui::Pixels::ZERO && registered.handle.max_offset().y > gpui::Pixels::ZERO).then(|| registered.handle.clone())
        });
        if let Some(handle) = handle {
            page_scroll(&handle, direction, window, cx);
        }
    });
    cx.global_mut::<PageScrollRegistry>()._keystroke_subscription = Some(subscription);
}

/// Registers an externally managed vertical scroll handle for application-wide
/// left/right paging.
pub fn register_page_scroll_handle(id: impl Into<ElementId>, handle: ScrollHandle, cx: &mut App) {
    if !cx.has_global::<PageScrollRegistry>() {
        return;
    }
    let id = id.into();
    let registry = cx.global_mut::<PageScrollRegistry>();
    registry.scrolls.retain(|registered| registered.id != id);
    registry.scrolls.push(RegisteredScroll { id, handle });
}

/// Adds Bokheim's keyboard paging behavior to a scrollable element.
pub trait ScrollableElement: InteractiveElement + Styled + ParentElement + Element {
    /// Adds a vertical scrollbar. Left pages up and right pages down by the
    /// scroll area's measured viewport height.
    #[track_caller]
    fn overflow_y_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Vertical)
    }

    /// Adds a vertical scrollbar with a stable identity supplied by the
    /// caller. This is useful for repeated scroll surfaces rendered from the
    /// same source location, such as the columns in a folder picker.
    fn overflow_y_scrollbar_with_id(self, id: impl Into<ElementId>) -> Scrollable<Self> {
        Scrollable::with_id(self, ScrollbarAxis::Vertical, id.into())
    }
}

/// A scrollable container with an overlaid scrollbar and viewport paging.
#[derive(IntoElement)]
pub struct Scrollable<E: InteractiveElement + Styled + ParentElement + Element> {
    id: ElementId,
    element: E,
    axis: ScrollbarAxis,
}

impl<E> Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    #[track_caller]
    fn new(element: E, axis: ScrollbarAxis) -> Self {
        Self { id: ElementId::CodeLocation(*Location::caller()), element, axis }
    }

    fn with_id(element: E, axis: ScrollbarAxis, id: ElementId) -> Self {
        Self { id, element, axis }
    }
}

impl<E> Styled for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<E> ParentElement for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.element.extend(elements);
    }
}

impl<E> InteractiveElement for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.element.interactivity()
    }
}

impl<E> RenderOnce for Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element + 'static,
{
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll_handle = window.use_keyed_state(self.id.clone(), cx, |_, _| ScrollHandle::default()).read(cx).clone();
        register_page_scroll_handle(self.id.clone(), scroll_handle.clone(), cx);
        let root_style = outer_layout_style(&mut self.element);

        let content = self.element.id((self.id.clone(), "content")).flex_none().map(|this| match self.axis {
            ScrollbarAxis::Vertical => this.h_auto().min_h_full(),
            ScrollbarAxis::Horizontal => this.w_auto().min_w_full(),
            ScrollbarAxis::Both => this.size_auto().min_size_full(),
        });
        let scroll_area = div()
            .id((self.id.clone(), "area"))
            .size_full()
            .flex()
            .track_scroll(&scroll_handle)
            .map(|this| match self.axis {
                ScrollbarAxis::Vertical => this.flex_col().overflow_y_scroll(),
                ScrollbarAxis::Horizontal => this.flex_row().overflow_x_scroll(),
                ScrollbarAxis::Both => this.overflow_scroll(),
            })
            .child(content);

        div().id(self.id.clone()).size_full().refine_style(&root_style).relative().on_key_down(page_with_arrow_keys(scroll_handle.clone())).child(scroll_area).child(Scrollbar::new(&scroll_handle).id((self.id, "scrollbar")).axis(self.axis))
    }
}

/// A viewport-filling uniform list with Bokheim's keyboard paging behavior.
#[derive(IntoElement)]
pub struct VirtualList {
    id: ElementId,
    list: UniformList,
}

impl VirtualList {
    #[track_caller]
    fn new(list: UniformList) -> Self {
        Self { id: ElementId::CodeLocation(*Location::caller()), list }
    }
}

impl Styled for VirtualList {
    fn style(&mut self) -> &mut StyleRefinement {
        self.list.style()
    }
}

impl RenderOnce for VirtualList {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = window.use_keyed_state(self.id.clone(), cx, |_, _| UniformListScrollHandle::new()).read(cx).clone();
        let base_handle = handle.0.borrow().base_handle.clone();
        register_page_scroll_handle(self.id, base_handle.clone(), cx);
        self.list.flex_1().track_scroll(&handle).on_key_down(page_with_arrow_keys(base_handle))
    }
}

#[track_caller]
pub fn virtual_list(list: UniformList) -> VirtualList {
    VirtualList::new(list)
}

/// A viewport-filling virtual list whose rows each declare their own height,
/// carrying the same scrollbar and keyboard paging as [`VirtualList`].
///
/// Use this where the rows are not interchangeable — a catalogue page mixes
/// section headers, link strips and book rows — and [`virtual_list`] where
/// every row is the same height.
#[derive(IntoElement)]
pub struct SizedVirtualList {
    id: ElementId,
    list: gpui_component::VirtualList,
}

impl SizedVirtualList {
    #[track_caller]
    fn new(list: gpui_component::VirtualList) -> Self {
        Self { id: ElementId::CodeLocation(*Location::caller()), list }
    }
}

impl Styled for SizedVirtualList {
    fn style(&mut self) -> &mut StyleRefinement {
        self.list.style()
    }
}

impl RenderOnce for SizedVirtualList {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let handle = window.use_keyed_state(self.id.clone(), cx, |_, _| VirtualListScrollHandle::new()).read(cx).clone();
        let base_handle = handle.base_handle().clone();
        register_page_scroll_handle(self.id.clone(), base_handle.clone(), cx);
        // The list keeps its own gap and padding; only the layout box that
        // positions it moves out to the root, so the scrollbar can overlay it.
        let root_style = outer_layout_style(&mut self.list);
        div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .refine_style(&root_style)
            .on_key_down(page_with_arrow_keys(base_handle))
            .child(self.list.track_scroll(&handle))
            .child(Scrollbar::new(&handle).id((self.id, "scrollbar")).axis(ScrollbarAxis::Vertical))
    }
}

#[track_caller]
pub fn sized_virtual_list(list: gpui_component::VirtualList) -> SizedVirtualList {
    SizedVirtualList::new(list)
}


impl ScrollableElement for Div {}
impl<E> ScrollableElement for Stateful<E>
where
    E: ParentElement + Styled + Element,
    Self: InteractiveElement,
{
}

pub fn page_with_arrow_keys(scroll_handle: ScrollHandle) -> impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }

        let Some(direction) = page_direction(&event.keystroke.key) else { return };
        page_scroll(&scroll_handle, direction, window, cx);
    }
}

/// Advances a horizontally scrolling carousel one step: `1.0` forward, `-1.0`
/// back. The step is most of the visible run but never less than one whole
/// card, so a window that fits three covers moves by three and a narrow one
/// still moves by one rather than appearing stuck.
pub fn step_carousel(scroll_handle: &ScrollHandle, direction: f32, item_width: f32, window: &mut Window) {
    let bounds = scroll_handle.bounds();
    let max_offset = scroll_handle.max_offset().x;
    if bounds.size.width <= gpui::Pixels::ZERO || max_offset <= gpui::Pixels::ZERO {
        return;
    }

    let step = (f32::from(bounds.size.width) * crate::CAROUSEL_STEP_FRACTION).max(item_width);
    let old_offset = scroll_handle.offset();
    // Scroll offsets run from 0 at the start to `-max_offset` at the end, so
    // moving forward subtracts. Clamping here rather than leaving it to the
    // next paint keeps repeated clicks at the end from banking offset that
    // later has to be spent before the row moves back.
    let x = (old_offset.x - gpui::px(step * direction)).clamp(-max_offset, gpui::Pixels::ZERO);
    if x != old_offset.x {
        scroll_handle.set_offset(gpui::point(x, old_offset.y));
        window.refresh();
    }
}

fn page_direction(key: &str) -> Option<f32> {
    match key {
        "left" => Some(1.0),
        "right" => Some(-1.0),
        _ => None,
    }
}

fn page_scroll(scroll_handle: &ScrollHandle, direction: f32, window: &mut Window, cx: &mut App) {
    let bounds = scroll_handle.bounds();
    if bounds.size.height <= gpui::Pixels::ZERO || scroll_handle.max_offset().y <= gpui::Pixels::ZERO {
        return;
    }

    let old_offset = scroll_handle.offset();
    scroll_handle.set_offset(gpui::point(old_offset.x, old_offset.y + bounds.size.height * direction));
    if scroll_handle.offset() != old_offset {
        window.refresh();
        cx.stop_propagation();
    }
}

fn outer_layout_style<E: Styled>(element: &mut E) -> StyleRefinement {
    let style = element.style();
    StyleRefinement {
        size: style.size.clone(),
        min_size: style.min_size.clone(),
        max_size: style.max_size.clone(),
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        flex_basis: style.flex_basis,
        align_self: style.align_self,
        ..Default::default()
    }
}
