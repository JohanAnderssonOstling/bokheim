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

use crate::{
    BrowserTheme, RADIUS_MD, RADIUS_SM, SETTINGS_CONTROL_GAP, SETTINGS_CONTROL_HEIGHT, SETTINGS_STEPPER_VALUE_WIDTH, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, SPACE_XXS, ScrollableElement, TEXT_LG, TEXT_SM, TEXT_XS, action_button,
    browser_theme, scroll::Scrollable,
};

pub fn reader_toolbar(theme: BrowserTheme) -> Div {
    div().h(px(58.0)).flex_none().w_full().min_w_0().px(px(SPACE_MD)).flex().items_center().gap(px(SPACE_SM)).bg(theme.surface)
}

pub fn reader_control_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    action_button(id, label).h(px(34.0)).rounded(px(RADIUS_SM)).border_color(theme.border).bg(theme.surface)
}

pub fn reader_control_button_icon(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, theme: BrowserTheme) -> Button {
    let label = label.into();
    Button::new(id).icon(icon).secondary().outline().w(px(34.0)).h(px(34.0)).px_0().rounded(px(RADIUS_SM)).border_color(theme.border).bg(theme.surface).tooltip(label)
}

pub fn reader_control_button_icon_state(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: IconName, enabled: bool, theme: BrowserTheme) -> Button {
    reader_control_button_icon(id, label, icon, theme).disabled(!enabled)
}

pub fn reader_toolbar_title(title: impl Into<SharedString>) -> Div {
    div().min_w_0().max_w(px(520.0)).flex_none().text_align(gpui::TextAlign::Center).text_size(gpui::rems(TEXT_SM / 16.0)).line_clamp(1).child(title.into())
}

pub fn reader_control_button_state(id: impl Into<ElementId>, label: impl Into<SharedString>, enabled: bool, theme: BrowserTheme) -> Button {
    reader_control_button(id, label, theme).disabled(!enabled)
}

pub fn reader_choice_button(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Button {
    // Selected segments use the primary/accent treatment; unselected segments
    // remain neutral secondary controls.
    action_button(id, label).h(px(SETTINGS_CONTROL_HEIGHT)).flex_1().rounded(px(RADIUS_SM)).selected(selected).px(px(SPACE_SM)).when(selected, |button| button.primary())
}

pub fn reader_choice_button_group(id: impl Into<ElementId>, buttons: impl IntoIterator<Item = Button>) -> ButtonGroup {
    ButtonGroup::new(id).children(buttons).outline().w(px(240.0)).flex_none()
}

pub fn reader_square_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    reader_control_button(id, label, theme).w(px(34.0)).px_0().text_size(gpui::rems(TEXT_LG / 16.0))
}

pub fn reader_match_count(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().min_w(px(42.0)).text_size(gpui::rems(TEXT_SM / 16.0)).text_color(theme.text_muted).child(text.into())
}

fn reader_settings_trigger(theme: BrowserTheme) -> gpui_component::button::Button {
    let presentation = ReaderSettingsPresentationContract::default();
    reader_control_button_icon("reader-settings-trigger", presentation.trigger_label, IconName::Settings2, theme)
}

/// How far through the book, as a single percentage.
///
/// Deliberately just the number: the toolbar is permanent chrome over the page,
/// and location and section counts are detail the tooltip can carry. The fixed
/// width keeps the title beside it from reflowing on every page turn.
pub fn reader_progress(label: impl Into<SharedString>, detail: impl Into<SharedString>, theme: BrowserTheme) -> Stateful<Div> {
    div()
        .id("reader-progress")
        .flex_none()
        .w(px(44.0))
        .px(px(SPACE_XS))
        .text_align(gpui::TextAlign::Right)
        .text_size(gpui::rems(TEXT_SM / 16.0))
        .text_color(theme.text_muted)
        .tooltip({
            let detail = detail.into();
            move |window, cx| gpui_component::tooltip::Tooltip::new(detail.clone()).build(window, cx)
        })
        .child(label.into())
}

fn reader_document_surface(theme: BrowserTheme) -> Div {
    div().size_full().min_h_0().min_w_0().bg(theme.page_bg).overflow_hidden()
}

pub fn reader_document_message(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    reader_document_surface(theme).flex().items_center().justify_center().text_color(theme.text_muted).text_size(gpui::rems(TEXT_SM / 16.0)).child(text.into())
}

pub fn reader_pdf_scroll_surface(scroll_handle: &ScrollHandle, theme: BrowserTheme, cx: &mut App) -> Stateful<Div> {
    crate::register_page_scroll_handle("pdf-reader-scroll", scroll_handle.clone(), cx);
    reader_document_surface(theme).id("pdf-reader-scroll").track_scroll(scroll_handle).flex().items_start().justify_start()
}

pub fn reader_toc(theme: BrowserTheme) -> Div {
    div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(SPACE_XXS)).p(px(SPACE_SM)).bg(theme.surface).border_r_1().border_color(theme.border)
}

pub fn reader_state_panel(theme: BrowserTheme) -> Scrollable<Div> {
    div().size_full().min_h_0().min_w_0().overflow_y_scrollbar().flex().flex_col().gap(px(SPACE_SM)).p(px(SPACE_SM)).bg(theme.surface).border_r_1().border_color(theme.border)
}

pub fn reader_state_card(id: impl Into<ElementId>, selected: bool, theme: BrowserTheme) -> Stateful<Div> {
    div().id(id).w_full().flex().flex_col().gap(px(SPACE_XS)).p(px(SPACE_SM)).rounded(px(RADIUS_SM)).border_1().border_color(theme.border).bg(theme.raised_bg).when(selected, |card| card.bg(theme.accent_soft))
}

pub fn reader_state_quote(text: impl Into<SharedString>) -> Div {
    div().text_size(gpui::rems(TEXT_SM / 16.0)).line_height(gpui::relative(1.35)).child(text.into())
}

pub fn reader_state_note(text: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().text_size(gpui::rems(TEXT_XS / 16.0)).text_color(theme.text_muted).child(text.into())
}

pub fn reader_state_actions() -> Div {
    div().flex().flex_wrap().items_center().gap(px(SPACE_XS))
}

pub fn reader_note_editor(theme: BrowserTheme) -> Div {
    div().h(px(92.0)).w_full().overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.border).bg(theme.raised_bg)
}

/// Compact non-modal annotation editor. Its caller supplies absolute placement
/// relative to the document viewport.
pub fn reader_annotation_editor(theme: BrowserTheme) -> Scrollable<Stateful<Div>> {
    div()
        .id("reader-annotation-editor")
        .w(px(360.0))
        .max_w_full()
        .overflow_y_scrollbar()
        .occlude()
        .p(px(SPACE_MD))
        .flex()
        .flex_col()
        .gap(px(SPACE_SM))
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow(crate::overlay_shadow(theme))
}

pub fn reader_annotation_note_editor(theme: BrowserTheme) -> Div {
    div().h(px(60.0)).w_full().overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.border).bg(theme.raised_bg)
}

fn reader_toc_disclosure(id: impl Into<ElementId>, label: impl IntoElement, theme: BrowserTheme) -> Stateful<Div> {
    div().id(id).w(px(22.0)).h_full().flex_none().flex().items_center().justify_center().rounded(px(RADIUS_SM)).bg(theme.surface).cursor_pointer().hover(move |style| style.bg(theme.accent_soft)).child(label)
}

pub fn reader_toc_title(id: impl Into<ElementId>, title: impl Into<SharedString>, theme: BrowserTheme) -> Stateful<Div> {
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
        .rounded(px(RADIUS_SM))
        .px(px(SPACE_SM))
        .cursor_pointer()
        .hover(move |style| style.bg(theme.accent_soft))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(title.into())
}

pub fn reader_settings_dropdown(theme: BrowserTheme) -> Scrollable<Div> {
    let presentation = ReaderSettingsPresentationContract::default();
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .w(px(f32::from(presentation.max_width_px)))
        .max_h(px(f32::from(presentation.max_height_px)))
        .overflow_y_scrollbar()
        .rounded(px(RADIUS_MD))
        .border_1()
        .border_color(theme.border)
        .shadow(crate::overlay_shadow(theme))
        .p(px(f32::from(presentation.padding_px)))
        .flex()
        .flex_col()
        .gap(px(f32::from(presentation.gap_px)))
        .bg(theme.surface)
}

pub fn reader_settings_section(label: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div().w_full().flex().flex_col().gap(px(SPACE_XS)).child(div().pb(px(SPACE_XXS)).text_size(gpui::rems(TEXT_XS / 16.0)).font_weight(FontWeight::SEMIBOLD).text_color(theme.text_muted).child(label.into()))
}

pub fn reader_setting_row(label: impl Into<SharedString>) -> Div {
    div().w_full().min_h(px(38.0)).flex().items_center().gap(px(SPACE_SM)).child(div().w(px(112.0)).flex_none().text_size(gpui::rems(TEXT_SM / 16.0)).child(label.into())).child(div().flex_1())
}

pub fn reader_setting_value(value: impl Into<SharedString>, theme: BrowserTheme) -> Div {
    div()
        .w(px(SETTINGS_STEPPER_VALUE_WIDTH))
        .h(px(SETTINGS_CONTROL_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.surface)
        .text_align(gpui::TextAlign::Center)
        .text_size(gpui::rems(TEXT_SM / 16.0))
        .text_color(theme.text_muted)
        .child(value.into())
}

pub fn reader_stepper_button(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: BrowserTheme) -> Button {
    action_button(id, label).w(px(SETTINGS_CONTROL_HEIGHT)).h(px(SETTINGS_CONTROL_HEIGHT)).px_0().rounded(px(0.0)).border_0().bg(theme.surface).text_size(gpui::rems(TEXT_LG / 16.0))
}

pub fn reader_stepper_group(decrement: Button, value: impl Into<SharedString>, increment: Button, theme: BrowserTheme) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(SETTINGS_CONTROL_GAP))
        .overflow_hidden()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(theme.border)
        .bg(theme.border)
        .child(decrement)
        .child(reader_setting_value(value, theme))
        .child(increment)
}

pub fn reader_toc_tree_item(id: impl Into<ElementId>, disclosure_id: impl Into<ElementId>, depth: usize, has_children: bool, expanded: bool, title: impl IntoElement, theme: BrowserTheme) -> ListItem {
    let disclosure = has_children
        .then(|| reader_toc_disclosure(disclosure_id, Icon::new(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }), theme))
        .map(IntoElement::into_any_element)
        .unwrap_or_else(|| div().w(px(22.0)).h_full().flex_none().bg(theme.surface).into_any_element());
    ListItem::new(id).w_full().h(px(38.0)).py_0().pl(px((8.0 + depth as f32 * 14.0).min(76.0))).pr(px(SPACE_SM)).rounded(px(RADIUS_SM)).child(div().size_full().flex().items_center().child(disclosure).child(title))
}

pub fn reader_search_box(theme: BrowserTheme) -> Div {
    div().w(px(260.0)).h(px(36.0)).overflow_hidden().rounded(px(RADIUS_SM)).border_1().border_color(theme.border).bg(theme.raised_bg)
}

pub fn reader_search_bar(theme: BrowserTheme) -> Div {
    div().h(px(52.0)).flex_none().w_full().px(px(SPACE_MD)).flex().items_center().gap(px(SPACE_SM)).bg(theme.surface).border_t_1().border_color(theme.border)
}

pub fn reader_footnote_panel(theme: BrowserTheme) -> Scrollable<Div> {
    div().max_h(px(240.0)).flex_none().overflow_y_scrollbar().px(px(SPACE_LG)).py(px(SPACE_MD)).flex().flex_col().gap(px(SPACE_SM)).bg(theme.surface).border_t_1().border_color(theme.border)
}

fn reader_image_preview(source: impl Into<ImageSource>) -> Img {
    img(source).size_full().object_fit(ObjectFit::Contain)
}

fn reader_modal_overlay() -> Div {
    div().absolute().inset_0().size_full().flex().items_center().justify_center().p(px(SPACE_XL))
}

fn reader_modal_scrim(theme: BrowserTheme) -> Stateful<Div> {
    div().id("reader-modal-scrim").absolute().inset_0().size_full().bg(theme.overlay_scrim).cursor_default()
}

fn reader_modal_panel(width: Pixels, height: Pixels, theme: BrowserTheme) -> Div {
    div().relative().w(width).h(height).max_w_full().max_h_full().min_h_0().min_w_0().occlude().p(px(SPACE_MD)).flex().flex_col().gap(px(SPACE_SM)).rounded(px(RADIUS_MD)).border_1().border_color(theme.border).bg(theme.surface)
}

type ReaderPopupCallback = Rc<dyn Fn(&mut Window, &mut App)>;

/// Shared centered reader-modal shell. Specialized popups provide only their
/// content, requested size, and dismissal behavior.
#[derive(IntoElement)]
struct ReaderModal {
    size: Size<Pixels>,
    content: gpui::AnyElement,
    on_dismiss: ReaderPopupCallback,
}

impl ReaderModal {
    fn new(size: Size<Pixels>, content: impl IntoElement) -> Self {
        Self { size, content: content.into_any_element(), on_dismiss: Rc::new(|_, _| {}) }
    }

    fn on_dismiss(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Rc::new(callback);
        self
    }
}

impl RenderOnce for ReaderModal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = browser_theme(cx);
        let dismiss = self.on_dismiss;
        reader_modal_overlay().child(reader_modal_scrim(theme).on_click(move |_, window, cx| dismiss(window, cx))).child(reader_modal_panel(self.size.width, self.size.height, theme).child(self.content))
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
        let theme = browser_theme(cx);
        let intrinsic = self.image.clone().use_render_image(window, cx).map(|rendered| {
            let size = rendered.size(0);
            (size.width.0 as f32, size.height.0 as f32)
        });
        let (panel_width, panel_height) = image_popup_panel_size(f32::from(self.reader_size.width), f32::from(self.reader_size.height), intrinsic);
        let copy = self.on_copy;
        let dismiss_modal = self.on_dismiss.clone();
        let dismiss_button = self.on_dismiss;
        let content = div().size_full().min_h_0().min_w_0().flex().flex_col().gap(px(SPACE_SM)).child(reader_state_note(self.uri, theme)).child(reader_content().child(reader_image_preview(self.image))).child(
            reader_state_actions()
                .child(reader_control_button("reader-image-copy", "Copy image", theme).on_click(move |_, window, cx| copy(window, cx)))
                .child(reader_control_button("reader-image-close", "Close", theme).on_click(move |_, window, cx| dismiss_button(window, cx))),
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

/// Invisible strip along the top edge that brings a hidden toolbar back.
///
/// Kept thin so it barely overlaps the document: a reader who is selecting text
/// near the top of the page should not keep summoning the chrome.
pub fn reader_chrome_reveal() -> Stateful<Div> {
    div().id("reader-chrome-reveal").absolute().top_0().left_0().right_0().h(px(SPACE_SM))
}

pub fn reader_content() -> Div {
    div().flex_1().min_h_0()
}

/// The appearance popover, anchored to the end of the reader toolbar.
///
/// `document_focus` is where the popover should put keyboard focus while it is
/// open. A popover always focuses something on open, and nothing in this panel
/// takes keyboard input — it is buttons throughout — so pointing it at the
/// document leaves paging keys working while settings are being adjusted.
/// Without this the popover focuses itself and the reader's navigation keys go
/// dead until the document is clicked again.
pub fn reader_settings_popover(document_focus: Option<&gpui::FocusHandle>, theme: BrowserTheme) -> Popover {
    let presentation = ReaderSettingsPresentationContract::default();
    let anchor = match presentation.anchor {
        ReaderSettingsAnchor::ReaderToolbarEnd => gpui::Anchor::BottomRight,
    };
    Popover::new("reader-settings").anchor(anchor).p_0().when_some(document_focus, |popover, focus| popover.track_focus(focus)).trigger(reader_settings_trigger(theme).tooltip("Reader settings"))
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
        let theme = browser_theme(cx);
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
            .child(div().id("reader-resize-handle").absolute().top_0().left(px(width - 4.0)).h_full().w(px(9.0)).cursor_col_resize().flex().justify_center().child(div().h_full().w(px(1.0)).bg(theme.border)).on_mouse_down(
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
