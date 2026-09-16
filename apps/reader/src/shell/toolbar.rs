//! Toolbar structure shared by every reader surface.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, FocusHandle, Window};
use gpui_component::IconName;
use ui_components as components;

/// Operations provided by the entity that owns a reader toolbar.
pub(crate) trait ToolbarTarget: Sized + 'static {
    #[cfg(feature = "kobo")]
    fn brightness_controls(&self) -> Entity<super::brightness::BrightnessControls>;
    fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn toggle_sidebar(&mut self, contents_available: bool, cx: &mut Context<Self>);
    fn search_is_open(&self) -> bool;
    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>);
    /// Whether an annotation is open. The toolbar offers to discard it while it
    /// is, which is the only place that offer is made now that the note panel
    /// carries no buttons of its own.
    fn has_selected_annotation(&self) -> bool {
        false
    }
    fn delete_selected_annotation(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
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

    pub(crate) fn contents_available(mut self, available: bool) -> Self {
        self.contents_available = available;
        self
    }

    pub(crate) fn right(mut self, element: impl IntoElement) -> Self {
        self.right_extras.push(element.into_any_element());
        self
    }
}

/// `owner` is already leased by GPUI during rendering. Read state through it;
/// the entity handle is only for callbacks that run after rendering completes.
pub(crate) fn reader_toolbar<C: ToolbarTarget>(owner: &C, target: &Entity<C>, content: ToolbarContent, theme: components::BrowserTheme, window: &Window) -> AnyElement {
    let library_target = target.clone();
    let sidebar_target = target.clone();
    let search_target = target.clone();
    let delete_target = target.clone();
    let annotation_open = owner.has_selected_annotation();
    let contents_available = content.contents_available;
    let left_extras = content.left_extras;
    let right_extras = content.right_extras;

    let heading = gpui::div().min_w_0().flex_1().flex().items_center().gap(gpui::px(components::SPACE_SM)).child(components::reader_toolbar_title(content.title)).child(components::reader_progress(
        content.progress_label,
        content.progress_detail,
        theme,
    ));

    let left = gpui::div()
        .flex_none()
        .flex()
        .items_center()
        .gap(gpui::px(components::SPACE_SM))
        .when(!components::uses_mobile_navigation(window), |left| {
            left.child(components::reader_control_button_icon("reader-library", "Return to library", IconName::ArrowLeft, theme).on_click(move |_, window, cx| {
                library_target.update(cx, |this, cx| this.close_reader(window, cx));
            }))
        })
        .child(components::reader_control_button_icon("reader-sidebar", "Toggle contents and annotations", IconName::PanelLeftOpen, theme).on_click(move |_, _, cx| {
            sidebar_target.update(cx, |this, cx| this.toggle_sidebar(contents_available, cx));
        }))
        .children(left_extras);

    let right = gpui::div()
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .gap(gpui::px(components::SPACE_SM))
        .children(right_extras)
        .children(annotation_open.then(|| {
            components::reader_annotation_delete_button("reader-delete-annotation", gpui::px(components::READER_TOOLBAR_CONTROL_HEIGHT), theme).on_click(move |_, window, cx| {
                delete_target.update(cx, |this, cx| this.delete_selected_annotation(window, cx));
            })
        }))
        .map(|right| {
            #[cfg(feature = "kobo")]
            let right = right.child(super::brightness::popover(owner.brightness_controls(), content.document_focus.as_ref(), theme));
            right
        })
        .child(components::selectable_button(components::reader_control_button_icon("reader-search", if owner.search_is_open() { "Hide search" } else { "Search" }, IconName::Search, theme), owner.search_is_open(), theme).on_click(
            move |_, window, cx| {
                search_target.update(cx, |this, cx| this.toggle_search(window, cx));
            },
        ))
        .child(components::reader_settings_popover(content.document_focus.as_ref(), theme).child(content.settings));

    components::reader_toolbar(theme).child(left).child(heading).child(right).into_any_element()
}

#[cfg(all(test, not(feature = "kobo")))]
mod tests {
    use super::*;

    struct TestReader {
        search_open: bool,
        draws: usize,
    }

    impl ToolbarTarget for TestReader {
        fn close_reader(&mut self, _: &mut Window, _: &mut Context<Self>) {}
        fn toggle_sidebar(&mut self, _: bool, _: &mut Context<Self>) {}
        fn search_is_open(&self) -> bool {
            self.search_open
        }
        fn toggle_search(&mut self, _: &mut Window, _: &mut Context<Self>) {
            self.search_open = !self.search_open;
        }
    }

    impl gpui::Render for TestReader {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.draws += 1;
            let content = ToolbarContent::new("Book".into(), "1 / 2".into(), String::new(), None, gpui::div().into_any_element());
            reader_toolbar(self, &cx.entity(), content, components::BrowserTheme::default(), window)
        }
    }

    #[gpui::test]
    fn toolbar_can_be_built_while_its_reader_is_leased(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let reader = cx.add_window(|_, _| TestReader { search_open: false, draws: 0 });
        cx.update_window(reader.into(), |_, window, cx| window.draw(cx).clear(cx)).unwrap();
        reader
            .update(cx, |reader, _, cx| {
                assert!(reader.draws > 0);
                reader.draws = 0;
                reader.search_open = true;
                cx.notify();
            })
            .unwrap();
        cx.update_window(reader.into(), |_, window, cx| window.draw(cx).clear(cx)).unwrap();
        reader.update(cx, |reader, _, _| assert!(reader.draws > 0)).unwrap();
    }
}
