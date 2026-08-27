//! Toolbar structure shared by every reader surface.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, FocusHandle, Window};
use gpui_component::IconName;
use ui_components as components;

/// Operations provided by the entity that owns a reader toolbar.
pub(crate) trait ToolbarTarget: Sized + 'static {
    fn return_to_library(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn toggle_sidebar(&mut self, contents_available: bool, cx: &mut Context<Self>);
    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

/// The format-specific content placed into the common toolbar structure.
pub(crate) struct ToolbarContent {
    title: String,
    progress_label: String,
    progress_detail: String,
    document_focus: Option<FocusHandle>,
    settings: AnyElement,
    contents_available: bool,
    left_extras: Vec<AnyElement>,
    right_extras: Vec<AnyElement>,
}

impl ToolbarContent {
    pub(crate) fn new(title: String, progress_label: String, progress_detail: String, document_focus: Option<FocusHandle>, settings: AnyElement) -> Self {
        Self { title, progress_label, progress_detail, document_focus, settings, contents_available: true, left_extras: Vec::new(), right_extras: Vec::new() }
    }

    #[cfg(feature = "pdf")]
    pub(crate) fn contents_available(mut self, available: bool) -> Self {
        self.contents_available = available;
        self
    }

    pub(crate) fn left(mut self, element: impl IntoElement) -> Self {
        self.left_extras.push(element.into_any_element());
        self
    }

    #[cfg(feature = "pdf")]
    pub(crate) fn right(mut self, element: impl IntoElement) -> Self {
        self.right_extras.push(element.into_any_element());
        self
    }
}

pub(crate) fn reader_toolbar<C: ToolbarTarget>(target: &Entity<C>, content: ToolbarContent, theme: components::BrowserTheme) -> AnyElement {
    let library_target = target.clone();
    let sidebar_target = target.clone();
    let search_target = target.clone();

    let left = gpui::div()
        .min_w_0()
        .flex_1()
        .flex()
        .items_center()
        .gap(gpui::px(components::SPACE_SM))
        .child(components::reader_control_button_icon("reader-library", "Return to library", IconName::BookOpen, theme).on_click(move |_, window, cx| {
            library_target.update(cx, |this, cx| this.return_to_library(window, cx));
        }))
        .child(components::reader_control_button_icon("reader-sidebar", "Toggle contents and annotations", IconName::PanelLeftOpen, theme).on_click(move |_, _, cx| {
            sidebar_target.update(cx, |this, cx| this.toggle_sidebar(content.contents_available, cx));
        }))
        .children(content.left_extras);

    let right = gpui::div()
        .min_w_0()
        .flex_1()
        .flex()
        .items_center()
        .justify_end()
        .gap(gpui::px(components::SPACE_SM))
        .children(content.right_extras)
        .child(components::reader_control_button_icon("reader-search", "Search", IconName::Search, theme).on_click(move |_, window, cx| {
            search_target.update(cx, |this, cx| this.open_search(window, cx));
        }))
        .child(components::reader_progress(content.progress_label, content.progress_detail, theme))
        .child(components::reader_settings_popover(content.document_focus.as_ref(), theme).child(content.settings));

    components::reader_toolbar(theme).child(left).child(components::reader_toolbar_title(content.title)).child(right).into_any_element()
}
