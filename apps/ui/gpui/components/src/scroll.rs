use std::panic::Location;

use gpui::prelude::FluentBuilder;
use gpui::{App, Div, Element, ElementId, Global, InteractiveElement, IntoElement, ParentElement, RenderOnce, ScrollHandle, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Subscription, Window, div};
use gpui_component::StyledExt;
use gpui_component::scroll::{Scrollbar, ScrollbarAxis};

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

    /// Adds vertical scrolling without drawing a bar inside the surface.
    fn overflow_y_scroll_hidden(self) -> Scrollable<Self> {
        Scrollable::hidden(self, ScrollbarAxis::Vertical)
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
    show_scrollbar: bool,
}

impl<E> Scrollable<E>
where
    E: InteractiveElement + Styled + ParentElement + Element,
{
    #[track_caller]
    fn new(element: E, axis: ScrollbarAxis) -> Self {
        Self { id: ElementId::CodeLocation(*Location::caller()), element, axis, show_scrollbar: true }
    }

    #[track_caller]
    fn hidden(element: E, axis: ScrollbarAxis) -> Self {
        Self { id: ElementId::CodeLocation(*Location::caller()), element, axis, show_scrollbar: false }
    }

    fn with_id(element: E, axis: ScrollbarAxis, id: ElementId) -> Self {
        Self { id, element, axis, show_scrollbar: true }
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

        let scrollbar = self.show_scrollbar.then(|| Scrollbar::new(&scroll_handle).id((self.id.clone(), "scrollbar")).axis(self.axis));
        div().id(self.id).size_full().relative().refine_style(&root_style).on_key_down(page_with_arrow_keys(scroll_handle.clone())).child(scroll_area).children(scrollbar)
    }
}

impl ScrollableElement for Div {}
impl<E> ScrollableElement for Stateful<E>
where
    E: ParentElement + Styled + Element,
    Self: InteractiveElement,
{
}

fn page_with_arrow_keys(scroll_handle: ScrollHandle) -> impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        if event.keystroke.modifiers != gpui::Modifiers::default() {
            return;
        }

        let Some(direction) = page_direction(&event.keystroke.key) else { return };
        page_scroll(&scroll_handle, direction, window, cx);
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
    // Scrollable renders an extra viewport wrapper around the supplied
    // element. Positioning belongs to that outer layout box; leaving it on the
    // inner content makes an `absolute()` scroll surface participate in its
    // parent's flex layout and positions only the content inside the wrapper.
    let position = style.position.take();
    let inset = std::mem::take(&mut style.inset);
    StyleRefinement {
        position,
        inset,
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

#[cfg(test)]
mod layout_tests {
    use super::*;
    use gpui::{Position, px};

    #[test]
    fn outer_layout_style_moves_absolute_positioning_to_the_scroll_wrapper() {
        let mut element = div().absolute().left(px(12.0)).top(px(24.0));
        let outer = outer_layout_style(&mut element);

        assert_eq!(outer.position, Some(Position::Absolute));
        assert_ne!(outer.inset, Default::default());
        assert_eq!(element.style().position, None);
        assert_eq!(element.style().inset, Default::default());
    }
}
