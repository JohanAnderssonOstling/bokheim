use std::rc::Rc;
use std::sync::Arc;

use app_contract::{ReaderSettingsAnchor, ReaderSettingsPresentationContract};
use gpui::prelude::*;
use gpui::{
    App, Bounds, Div, Element, ElementId, FontWeight, GlobalElementId, Image, ImageSource, Img, LayoutId, MouseButton, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, RenderOnce, ScrollHandle, SharedString, Size, Stateful, Style,
    StyledImage, Window, div, img, px,
};
use gpui_component::button::{Button, ButtonGroup, ButtonVariants};
use gpui_component::list::ListItem;
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::popover::Popover;
use gpui_component::{Disableable, ElementExt, Selectable};
use gpui_component::{Icon, IconName};

use crate::{ScrollableElement, scroll::Scrollable};

use crate::{ACCENT_SOFT, BORDER, CARD_BG, PAGE_BG, SURFACE, TEXT_MUTED, action_button};

pub fn reader_toolbar() -> Div {
    div().h(px(58.0)).flex_none().w_full().min_w_0().px(px(14.0)).flex().items_center().gap(px(10.0)).bg(gpui::rgb(SURFACE)).border_b_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_toolbar_group() -> Div {
    div().min_w_0().flex().items_center().gap(px(7.0))
}

pub fn reader_control_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    action_button(id, label).h(px(34.0)).rounded(px(8.0)).border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG))
}

pub fn reader_control_button_icon(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName) -> Button {
    reader_control_button(id, label).icon(icon)
}

pub fn reader_control_button_state(id: impl Into<ElementId>, label: impl Into<SharedString>, enabled: bool) -> Button {
    reader_control_button(id, label).disabled(!enabled)
}

pub fn reader_choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Button {
    // Let gpui-component own the background, border, and foreground colors so
    // its selected state is not overwritten by the reader's neutral card style.
    action_button(id, label).h(px(34.0)).rounded(px(8.0)).selected(selected).px(px(10.0))
}

pub fn reader_choice_button_group(id: impl Into<ElementId>, buttons: impl IntoIterator<Item = Button>) -> ButtonGroup {
    ButtonGroup::new(id).children(buttons).secondary().outline()
}

pub fn reader_square_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    reader_control_button(id, label).w(px(34.0)).px_0().text_size(gpui::rems(18.0 / 16.0))
}

pub fn reader_match_count(text: impl Into<SharedString>) -> Div {
    div().min_w(px(42.0)).text_size(gpui::rems(13.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn reader_settings_trigger() -> gpui_component::button::Button {
    let presentation = ReaderSettingsPresentationContract::default();
    action_button("reader-settings-trigger", presentation.trigger_label).h(px(34.0)).rounded(px(8.0)).border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG))
}

pub fn reader_title(text: impl Into<SharedString>) -> Div {
    div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().px(px(5.0)).text_size(gpui::rems(16.0 / 16.0)).font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub fn reader_document_surface() -> Div {
    div().size_full().min_h_0().min_w_0().bg(gpui::rgb(PAGE_BG)).overflow_hidden()
}

pub fn reader_document_message(text: impl Into<SharedString>) -> Div {
    reader_document_surface().flex().items_center().justify_center().text_color(gpui::rgb(TEXT_MUTED)).text_size(gpui::rems(14.0 / 16.0)).child(text.into())
}

pub fn reader_pdf_scroll_surface(scroll_handle: &ScrollHandle) -> Stateful<Div> {
    reader_document_surface().id("pdf-reader-scroll").overflow_x_scroll().overflow_y_scroll().track_scroll(scroll_handle).flex().items_start().justify_center()
}

pub fn reader_toc() -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(2.0)).p(px(8.0)).bg(gpui::rgb(SURFACE)).border_r_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_state_panel() -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().overflow_y_scrollbar().flex().flex_col().gap(px(8.0)).p(px(10.0)).bg(gpui::rgb(SURFACE)).border_r_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_state_tabs() -> Div {
    div().flex().items_center().gap(px(6.0)).pb(px(4.0))
}

pub fn reader_state_card(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    div().id(id).w_full().flex().flex_col().gap(px(6.0)).p(px(9.0)).rounded(px(8.0)).border_1().border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG)).when(selected, |card| card.bg(gpui::rgb(ACCENT_SOFT)))
}

pub fn reader_state_quote(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(13.0 / 16.0)).line_height(gpui::relative(1.35)).child(text.into())
}

pub fn reader_state_note(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(12.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn reader_state_actions() -> Div {
    div().flex().flex_wrap().items_center().gap(px(5.0))
}

pub fn reader_note_editor() -> Div {
    div().h(px(92.0)).w_full().overflow_hidden().rounded(px(8.0)).border_1().border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG))
}

/// Non-modal annotation editor anchored to the reader edge.
pub fn reader_annotation_editor() -> Scrollable<Div> {
    div()
        .absolute()
        .top(px(12.0))
        .right(px(12.0))
        .bottom(px(12.0))
        .w(px(360.0))
        .max_w_full()
        .max_h(px(420.0))
        .overflow_y_scrollbar()
        .occlude()
        .p(px(12.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(gpui::rgb(BORDER))
        .bg(gpui::rgb(SURFACE))
        .shadow_lg()
}

pub fn reader_annotation_editor_header() -> Div {
    div().w_full().flex().items_center().justify_between().gap(px(8.0))
}

pub fn reader_toc_row(id: impl Into<ElementId>, depth: usize, selected: bool) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(38.0))
        .min_w_0()
        .flex()
        .flex_none()
        .items_center()
        .pl(px((10.0 + depth as f32 * 12.0).min(70.0)))
        .pr(px(10.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .rounded(px(7.0))
        .text_size(gpui::rems(15.0 / 16.0))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(selected, |row| row.bg(gpui::rgb(ACCENT_SOFT)))
        .hover(|style| style.bg(gpui::rgb(ACCENT_SOFT)))
}

pub fn reader_toc_disclosure(id: impl Into<ElementId>, label: impl IntoElement) -> Stateful<Div> {
    div().id(id).w(px(22.0)).h_full().flex_none().flex().items_center().justify_center().rounded(px(5.0)).bg(gpui::rgb(SURFACE)).cursor_pointer().hover(|style| style.bg(gpui::rgb(ACCENT_SOFT))).child(label)
}

pub fn reader_toc_title(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h_full()
        .min_w_0()
        .flex_1()
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .rounded(px(7.0))
        .px(px(8.0))
        .cursor_pointer()
        .hover(|style| style.bg(gpui::rgb(ACCENT_SOFT)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(title.into())
}

pub fn reader_settings_panel() -> Div {
    div().w(px(240.0)).flex().flex_col().gap_2()
}

pub fn reader_settings_dropdown() -> Scrollable<Div> {
    let presentation = ReaderSettingsPresentationContract::default();
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .w(px(f32::from(presentation.max_width_px)))
        .max_h(px(f32::from(presentation.max_height_px)))
        .overflow_y_scrollbar()
        .p(px(f32::from(presentation.padding_px)))
        .flex()
        .flex_col()
        .gap(px(f32::from(presentation.gap_px)))
        .bg(gpui::rgb(SURFACE))
}

pub fn reader_settings_title() -> Div {
    crate::section_title(ReaderSettingsPresentationContract::default().title)
}

pub fn reader_settings_choice_grid() -> Div {
    let presentation = ReaderSettingsPresentationContract::default();
    div().w_full().flex().flex_wrap().gap(px(f32::from(presentation.gap_px)))
}

pub fn reader_settings_choice_cell(label: impl Into<SharedString>) -> Div {
    let presentation = ReaderSettingsPresentationContract::default();
    let available_width = f32::from(presentation.max_width_px) - 2.0 * f32::from(presentation.padding_px) - f32::from(presentation.gap_px);
    let width = (available_width / 2.0).max(f32::from(presentation.minimum_column_width_px));
    div().w(px(width)).min_w_0().flex().flex_col().gap(px(5.0)).child(div().text_size(gpui::rems(12.0 / 16.0)).font_weight(FontWeight::BOLD).child(label.into()))
}

pub fn reader_settings_choice_controls() -> Div {
    div().w_full().min_w_0().flex().items_center().gap(px(6.0))
}

pub fn reader_settings_choice_value(value: impl Into<SharedString>) -> Div {
    div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(gpui::rems(13.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(value.into())
}

pub fn reader_setting_row(label: impl Into<SharedString>) -> Div {
    div().flex().items_center().gap(px(8.0)).child(div().w(px(124.0)).text_size(gpui::rems(14.0 / 16.0)).child(label.into()))
}

pub fn reader_setting_value(value: impl Into<SharedString>) -> Div {
    div().w(px(72.0)).text_center().text_size(gpui::rems(13.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(value.into())
}

pub fn reader_toc_tree_item(id: impl Into<ElementId>, disclosure_id: impl Into<ElementId>, depth: usize, has_children: bool, expanded: bool, title: impl IntoElement) -> ListItem {
    let disclosure = has_children
        .then(|| reader_toc_disclosure(disclosure_id, Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight })))
        .map(IntoElement::into_any_element)
        .unwrap_or_else(|| div().w(px(22.0)).h_full().flex_none().bg(gpui::rgb(SURFACE)).into_any_element());
    ListItem::new(id).w_full().h(px(38.0)).py_0().pl(px((8.0 + depth as f32 * 14.0).min(76.0))).pr(px(8.0)).rounded(px(7.0)).child(div().size_full().flex().items_center().child(disclosure).child(title))
}

pub fn reader_shortcuts() -> Div {
    div().mt(px(4.0)).flex().flex_col().gap(px(3.0))
}

pub fn reader_shortcut(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(12.0 / 16.0)).text_color(gpui::rgb(TEXT_MUTED)).child(text.into())
}

pub fn reader_settings_actions() -> Div {
    div().flex().gap_2()
}

pub fn reader_search_box() -> Div {
    div().w(px(260.0)).h(px(36.0)).overflow_hidden().rounded(px(8.0)).border_1().border_color(gpui::rgb(BORDER)).bg(gpui::rgb(CARD_BG))
}

pub fn reader_search_actions() -> Div {
    div().flex().items_center().gap_1()
}

pub fn reader_search_bar() -> Div {
    div().h(px(52.0)).flex_none().w_full().px(px(12.0)).flex().items_center().gap(px(8.0)).bg(gpui::rgb(SURFACE)).border_t_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_footnote_panel() -> Scrollable<Div> {
    div().max_h(px(240.0)).flex_none().overflow_y_scrollbar().px(px(18.0)).py(px(12.0)).flex().flex_col().gap(px(9.0)).bg(gpui::rgb(SURFACE)).border_t_1().border_color(gpui::rgb(BORDER))
}

pub fn reader_image_preview(source: impl Into<ImageSource>) -> Img {
    img(source).size_full().object_fit(ObjectFit::Contain)
}

fn reader_modal_overlay() -> Div {
    div().absolute().inset_0().size_full().flex().items_center().justify_center().p(px(24.0))
}

fn reader_modal_scrim() -> Stateful<Div> {
    div().id("reader-modal-scrim").absolute().inset_0().size_full().bg(gpui::rgba(0x1f222080)).cursor_default()
}

fn reader_modal_panel(width: Pixels, height: Pixels) -> Div {
    div().relative().w(width).h(height).max_w_full().max_h_full().min_h_0().min_w_0().occlude().p(px(12.0)).flex().flex_col().gap(px(8.0)).rounded(px(12.0)).border_1().border_color(gpui::rgb(BORDER)).bg(gpui::rgb(SURFACE))
}

type ReaderPopupCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Shared centered reader-modal shell. Specialized popups provide only their
/// content, requested size, and dismissal behavior.
#[derive(IntoElement)]
pub struct ReaderModal {
    size: Size<Pixels>,
    content: gpui::AnyElement,
    on_dismiss: ReaderPopupCallback,
}

impl ReaderModal {
    pub fn new(size: Size<Pixels>, content: impl IntoElement) -> Self {
        Self { size, content: content.into_any_element(), on_dismiss: Rc::new(|_, _| {}) }
    }

    pub fn on_dismiss(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Rc::new(callback);
        self
    }
}

impl RenderOnce for ReaderModal {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let dismiss = self.on_dismiss;
        reader_modal_overlay().child(reader_modal_scrim().on_click(move |_, window, cx| dismiss(window, cx))).child(reader_modal_panel(self.size.width, self.size.height).child(self.content))
    }
}

/// Reader-specific semantic sections layered on GPUI Component's native
/// `PopupMenu`. This keeps labels, ordering, and separators consistent while
/// retaining the toolkit's focus and keyboard behavior.
pub struct ReaderContextMenu {
    menu: PopupMenu,
    has_section: bool,
}

impl ReaderContextMenu {
    pub fn new(menu: PopupMenu) -> Self {
        Self { menu, has_section: false }
    }

    fn begin_section(mut self) -> Self {
        if self.has_section {
            self.menu = self.menu.separator();
        }
        self.has_section = true;
        self
    }

    pub fn selection(mut self, on_highlight: impl Fn(&mut Window, &mut App) + 'static, on_cite: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self = self.begin_section();
        self.menu = self.menu.item(PopupMenuItem::new("Highlight").on_click(move |_, window, cx| on_highlight(window, cx))).item(PopupMenuItem::new("Cite").on_click(move |_, window, cx| on_cite(window, cx)));
        self
    }

    pub fn table(mut self, on_markdown: impl Fn(&mut Window, &mut App) + 'static, on_unstyled_html: impl Fn(&mut Window, &mut App) + 'static, on_styled_html: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self = self.begin_section();
        self.menu = self
            .menu
            .item(PopupMenuItem::new("Copy Table as Markdown").on_click(move |_, window, cx| on_markdown(window, cx)))
            .item(PopupMenuItem::new("Copy Table as Unstyled HTML").on_click(move |_, window, cx| on_unstyled_html(window, cx)))
            .item(PopupMenuItem::new("Copy Table as Styled HTML").on_click(move |_, window, cx| on_styled_html(window, cx)));
        self
    }

    pub fn image(mut self, on_copy: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self = self.begin_section();
        self.menu = self.menu.item(PopupMenuItem::new("Copy Image").on_click(move |_, window, cx| on_copy(window, cx)));
        self
    }

    pub fn finish(self) -> PopupMenu {
        self.menu
    }
}

fn image_popup_panel_size(reader_width: f32, reader_height: f32, intrinsic: Option<(f32, f32)>) -> (f32, f32) {
    const OVERLAY_INSET: f32 = 48.0;
    const PANEL_HORIZONTAL_CHROME: f32 = 24.0;
    const PANEL_VERTICAL_CHROME: f32 = 92.0;
    const MIN_PANEL_WIDTH: f32 = 280.0;
    const MIN_PANEL_HEIGHT: f32 = 180.0;
    const MAX_PANEL_WIDTH: f32 = 1100.0;
    const MAX_PANEL_HEIGHT: f32 = 820.0;

    let available_width = (reader_width - OVERLAY_INSET).max(1.0).min(MAX_PANEL_WIDTH);
    let available_height = (reader_height - OVERLAY_INSET).max(1.0).min(MAX_PANEL_HEIGHT);
    let image_limit_width = (available_width - PANEL_HORIZONTAL_CHROME).max(1.0);
    let image_limit_height = (available_height - PANEL_VERTICAL_CHROME).max(1.0);
    let (image_width, image_height) = intrinsic.filter(|(width, height)| width.is_finite() && height.is_finite() && *width > 0.0 && *height > 0.0).unwrap_or((4.0, 3.0));
    let scale = (image_limit_width / image_width).min(image_limit_height / image_height);
    let displayed_width = image_width * scale;
    let displayed_height = image_height * scale;
    ((displayed_width + PANEL_HORIZONTAL_CHROME).clamp(MIN_PANEL_WIDTH.min(available_width), available_width), (displayed_height + PANEL_VERTICAL_CHROME).clamp(MIN_PANEL_HEIGHT.min(available_height), available_height))
}

/// Centered, responsive image viewer used by reader surfaces.
#[derive(IntoElement)]
pub struct ReaderImagePopup {
    uri: SharedString,
    image: Arc<Image>,
    reader_size: Size<Pixels>,
    on_copy: ReaderPopupCallback,
    on_dismiss: ReaderPopupCallback,
}

impl ReaderImagePopup {
    pub fn new(uri: impl Into<SharedString>, image: Arc<Image>, reader_size: Size<Pixels>) -> Self {
        Self { uri: uri.into(), image, reader_size, on_copy: Rc::new(|_, _| {}), on_dismiss: Rc::new(|_, _| {}) }
    }

    pub fn on_copy(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_copy = Rc::new(callback);
        self
    }

    pub fn on_dismiss(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Rc::new(callback);
        self
    }
}

impl RenderOnce for ReaderImagePopup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let intrinsic = self.image.clone().use_render_image(window, cx).map(|rendered| {
            let size = rendered.size(0);
            (size.width.0 as f32, size.height.0 as f32)
        });
        let (panel_width, panel_height) = image_popup_panel_size(f32::from(self.reader_size.width), f32::from(self.reader_size.height), intrinsic);
        let copy = self.on_copy;
        let dismiss_modal = self.on_dismiss.clone();
        let dismiss_button = self.on_dismiss;
        let content = div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(8.0)).child(reader_state_note(self.uri)).child(reader_content().child(reader_image_preview(self.image))).child(
            reader_state_actions()
                .child(reader_control_button("reader-image-copy", "Copy image").on_click(move |_, window, cx| copy(window, cx)))
                .child(reader_control_button("reader-image-close", "Close").on_click(move |_, window, cx| dismiss_button(window, cx))),
        );

        ReaderModal::new(gpui::size(px(panel_width), px(panel_height)), content).on_dismiss(move |window, cx| dismiss_modal(window, cx))
    }
}

#[cfg(test)]
mod image_popup_tests {
    use super::image_popup_panel_size;

    #[test]
    fn panel_tracks_reader_bounds_and_image_aspect_ratio() {
        let landscape = image_popup_panel_size(1000.0, 700.0, Some((1600.0, 900.0)));
        let portrait = image_popup_panel_size(1000.0, 700.0, Some((900.0, 1600.0)));
        let constrained = image_popup_panel_size(420.0, 320.0, Some((1600.0, 900.0)));

        assert!(landscape.0 > landscape.1, "a landscape image should produce a wider dialog");
        assert!(portrait.1 > portrait.0, "a portrait image should produce a taller dialog");
        assert!(constrained.0 <= 372.0 && constrained.1 <= 272.0, "the dialog must remain within the reader inset");
    }
}

pub fn reader_document_body() -> Div {
    div().relative().size_full().min_h_0().min_w_0().flex().flex_col().overflow_hidden()
}

pub fn empty_document() -> Div {
    div().size_full()
}

pub fn reader_content() -> Div {
    div().flex_1().min_h_0()
}

pub fn reader_settings_popover() -> Popover {
    let presentation = ReaderSettingsPresentationContract::default();
    let anchor = match presentation.anchor {
        ReaderSettingsAnchor::ReaderToolbarEnd => gpui::Anchor::BottomRight,
    };
    Popover::new("reader-settings").anchor(anchor).p_0().trigger(reader_settings_trigger().tooltip("Reader settings"))
}

const READER_SIDEBAR_MIN_WIDTH: f32 = 260.0;
const READER_SIDEBAR_MAX_WIDTH: f32 = 520.0;

fn clamp_reader_sidebar_width(width: f32) -> f32 {
    width.clamp(READER_SIDEBAR_MIN_WIDTH, READER_SIDEBAR_MAX_WIDTH)
}

struct ReaderSplitState {
    width: f32,
    bounds: Bounds<Pixels>,
    resizing: bool,
}

impl ReaderSplitState {
    fn new(width: f32) -> Self {
        Self { width: clamp_reader_sidebar_width(width), bounds: Bounds::default(), resizing: false }
    }
}

/// A reader-specific two-pane layout whose sidebar has an absolute pixel width.
///
/// Unlike a proportional resizable panel group, window resizing never changes
/// the sidebar width. Only dragging the divider updates it.
#[derive(IntoElement)]
pub struct ReaderSplit {
    sidebar: gpui::AnyElement,
    document: gpui::AnyElement,
    width: f32,
    on_resize: Rc<dyn Fn(f32, &mut Window, &mut App)>,
}

impl ReaderSplit {
    pub fn on_resize(mut self, callback: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_resize = Rc::new(callback);
        self
    }
}

impl RenderOnce for ReaderSplit {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let requested_width = clamp_reader_sidebar_width(self.width);
        let state = window.use_keyed_state("reader-layout", cx, |_, _| ReaderSplitState::new(requested_width));
        state.update(cx, |state, _| {
            if !state.resizing && (state.width - requested_width).abs() >= 0.5 {
                state.width = requested_width;
            }
        });
        let width = state.read(cx).width;

        let start_resize = state.clone();
        let track_bounds = state.clone();
        div()
            .id("reader-layout")
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .overflow_hidden()
            .child(div().h_full().w(px(width)).min_w(px(width)).max_w(px(width)).flex_none().min_h_0().overflow_hidden().child(self.sidebar))
            .child(div().h_full().min_h_0().min_w_0().flex_1().overflow_hidden().child(self.document))
            .child(div().id("reader-resize-handle").absolute().top_0().left(px(width - 4.0)).h_full().w(px(9.0)).cursor_col_resize().flex().justify_center().child(div().h_full().w(px(1.0)).bg(gpui::rgb(BORDER))).on_mouse_down(
                MouseButton::Left,
                move |_, _, cx| {
                    cx.stop_propagation();
                    start_resize.update(cx, |state, cx| {
                        state.resizing = true;
                        cx.notify();
                    });
                },
            ))
            .on_prepaint(move |bounds, _, cx| {
                track_bounds.update(cx, |state, _| state.bounds = bounds);
            })
            .child(ReaderSplitEventSink { state, on_resize: self.on_resize })
    }
}

struct ReaderSplitEventSink {
    state: gpui::Entity<ReaderSplitState>,
    on_resize: Rc<dyn Fn(f32, &mut Window, &mut App)>,
}

impl IntoElement for ReaderSplitEventSink {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ReaderSplitEventSink {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), None, cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Window, _: &mut App) -> Self::PrepaintState {}

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, _: Bounds<Pixels>, _: &mut Self::RequestLayoutState, _: &mut Self::PrepaintState, window: &mut Window, _cx: &mut App) {
        let move_state = self.state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if !phase.bubble() || !move_state.read(cx).resizing {
                return;
            }
            let bounds = move_state.read(cx).bounds;
            let width = clamp_reader_sidebar_width(f32::from(event.position.x - bounds.left()));
            move_state.update(cx, |state, cx| {
                if (state.width - width).abs() >= 0.5 {
                    state.width = width;
                    cx.notify();
                }
            });
        });

        let end_state = self.state.clone();
        let on_resize = self.on_resize.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() || !end_state.read(cx).resizing {
                return;
            }
            let width = end_state.read(cx).width;
            end_state.update(cx, |state, cx| {
                state.resizing = false;
                cx.notify();
            });
            on_resize(width, window, cx);
        });
    }
}

pub fn reader_split(sidebar: impl IntoElement, document: impl IntoElement, sidebar_width: f32) -> ReaderSplit {
    ReaderSplit { sidebar: sidebar.into_any_element(), document: document.into_any_element(), width: sidebar_width, on_resize: Rc::new(|_, _, _| {}) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_sidebar_width_is_clamped_only_to_its_user_range() {
        assert_eq!(clamp_reader_sidebar_width(100.0), READER_SIDEBAR_MIN_WIDTH);
        assert_eq!(clamp_reader_sidebar_width(360.0), 360.0);
        assert_eq!(clamp_reader_sidebar_width(900.0), READER_SIDEBAR_MAX_WIDTH);
    }
}
