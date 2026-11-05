//! EPUB contents, annotation, and settings sidebar tabs.

use ui_text::text;

use crate::invalidation::notify;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, IntoElement, Window, div};
use gpui_component::input::Input;
use library_backend::Annotation;
use std::rc::Rc;
use ui_components as components;

use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::shell::annotation_tree::AnnotationTreeTarget;
use crate::shell::bookmarks::{self, BookmarkTarget};
use crate::shell::marks::BookmarkChange;
use crate::shell::sidebar_list;
use crate::shell::state_panel::{self, AnnotationSidebarTarget, SidebarTab, TocActivation};
use crate::shell::toolbar::SidebarTarget;

impl AnnotationSidebarTarget for ReaderView {
    fn copy_sidebar_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>) {
        let section = annotation.toc_ordinal.and_then(|ordinal| usize::try_from(ordinal).ok()).and_then(|ordinal| self.toc.title_for_ordinal(ordinal));
        crate::shell::annotation_copy::copy_passage(&self.locator, &self.book_title, section.as_deref(), &annotation.exact_text, &annotation.note, Some(annotation.id.clone()), annotation.anchor.clone(), cx);
    }

    fn select_sidebar_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.annotation_popup_anchor = None;
        self.select_annotation(id, window, cx);
    }

    fn go_to_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>) {
        if let Some(cfi) = self.book.annotation_cfi(&annotation.anchor) {
            self.navigate_to_cfi(cfi, cx);
        }
    }

    fn delete_sidebar_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        self.delete_annotation(id, cx);
    }
}

impl AnnotationTreeTarget for ReaderView {
    fn set_annotation_branch_expanded(&mut self, id: usize, expanded: bool, cx: &mut Context<Self>) {
        if self.sidebar_tab() == SidebarTab::Bookmarks {
            self.bookmark_tree.set_expanded(id, expanded, self.marks.selected_id());
        } else {
            self.annotation_tree.set_expanded(id, expanded);
        }
        notify(cx, Component::ReaderShell, "annotation_branch_toggled");
    }
}

impl ReaderView {
    fn render_toc_body(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &App) -> AnyElement {
        let reset_entity = entity.clone();
        let reset_read: state_panel::TocReadReset = Rc::new(move |id, _, cx| {
            reset_entity.update(cx, |this, cx| {
                let Some(target) = this.toc.link_for_id(id).map(str::to_owned) else { return };
                this.toc.reset_chapter_read_status(&target);
                this.reading_positions.reset_chapter_read_status(target);
                cx.notify();
            });
        });
        let toc = state_panel::reader_toc_tree(
            &self.toc_tree,
            Some(self.toc_filter.read(cx).index()),
            &entity,
            |this, id| state_panel::TocRowStatus { active: this.toc.active_id() == Some(id), read: this.toc.is_read(id) },
            Some(reset_read),
            self.settings_navigation.keyboard_visible(),
            theme,
            cx,
            |id, activation, window, this: &mut Self, cx| {
                let Some(link) = this.toc.link_for_id(id).map(str::to_owned) else { return };
                this.navigate_to(link, cx);
                if activation == TocActivation::Pointer {
                    this.focus_sidebar_body(window, cx);
                    #[cfg(feature = "kobo")]
                    {
                        this.sidebar.close(cx);
                        this.focus_document(window, cx);
                        notify(cx, Component::ReaderShell, "toc_navigated");
                    }
                }
            },
        );
        let reset = components::contents_read_reset_button("reset-chapter-read-status").on_click(move |_, _, cx| {
            entity.update(cx, |this, cx| {
                this.reading_positions.reset_read_status();
                this.toc.read_status.reset();
                cx.notify();
            });
        });
        components::reader_toc(theme).child(self.toc_filter.clone()).child(gpui::div().flex_none().p(gpui::px(components::SPACE_SM)).child(reset)).child(toc).into_any_element()
    }

    fn render_annotations_body(&mut self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let mut tree = self.toc.annotation_tree(self.marks.annotations().iter().map(|annotation| {
            let origin = annotation.toc_ordinal.and_then(|ordinal| usize::try_from(ordinal).ok());
            if annotation.anchor.recurring_scope() == Some(book_model::RecurringScope::Chapter) { self.book.annotation_cfi(&annotation.anchor).and_then(|cfi| self.toc.chapter_for_cfi(&cfi)).or(origin) } else { origin }
        }));
        let query = self.annotation_filter_input.read(cx).value();
        tree.filter(self.marks.annotations(), &query);
        tree.arrange_recurring(self.marks.annotations());
        self.annotation_tree.set_filter(&query);
        let selected_id = self.marks.selected_id();
        let selected_index = self.marks.annotations().iter().position(|annotation| Some(annotation.id.as_str()) == selected_id);
        self.annotation_tree.reveal_selected(&tree, selected_id, selected_index);
        let empty_message = if self.marks.annotations().is_empty() { text!(reader::no_notes_yet_select_text_then_choose_annotation_to_add_a_note) } else { text!(reader::no_annotations_match_your_filter) };
        self.annotation_tree.prepare_list(&tree, self.marks.annotations(), selected_id, empty_message);
        let rows = sidebar_list::panel("epub-annotation-list", &cx.entity(), &self.annotation_tree.list.state, theme, |this, row, target, theme, _| {
            this.annotation_tree.render_row(target, row, this.marks.annotations(), this.marks.selected_id(), theme)
        });
        let filter = components::reader_search_bar(theme)
            .child(components::reader_search_box(theme).debug_selector(|| "reader-annotation-filter".into()).child(Input::new(&self.annotation_filter_input).appearance(false).cleanable(true).size_full()));
        div().size_full().min_h_0().min_w_0().flex().flex_col().child(div().flex_none().child(filter)).child(rows).into_any_element()
    }

    /// The selected panel's body; the shell supplies its rail and header.
    pub(super) fn render_sidebar(&mut self, active: SidebarTab, entity: Entity<Self>, theme: components::BrowserTheme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match active {
            SidebarTab::Contents => self.render_toc_body(entity, theme, cx),
            SidebarTab::Notes => self.render_annotations_body(theme, cx),
            SidebarTab::Bookmarks => self.render_bookmarks_body(theme, cx),
            SidebarTab::Search => self.render_search(theme, window, cx),
            SidebarTab::Settings => self.render_settings_panel(window, cx),
        }
    }
}

impl BookmarkTarget for ReaderView {
    fn filter_bookmarks(&mut self, cx: &mut Context<Self>) {
        self.bookmark_tree.filter(&self.bookmark_filter_input.read(cx).value(), self.marks.selected_id());
    }

    fn create_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let LoadState::Ready(renderer) = &self.load_state else { return };
        let Some(cfi) = renderer.read(cx).current_cfi() else { return };
        if !self.toc.is_loaded() {
            return;
        }
        self.save_selected_annotation(None, None, cx);
        self.close_annotation_popup(cx);
        let ordinal = self.toc.bookmark_ordinal_for_cfi(&cfi);
        let bookmark = bookmarks::bookmark(self.locator.book_id(), self.book.annotation_anchor(cfi), ordinal, book_model::layout_fraction_progress(self.progress.fraction));
        self.marks.start_bookmark(bookmark);
        self.annotation_note_input.update(cx, |input, cx| {
            input.set_placeholder(text!(reader::name_bookmark), window, cx);
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        if components::uses_mobile_reader_controls(window) {
            self.sidebar.close(cx);
        }
        cx.notify();
    }
}

impl ReaderView {
    pub(in crate::epub) fn update_bookmarks(&mut self, change: BookmarkChange, cx: &mut Context<Self>) {
        match change {
            BookmarkChange::Contents => self.refresh_bookmark_rows(cx),
            BookmarkChange::Selection => self.bookmark_tree.select(self.marks.selected_id()),
        }
    }

    /// Chapter membership changes only with bookmark data or the outline.
    pub(in crate::epub) fn refresh_bookmark_rows(&mut self, cx: &mut Context<Self>) {
        let bookmarks = self.marks.bookmarks();
        let hierarchy = self.toc.annotation_tree(bookmarks.iter().map(|bookmark| bookmark.toc_ordinal.and_then(|ordinal| usize::try_from(ordinal).ok())));
        self.bookmark_tree.replace(hierarchy, bookmarks, &self.bookmark_filter_input.read(cx).value(), self.marks.selected_id());
    }

    fn render_bookmarks_body(&self, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let rows = sidebar_list::panel("epub-bookmark-list", &cx.entity(), &self.bookmark_tree.state.list.state, theme, |this, row, target, theme, _| {
            this.bookmark_tree.render_row(target, row, this.marks.bookmarks(), this.marks.selected_id(), theme)
        });
        div().size_full().min_h_0().min_w_0().flex().flex_col().child(bookmarks::filter_bar(&self.bookmark_filter_input, theme)).child(rows).into_any_element()
    }
}
