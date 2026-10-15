//! EPUB contents, annotation, and settings sidebar tabs.

use crate::invalidation::notify;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, Window};
use library_backend::Annotation;
use ui_components as components;

use crate::epub::ReaderView;
use crate::invalidation::Component;
use crate::shell::annotation_tree::{self, AnnotationTreeTarget};
use crate::shell::state_panel::{self, AnnotationSidebarTarget, SidebarTab, TocActivation};
use crate::shell::toolbar::SidebarTarget;

impl AnnotationSidebarTarget for ReaderView {
    fn select_sidebar_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.annotation_popup_anchor = None;
        self.select_annotation(id, window, cx);
    }

    fn go_to_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>) {
        self.navigate_to_cfi(annotation.anchor.epub_cfi_value().unwrap_or_default().to_owned(), cx);
    }

    fn delete_sidebar_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        self.delete_annotation(id, cx);
    }
}

impl AnnotationTreeTarget for ReaderView {
    fn set_annotation_branch_expanded(&mut self, id: usize, expanded: bool, cx: &mut Context<Self>) {
        self.annotation_tree.set_expanded(id, expanded);
        notify(cx, Component::ReaderShell, "annotation_branch_toggled");
    }
}

impl ReaderView {
    fn render_toc_body(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &App) -> AnyElement {
        let active_link = self.toc.active_link();
        let toc = state_panel::reader_toc_tree(&self.toc_tree, &entity, active_link.as_deref(), self.settings_navigation.keyboard_visible(), theme, cx, |link, activation, window, this: &mut Self, cx| {
            this.navigate_to(link.to_owned(), cx);
            if activation == TocActivation::Pointer {
                this.focus_sidebar_body(window, cx);
                #[cfg(feature = "kobo")]
                {
                    this.sidebar.close(cx);
                    this.focus_document(window, cx);
                    notify(cx, Component::ReaderShell, "toc_navigated");
                }
            }
        });
        components::reader_toc(theme).child(toc).into_any_element()
    }

    fn render_annotations_body(&mut self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let tree = self.toc.annotation_tree(self.marks.annotations().iter().map(|annotation| annotation.toc_ordinal.and_then(|ordinal| usize::try_from(ordinal).ok())));
        let selected_id = self.marks.selected_id();
        let selected_index = self.marks.annotations().iter().position(|annotation| Some(annotation.id.as_str()) == selected_id);
        self.annotation_tree.reveal_selected(&tree, selected_id, selected_index);
        let rows = annotation_tree::annotation_tree_rows(&cx.entity(), &tree, &self.annotation_tree, self.marks.annotations(), selected_id, "No notes yet. Select text, then choose Annotation to add a note.", theme);
        components::reader_state_panel(theme).children(rows).into_any_element()
    }

    /// The selected panel's body; the shell supplies its rail and header.
    pub(super) fn render_sidebar(&mut self, active: SidebarTab, entity: Entity<Self>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        match active {
            SidebarTab::Contents => self.render_toc_body(entity, theme, cx),
            SidebarTab::Notes => self.render_annotations_body(theme, cx),
            SidebarTab::Search => self.render_search(theme, cx),
            SidebarTab::Settings => self.render_settings_panel(cx),
        }
    }
}
