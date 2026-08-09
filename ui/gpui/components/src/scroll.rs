use std::panic::Location;

use gpui::prelude::FluentBuilder;
use gpui::{App, Div, Element, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, ScrollHandle, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window, div};
use gpui_component::StyledExt;
use gpui_component::scroll::{Scrollbar, ScrollbarAxis};

/// Adds Bokheim's keyboard paging behavior to a scrollable element.
pub trait ScrollableElement: InteractiveElement + Styled + ParentElement + Element {
    /// Adds a vertical scrollbar. Left pages up and right pages down by the
    /// scroll area's measured viewport height.
    #[track_caller]
    fn overflow_y_scrollbar(self) -> Scrollable<Self> {
        Scrollable::new(self, ScrollbarAxis::Vertical)
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
        let root_style = outer_layout_style(&mut self.element);

        let key_scroll_handle = scroll_handle.clone();
        let page_with_arrow_keys = move |event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut App| {
            if event.keystroke.modifiers != gpui::Modifiers::default() {
                return;
            }

            let direction = match event.keystroke.key.as_str() {
                "left" => 1.0,
                "right" => -1.0,
                _ => return,
            };
            let bounds = key_scroll_handle.bounds();
            if bounds.size.height <= gpui::Pixels::ZERO || key_scroll_handle.max_offset().y <= gpui::Pixels::ZERO || !bounds.contains(&window.mouse_position()) {
                return;
            }

            let old_offset = key_scroll_handle.offset();
            key_scroll_handle.set_offset(gpui::point(old_offset.x, old_offset.y + bounds.size.height * direction));
            if key_scroll_handle.offset() != old_offset {
                window.refresh();
                cx.stop_propagation();
            }
        };

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

        div().id(self.id.clone()).size_full().refine_style(&root_style).relative().on_key_down(page_with_arrow_keys).child(scroll_area).child(Scrollbar::new(&scroll_handle).id((self.id, "scrollbar")).axis(self.axis))
    }
}

impl ScrollableElement for Div {}
impl<E> ScrollableElement for Stateful<E>
where
    E: ParentElement + Styled + Element,
    Self: InteractiveElement,
{
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
