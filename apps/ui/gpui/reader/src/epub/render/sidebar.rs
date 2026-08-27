//! EPUB contents and annotation sidebars.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, IntoElement};
use library_model::ReaderAnnotation;
use ui_components as components;

use crate::epub::ReaderView;
use crate::invalidation::Component;
use crate::shell::state_panel::{self, SidebarPanel};

impl ReaderView {
    fn render_sidebar_tabs(&self, show_annotations: bool, cx: &mut Context<Self>) -> AnyElement {
        state_panel::reader_sidebar_tabs("reader-sidebar-tabs", &cx.entity(), show_annotations, true, cx, |annotations, this, cx| {
            let panel = if annotations { SidebarPanel::Annotations } else { SidebarPanel::Contents };
            state_panel::select_panel(&mut this.sidebar, panel, cx);
            crate::invalidation::notify(cx, Component::ReaderShell, "sidebar_tab_changed");
        })
    }

    pub(super) fn render_toc(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let toc = state_panel::reader_toc_tree(&self.toc_tree, &entity, theme, |link, window, this: &mut Self, cx| {
            this.navigate_to(link.to_owned(), cx);
            this.focus_document(window, cx);
            #[cfg(feature = "kobo")]
            {
                this.sidebar.close();
                crate::invalidation::notify(cx, Component::ReaderShell, "toc_navigated");
            }
        });
        components::reader_toc(theme).child(self.render_sidebar_tabs(false, cx)).child(toc).into_any_element()
    }

    pub(super) fn render_reader_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        components::reader_state_panel(theme)
            .child(self.render_sidebar_tabs(true, cx))
            .children(state_panel::reader_state_rows(
                &cx.entity(),
                &self.marks,
                "Select text, then use Highlight to create an annotation.",
                theme,
                |id, window, this: &mut Self, cx| {
                    this.annotation_popup_anchor = None;
                    this.select_annotation(id, window, cx);
                },
                |annotation: &ReaderAnnotation, this: &mut Self, cx| this.navigate_to_cfi(annotation.anchor.epub_cfi_value().unwrap_or_default().to_owned(), cx),
                |id, this: &mut Self, cx| this.delete_annotation(id, cx),
            ))
            .into_any_element()
    }
}
