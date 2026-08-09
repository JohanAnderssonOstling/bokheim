use std::time::{SystemTime, UNIX_EPOCH};

use bokheim_ui_components as components;
use epub_svelte_lib::library::ResolvedBook;
use epub_svelte_lib::library::marks::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation, ReaderBookmark};
use epub_svelte_lib::{BookLocator, LibraryBackend};
use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Focusable, KeyBinding, Render, ScrollStrategy, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::tree::{TreeItem, TreeState, tree};
use pdf_reader_core::{NormalizedRect, PdfOutlineEntry};
use pdf_view_gpui::{MAX_VISIBLE_PAGE_COUNT, MIN_VISIBLE_PAGE_COUNT, PdfAnnotationOverlay, PdfAnnotationStyle, PdfFitMode, PdfView, PdfViewEvent};

use crate::epub::ReaderWindowId;
use crate::epub::marks::MarksState;
use crate::invalidation::Component;
use crate::pdf::toc::{PdfTocState, nest_pdf_outline, pdf_toc_tree_items};
use crate::settings::ReaderSettings;
use crate::shell::chrome;
use crate::shell::persistence::{self, PositionWriter};
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextPage, PreviousPage, ToggleSearch, ToggleToc};

pub(crate) struct PendingPdfReader {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) title: String,
    pub(crate) library: LibraryBackend,
    pub(crate) book: ResolvedBook,
    pub(crate) initial_page: usize,
}

pub(crate) struct PdfReaderView {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) unavailable: bool,
    title: String,
    library: LibraryBackend,
    pdf: Entity<PdfView>,
    search_input: Entity<InputState>,
    search_active: bool,
    search_match: (usize, usize),
    toc_tree: Entity<TreeState>,
    toc: PdfTocState,
    toc_visible: bool,
    annotations_visible: bool,
    /// Whether the side panel is showing bookmarks rather than annotations.
    showing_bookmarks: bool,
    marks: MarksState,
    annotation_note_input: Entity<InputState>,
    current_page: usize,
    page_count: usize,
    positions: PositionWriter<usize>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PdfReaderView {
    pub(crate) fn new(pending: PendingPdfReader, window: &mut Window, cx: &mut Context<Self>) -> Self {
        #[cfg(debug_assertions)]
        crate::invalidation::initialize_performance_measurements();
        let visible_page_count = ReaderSettings::preferences(cx).pdf_page_count as usize;
        let trim_margins = ReaderSettings::preferences(cx).pdf_trim_margins;
        let library = pending.library.clone();
        let content_hash = pending.locator.content_hash();
        let annotations = library.annotations(content_hash.clone()).map(|items| items.into_iter().filter(|item| item.pdf_anchor().is_some()).collect::<Vec<_>>()).unwrap_or_else(|error| {
            log::warn!("failed to load PDF annotations for book {content_hash}: {error}");
            Vec::new()
        });
        let bookmarks = library.bookmarks(content_hash.clone()).unwrap_or_else(|error| {
            log::warn!("failed to load PDF bookmarks for book {content_hash}: {error}");
            Vec::new()
        });
        let pdf = cx.new(|cx| {
            let mut pdf = PdfView::from_reader(pending.book.path, pending.book.reader, pending.initial_page, window, cx);
            pdf.set_visible_page_count(visible_page_count, cx);
            pdf.set_trim_margins(trim_margins, cx);
            pdf.set_annotations(annotation_overlays(&annotations), cx);
            pdf
        });
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        let annotation_note_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a note"));
        let pdf_subscription = cx.subscribe(&pdf, |this, pdf, event: &PdfViewEvent, cx| match event {
            PdfViewEvent::PageChanged { current, total } => {
                if !this.toc.loaded {
                    let outline = pdf.read(cx).info().map(|info| info.outline().to_vec()).unwrap_or_default();
                    this.set_toc(&outline, cx);
                }
                let unchanged = this.current_page == *current && this.page_count == *total;
                this.current_page = *current;
                this.page_count = *total;
                this.update_active_toc(current.saturating_sub(1), cx);
                if unchanged {
                    return;
                }
                this.positions.record(current.saturating_sub(1), "PDF page");
                crate::invalidation::notify(cx, Component::PdfShell, "page_changed");
            }
            PdfViewEvent::SearchChanged { current, total } => {
                if this.search_match == (*current, *total) {
                    return;
                }
                this.search_match = (*current, *total);
                crate::invalidation::notify(cx, Component::PdfShell, "match_changed");
            }
            PdfViewEvent::LoadFailed(error) => {
                log::error!("failed to load PDF reader {}: {error}", this.id.0);
            }
        });
        let pdf_for_search = pdf.clone();
        let search_subscription = cx.subscribe_in(&search_input, window, move |this, input, event: &InputEvent, _window, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                if query.is_empty() || query.chars().count() >= 2 {
                    pdf_for_search.update(cx, |pdf, cx| {
                        pdf.search(query, cx);
                    });
                }
            }
            InputEvent::PressEnter { shift, .. } => {
                this.navigate_search(if *shift { -1 } else { 1 }, cx);
            }
            _ => {}
        });
        let bounds_subscription = crate::shell::geometry::observe(window, cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let page_content_hash = pending.locator.content_hash();
        let positions = PositionWriter::new(cx, "persist PDF page", move |library, page: &usize| library.update_pdf_reading_page(page_content_hash.clone(), *page as u32).map_err(|error| error.to_string()), library.clone());
        Self {
            id: pending.id,
            locator: pending.locator,
            unavailable: false,
            title: pending.title,
            library,
            pdf,
            search_input,
            search_active: false,
            search_match: (0, 0),
            toc_tree,
            toc: PdfTocState::default(),
            toc_visible: ReaderSettings::preferences(cx).toc_visible,
            annotations_visible: false,
            showing_bookmarks: false,
            marks: MarksState { annotations, bookmarks, ..MarksState::default() },
            annotation_note_input,
            current_page: 0,
            page_count: 0,
            positions,
            focus,
            _subscriptions: vec![pdf_subscription, search_subscription, bounds_subscription],
        }
    }

    fn previous_page(&self, cx: &mut Context<Self>) {
        if !self.pdf.read(cx).can_previous_page() {
            return;
        }
        self.pdf.update(cx, |pdf, cx| {
            pdf.previous_page(cx);
        });
    }

    fn next_page(&self, cx: &mut Context<Self>) {
        if !self.pdf.read(cx).can_next_page() {
            return;
        }
        self.pdf.update(cx, |pdf, cx| {
            pdf.next_page(cx);
        });
    }

    fn navigate_search(&self, direction: i8, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| {
            pdf.navigate_search(direction, cx);
        });
    }

    fn set_toc(&mut self, outline: &[PdfOutlineEntry], cx: &mut Context<Self>) {
        let nodes = nest_pdf_outline(outline);
        let mut targets = Vec::new();
        let items = pdf_toc_tree_items(&nodes, &mut targets);
        self.toc_tree.update(cx, |tree, cx| tree.set_items(items, cx));
        self.toc.targets = targets;
        self.toc.loaded = true;
    }

    fn update_active_toc(&mut self, page_index: usize, cx: &mut Context<Self>) {
        let next = self.toc.entry_at(page_index);
        if self.toc.active_id == next {
            return;
        }
        self.toc_tree.update(cx, |tree, cx| {
            if let Some(id) = next.as_ref() {
                let selected = TreeItem::new(id.clone(), id.clone());
                tree.set_selected_item(Some(&selected), cx);
                tree.reveal_item(&selected.id, ScrollStrategy::Center, cx);
            } else {
                tree.set_selected_item(None, cx);
            }
        });
        self.toc.active_id = next;
    }

    fn navigate_to_toc(&self, id: &str, cx: &mut Context<Self>) {
        let Some(page_index) = self.toc.page_of(id) else { return };
        self.pdf.update(cx, |pdf, cx| pdf.set_page(page_index, cx));
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_active && self.search_input.read(cx).value().is_empty() {
            return;
        }
        self.search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.search_active = false;
        self.search_match = (0, 0);
        self.pdf.update(cx, |pdf, cx| {
            pdf.search(String::new(), cx);
        });
        self.focus.focus(window, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn adjust_scale(&self, delta: f32, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| {
            pdf.change_display_scale(delta, cx);
        });
    }

    fn set_fit_mode(&self, fit_mode: PdfFitMode, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| pdf.set_fit_mode(fit_mode, cx));
        crate::invalidation::notify(cx, Component::PdfShell, "fit_mode_changed");
    }

    fn adjust_visible_page_count(&self, delta: isize, cx: &mut Context<Self>) {
        let current = self.pdf.read(cx).visible_page_count();
        let next = current.saturating_add_signed(delta).clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT);
        if current == next {
            return;
        }
        ReaderSettings::update(cx, |preferences| preferences.pdf_page_count = next as u8);
        self.pdf.update(cx, |pdf, cx| {
            pdf.set_visible_page_count(next, cx);
        });
        crate::invalidation::notify(cx, Component::PdfShell, "page_count_changed");
    }

    fn set_trim_margins(&self, trim_margins: bool, cx: &mut Context<Self>) {
        if !ReaderSettings::update(cx, |preferences| preferences.pdf_trim_margins = trim_margins) {
            return;
        }
        self.pdf.update(cx, |pdf, cx| {
            pdf.set_trim_margins(trim_margins, cx);
        });
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn now_millis() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as i64).unwrap_or(0)
    }

    fn refresh_annotation_overlays(&self, cx: &mut Context<Self>) {
        let overlays = annotation_overlays(&self.marks.annotations);
        self.pdf.update(cx, |pdf, cx| pdf.set_annotations(overlays, cx));
    }

    fn create_annotation(&mut self, open_notes: bool, cx: &mut Context<Self>) {
        let Some(selection) = self.pdf.read(cx).selection_snapshot() else { return };
        let rects = selection.rects.iter().map(|rect| PdfAnnotationRect::new(rect.left(), rect.top(), rect.width(), rect.height())).collect::<Vec<_>>();
        let anchor = PdfAnnotationAnchor::new(selection.page_index as u32, rects, None);
        let anchor = AnnotationAnchor::pdf(anchor);
        let now = Self::now_millis();
        let annotation = ReaderAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            content_hash: self.locator.content_hash().to_string(),
            anchor,
            exact_text: selection.text,
            style: AnnotationStyle::Highlight,
            color: "#f6c945".to_owned(),
            note: String::new(),
            created_at: now,
            modified_at: now,
        };
        let library = self.library.clone();
        let stored = annotation.clone();
        persistence::write(
            cx,
            library,
            "save PDF annotation",
            move |library| library.upsert_annotation(&stored).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                if open_notes {
                    this.marks.selected_id = Some(annotation.id.clone());
                    this.annotations_visible = true;
                    this.toc_visible = false;
                }
                this.marks.annotations.insert(0, annotation);
                this.refresh_annotation_overlays(cx);
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            },
        );
    }

    fn select_annotation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.marks.annotation(&id).map(|annotation| annotation.note.clone()).unwrap_or_default();
        self.marks.selected_id = Some(id);
        self.annotation_note_input.update(cx, |input, cx| input.set_value(note, window, cx));
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn save_selected_annotation(&mut self, style: Option<AnnotationStyle>, cx: &mut Context<Self>) {
        let Some(id) = self.marks.selected_id.clone() else { return };
        let Some(mut annotation) = self.marks.annotation(&id).cloned() else { return };
        annotation.note = self.annotation_note_input.read(cx).value().to_string();
        if let Some(style) = style {
            annotation.style = style;
        }
        annotation.modified_at = Self::now_millis();
        let stored = annotation.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "update PDF annotation",
            move |library| library.upsert_annotation(&stored).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.replace(annotation);
                this.refresh_annotation_overlays(cx);
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            },
        );
    }

    fn delete_annotation(&mut self, id: String, cx: &mut Context<Self>) {
        let modified_at = Self::now_millis();
        let stored_id = id.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "delete PDF annotation",
            move |library| library.delete_annotation(&stored_id, modified_at).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.remove_annotation(&id);
                this.refresh_annotation_overlays(cx);
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            },
        );
    }

    fn navigate_to_annotation(&self, annotation: &ReaderAnnotation, cx: &mut Context<Self>) {
        let Some(anchor) = annotation.pdf_anchor() else { return };
        let Some(rect) = anchor.rects.first() else { return };
        let bounds = NormalizedRect::new(rect.left, rect.top, rect.width, rect.height);
        self.pdf.update(cx, |pdf, cx| pdf.reveal_annotation(anchor.page_index as usize, bounds, cx));
    }

    /// The outline entry the current page sits under, if the PDF has one.
    fn active_outline_title(&self) -> Option<&str> {
        self.toc.active_title()
    }

    /// How a bookmark for the current page is named in the list.
    fn bookmark_label(&self) -> String {
        bookmark_label(self.current_page.max(1), self.active_outline_title())
    }

    fn add_bookmark(&mut self, cx: &mut Context<Self>) {
        let now = Self::now_millis();
        let bookmark =
            ReaderBookmark { id: uuid::Uuid::new_v4().to_string(), content_hash: self.locator.content_hash().to_string(), cfi: pdf_page_cfi(self.current_page.saturating_sub(1)), label: self.bookmark_label(), created_at: now, modified_at: now };
        let stored = bookmark.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "save PDF bookmark",
            move |library| library.create_bookmark(&stored).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.bookmarks.push(bookmark);
                this.annotations_visible = true;
                this.showing_bookmarks = true;
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            },
        );
    }

    fn delete_bookmark(&mut self, id: String, cx: &mut Context<Self>) {
        let deleted_at = Self::now_millis();
        let stored_id = id.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "delete PDF bookmark",
            move |library| library.delete_bookmark(&stored_id, deleted_at).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.remove_bookmark(&id);
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            },
        );
    }

    fn navigate_to_bookmark(&self, cfi: &str, cx: &mut Context<Self>) {
        let Some(page_index) = pdf_page_from_cfi(cfi) else { return };
        self.pdf.update(cx, |pdf, cx| pdf.set_page(page_index, cx));
    }

    /// The annotations/bookmarks sidebar. Tabs mirror the EPUB reader so the
    /// two behave the same way.
    fn render_annotations(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut panel = components::reader_state_panel().child(
            components::reader_state_tabs()
                .child(components::reader_choice_button("pdf-annotations-tab", format!("Annotations ({})", self.marks.annotations.len()), !self.showing_bookmarks).on_click(cx.listener(|this, _, _, cx| {
                    this.showing_bookmarks = false;
                    crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
                })))
                .child(components::reader_choice_button("pdf-bookmarks-tab", format!("Bookmarks ({})", self.marks.bookmarks.len()), self.showing_bookmarks).on_click(cx.listener(|this, _, _, cx| {
                    this.showing_bookmarks = true;
                    crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
                }))),
        );

        if self.showing_bookmarks {
            panel = panel.child(components::reader_control_button("pdf-bookmark-current", "Bookmark here").tooltip("Bookmark the current page").on_click(cx.listener(|this, _, _, cx| this.add_bookmark(cx))));
            if self.marks.bookmarks.is_empty() {
                panel = panel.child(components::reader_state_note("No bookmarks yet. Use Bookmark here to save this page."));
            }
            for (index, bookmark) in self.marks.bookmarks.iter().cloned().enumerate() {
                let cfi = bookmark.cfi.clone();
                let id = bookmark.id.clone();
                panel = panel.child(
                    components::reader_state_card(("pdf-bookmark", index), false).child(components::reader_state_quote(bookmark.label)).child(
                        components::reader_state_actions()
                            .child(components::reader_control_button(("pdf-bookmark-go", index), "Go").on_click(cx.listener(move |this, _, _, cx| this.navigate_to_bookmark(&cfi, cx))))
                            .child(components::reader_control_button(("pdf-bookmark-delete", index), "Delete").on_click(cx.listener(move |this, _, _, cx| this.delete_bookmark(id.clone(), cx)))),
                    ),
                );
            }
            return panel.into_any_element();
        }

        if self.marks.annotations.is_empty() {
            panel = panel.child(components::reader_state_note("Select PDF text, then use Highlight or Note."));
        }
        for (index, annotation) in self.marks.annotations.iter().cloned().enumerate() {
            let selected = self.marks.selected_id.as_deref() == Some(annotation.id.as_str());
            let select_id = annotation.id.clone();
            let delete_id = annotation.id.clone();
            let navigate_annotation = annotation.clone();
            let note = annotation.note.clone();
            let mut card = components::reader_state_card(("pdf-annotation", index), selected)
                .on_click(cx.listener(move |this, _, window, cx| this.select_annotation(select_id.clone(), window, cx)))
                .child(components::reader_state_quote(format!("“{}”", annotation.exact_text)))
                .when(!note.is_empty(), |card| card.child(components::reader_state_note(note)))
                .child(
                    components::reader_state_actions()
                        .child(components::reader_control_button(("pdf-annotation-go", index), "Go").on_click(cx.listener(move |this, _, _, cx| this.navigate_to_annotation(&navigate_annotation, cx))))
                        .child(components::reader_control_button(("pdf-annotation-delete", index), "Delete").on_click(cx.listener(move |this, _, _, cx| this.delete_annotation(delete_id.clone(), cx)))),
                );
            if selected {
                card = card.child(components::reader_note_editor().child(Input::new(&self.annotation_note_input).appearance(false).size_full())).child(
                    components::reader_state_actions()
                        .child(components::reader_control_button(("pdf-annotation-save", index), "Save note").on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, cx))))
                        .child(components::reader_control_button(("pdf-annotation-highlight", index), "Highlight").on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(Some(AnnotationStyle::Highlight), cx))))
                        .child(components::reader_control_button(("pdf-annotation-underline", index), "Underline").on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(Some(AnnotationStyle::Underline), cx))))
                        .child(components::reader_control_button(("pdf-annotation-strike", index), "Strike").on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(Some(AnnotationStyle::Strikethrough), cx)))),
                );
            }
            panel = panel.child(card);
        }
        panel.into_any_element()
    }

    fn render_toc(&self, entity: Entity<Self>) -> gpui::AnyElement {
        let toc = tree(&self.toc_tree, move |index, entry, _selected, _window, _cx| {
            let item = entry.item();
            let id = item.id.to_string();
            let navigation_target = entity.clone();
            let title = components::reader_toc_title(("pdf-toc-tree-title", index), item.label.clone()).on_click(move |_, _, cx| {
                navigation_target.update(cx, |this, cx| this.navigate_to_toc(&id, cx));
            });
            components::reader_toc_tree_item(("pdf-toc-tree-item", index), ("pdf-toc-tree-disclosure", index), entry.depth(), entry.is_folder(), entry.is_expanded(), title)
        });
        components::reader_toc().child(toc).into_any_element()
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let scale = self.pdf.read(cx).display_scale();
        let fit_mode = self.pdf.read(cx).fit_mode();
        let visible_page_count = self.pdf.read(cx).visible_page_count();
        let trim_margins = ReaderSettings::preferences(cx).pdf_trim_margins;
        components::reader_settings_dropdown()
            .child(components::reader_settings_title())
            .child(
                components::reader_setting_row("Pages")
                    .child(components::reader_square_button("pdf-pages-decrease", "−").on_click(cx.listener(|this, _, _, cx| this.adjust_visible_page_count(-1, cx))))
                    .child(components::reader_setting_value(visible_page_count.to_string()))
                    .child(components::reader_square_button("pdf-pages-increase", "+").on_click(cx.listener(|this, _, _, cx| this.adjust_visible_page_count(1, cx)))),
            )
            .child(components::reader_setting_row("Margins").child(components::reader_choice_button_group(
                "pdf-margin-group",
                [
                    components::reader_choice_button("pdf-margin-off", "Off", !trim_margins).on_click(cx.listener(|this, _, _, cx| this.set_trim_margins(false, cx))),
                    components::reader_choice_button("pdf-margin-auto", "Auto", trim_margins).on_click(cx.listener(|this, _, _, cx| this.set_trim_margins(true, cx))),
                ],
            )))
            .child(components::reader_setting_row("Fit").child(components::reader_choice_button_group(
                "pdf-fit-group",
                [
                    components::reader_choice_button("pdf-fit-width", "Fit width", fit_mode == PdfFitMode::FitWidth).on_click(cx.listener(|this, _, _, cx| this.set_fit_mode(PdfFitMode::FitWidth, cx))),
                    components::reader_choice_button("pdf-fit-page", "Fit page", fit_mode == PdfFitMode::FitPage).on_click(cx.listener(|this, _, _, cx| this.set_fit_mode(PdfFitMode::FitPage, cx))),
                ],
            )))
            .child(
                components::reader_setting_row("Zoom")
                    .child(components::reader_square_button("pdf-zoom-out", "−").tooltip("Zoom out (Ctrl+-)").on_click(cx.listener(|this, _, _, cx| this.adjust_scale(-0.1, cx))))
                    .child(components::reader_setting_value(format!("{:.0}%", scale * 100.0)))
                    .child(components::reader_square_button("pdf-zoom-in", "+").tooltip("Zoom in (Ctrl++)").on_click(cx.listener(|this, _, _, cx| this.adjust_scale(0.1, cx)))),
            )
            .into_any_element()
    }

    fn on_previous_page(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        self.previous_page(cx);
    }

    fn on_next_page(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        self.next_page(cx);
    }

    fn on_toggle_toc(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.set_toc_visible(!self.toc_visible, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn set_toc_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.toc_visible = visible;
        ReaderSettings::update(cx, |preferences| preferences.toc_visible = visible);
    }

    fn on_focus_search(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_active {
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        self.search_active = true;
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn on_toggle_search(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_active {
            self.close_search(window, cx);
        } else {
            self.search_active = true;
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
            crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
        }
    }

    fn on_close_search(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.close_search(window, cx);
    }

    fn on_increase_scale(&mut self, _: &IncreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(0.1, cx);
    }

    fn on_decrease_scale(&mut self, _: &DecreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(-0.1, cx);
    }
}

impl Focusable for PdfReaderView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PdfReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::invalidation::record_render(Component::PdfShell);
        window.set_window_title(&self.title);
        let entity = cx.entity();
        let outline_available = self.pdf.read(cx).info().is_some_and(|info| !info.outline().is_empty());
        let sidebar_visible = self.annotations_visible || (self.toc_visible && outline_available);
        let toc_width = chrome::sidebar_width(sidebar_visible, cx);
        let available_width = (f32::from(window.viewport_size().width) - toc_width).max(0.0);
        self.pdf.update(cx, |pdf, cx| {
            pdf.set_available_width(available_width, cx);
        });
        let (can_previous, can_next, visible_page_range, selection_available) = {
            let pdf = self.pdf.read(cx);
            (pdf.can_previous_page(), pdf.can_next_page(), pdf.visible_page_range(), pdf.selection_snapshot().is_some())
        };
        let page_indicator = format_page_indicator(visible_page_range, self.page_count);

        let mut body = components::reader_document_body().child(components::reader_content().child(self.pdf.clone()));
        if self.search_active {
            body = body.child(
                components::reader_search_bar()
                    .child(components::reader_search_box().key_context("ReaderSearch").child(Input::new(&self.search_input).appearance(false).size_full()))
                    .child(components::reader_match_count(format!("{}/{}", self.search_match.0, self.search_match.1)))
                    .child(components::reader_square_button("pdf-search-previous", "‹").on_click(cx.listener(|this, _, _, cx| this.navigate_search(-1, cx))))
                    .child(components::reader_square_button("pdf-search-next", "›").on_click(cx.listener(|this, _, _, cx| this.navigate_search(1, cx))))
                    .child(components::reader_square_button("pdf-search-close", "×").on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))),
            );
        }
        let content = if self.annotations_visible {
            chrome::with_sidebar(self.render_annotations(cx), body.into_any_element(), cx)
        } else if self.toc_visible && outline_available {
            chrome::with_sidebar(self.render_toc(entity.clone()), body.into_any_element(), cx)
        } else {
            body.into_any_element()
        };
        let settings = self.render_settings(cx);
        let toolbar = components::reader_toolbar()
            .child(components::reader_control_button_state("pdf-previous-page", "←", can_previous).tooltip("Previous Fit Width column").on_click(cx.listener(|this, _, _, cx| this.previous_page(cx))))
            .child(components::reader_match_count(page_indicator))
            .child(components::reader_control_button_state("pdf-next-page", "→", can_next).tooltip("Next Fit Width column").on_click(cx.listener(|this, _, _, cx| this.next_page(cx))))
            .when(outline_available, |toolbar| {
                toolbar.child(components::reader_control_button_icon("pdf-contents", "Contents", gpui_component::IconName::BookOpen).on_click(cx.listener(|this, _, _, cx| {
                    this.set_toc_visible(!this.toc_visible, cx);
                    if this.toc_visible {
                        this.annotations_visible = false;
                    }
                    crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
                })))
            })
            .child(components::reader_title(self.title.clone()))
            .child(
                components::reader_control_button_state("pdf-highlight-selection", "Highlight", selection_available)
                    .opacity(if selection_available { 1.0 } else { 0.0 })
                    .tooltip("Save selected PDF text as a highlight")
                    .on_click(cx.listener(|this, _, _, cx| this.create_annotation(false, cx))),
            )
            .child(
                components::reader_control_button_state("pdf-note-selection", "Note", selection_available)
                    .opacity(if selection_available { 1.0 } else { 0.0 })
                    .tooltip("Highlight selected PDF text and open annotations")
                    .on_click(cx.listener(|this, _, _, cx| this.create_annotation(true, cx))),
            )
            .child(components::reader_control_button_icon("pdf-annotations", format!("Notes ({})", self.marks.annotations.len()), gpui_component::IconName::File).on_click(cx.listener(|this, _, _, cx| {
                this.annotations_visible = !this.annotations_visible;
                if this.annotations_visible {
                    this.toc_visible = false;
                }
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            })))
            .child(components::reader_control_button("pdf-search", "Search").on_click(cx.listener(|this, _, window, cx| {
                if !this.search_active {
                    this.search_active = true;
                    crate::invalidation::notify(cx, Component::PdfShell, "search_opened");
                }
                this.search_input.read(cx).focus_handle(cx).focus(window, cx);
            })))
            .child(components::reader_settings_popover().child(settings));

        components::page()
            .key_context("Reader")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_previous_page))
            .on_action(cx.listener(Self::on_next_page))
            .on_action(cx.listener(Self::on_toggle_toc))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_toggle_search))
            .on_action(cx.listener(Self::on_close_search))
            .on_action(cx.listener(Self::on_increase_scale))
            .on_action(cx.listener(Self::on_decrease_scale))
            .child(toolbar)
            .child(components::reader_content().child(content))
    }
}

/// Names a bookmark after its page, qualified by the outline section it falls
/// under when the PDF has one.
///
/// `page` is one-based, as shown to the reader.
fn bookmark_label(page: usize, outline_title: Option<&str>) -> String {
    let page = format!("Page {page}");
    match outline_title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => format!("{page} \u{2014} {title}"),
        None => page,
    }
}

/// Encodes a zero-based page as the synthetic spine CFI used for page-indexed
/// formats.
///
/// `ReaderBookmark::cfi` is a durable, synchronized field, so paginated
/// formats that have no real CFI share one scheme: spine step `(page + 1) * 2`,
/// matching the CBZ reader. Bookmarks are scoped to a book, so the two formats
/// never collide.
fn pdf_page_cfi(page_index: usize) -> String {
    format!("epubcfi(/6/{})", page_index.saturating_add(1).saturating_mul(2))
}

/// Reads a page back out of [`pdf_page_cfi`], returning `None` for anything
/// that is not one of those synthetic locators.
fn pdf_page_from_cfi(value: &str) -> Option<usize> {
    let start = value.find("/6/")?;
    let digits = value[start + 3..].chars().take_while(char::is_ascii_digit).collect::<String>();
    let step = digits.parse::<usize>().ok()?;
    step.checked_div(2)?.checked_sub(1)
}

fn annotation_overlays(annotations: &[ReaderAnnotation]) -> Vec<PdfAnnotationOverlay> {
    annotations
        .iter()
        .filter_map(|annotation| {
            let anchor = annotation.pdf_anchor()?;
            let style = match annotation.style {
                AnnotationStyle::Highlight => PdfAnnotationStyle::Highlight,
                AnnotationStyle::Underline => PdfAnnotationStyle::Underline,
                AnnotationStyle::Squiggly => PdfAnnotationStyle::Squiggly,
                AnnotationStyle::Strikethrough => PdfAnnotationStyle::Strikethrough,
            };
            let rects = anchor.rects.into_iter().map(|rect| NormalizedRect::new(rect.left, rect.top, rect.width, rect.height)).collect();
            Some(PdfAnnotationOverlay { id: annotation.id.clone(), page_index: anchor.page_index as usize, rects, style, color: annotation_color(&annotation.color, style) })
        })
        .collect()
}

fn annotation_color(value: &str, style: PdfAnnotationStyle) -> u32 {
    let rgb = value.strip_prefix('#').and_then(|hex| u32::from_str_radix(hex, 16).ok()).filter(|_| value.len() == 7).unwrap_or(0xf6c945);
    let alpha = if style == PdfAnnotationStyle::Highlight { 0x60 } else { 0xff };
    (rgb << 8) | alpha
}

fn format_page_indicator(visible_page_range: Option<(usize, usize)>, page_count: usize) -> String {
    let Some((first, last)) = visible_page_range else {
        return format!("0/{page_count}");
    };
    if first == last { format!("{}/{page_count}", first + 1) } else { format!("{}–{}/{page_count}", first + 1, last + 1) }
}

pub(crate) fn bind_pdf_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousPage, Some("PdfReaderDocument")),
        KeyBinding::new("right", NextPage, Some("PdfReaderDocument")),
        KeyBinding::new("up", PreviousPage, Some("PdfReaderDocument")),
        KeyBinding::new("down", NextPage, Some("PdfReaderDocument")),
        KeyBinding::new("pageup", PreviousPage, Some("PdfReaderDocument")),
        KeyBinding::new("pagedown", NextPage, Some("PdfReaderDocument")),
        KeyBinding::new("ctrl-=", IncreaseScale, Some("PdfReaderDocument")),
        KeyBinding::new("ctrl--", DecreaseScale, Some("PdfReaderDocument")),
    ]);
}

#[cfg(test)]
mod tests {
    use super::{annotation_color, bookmark_label, format_page_indicator, pdf_page_cfi, pdf_page_from_cfi};
    use pdf_view_gpui::PdfAnnotationStyle;

    #[test]
    fn page_indicator_reports_single_pages_and_spreads() {
        assert_eq!(format_page_indicator(None, 0), "0/0");
        assert_eq!(format_page_indicator(Some((0, 0)), 12), "1/12");
        assert_eq!(format_page_indicator(Some((1, 2)), 12), "2–3/12");
    }

    #[test]
    fn annotation_colors_use_translucent_highlights_and_opaque_rules() {
        assert_eq!(annotation_color("#f6c945", PdfAnnotationStyle::Highlight), 0xf6c94560);
        assert_eq!(annotation_color("#f6c945", PdfAnnotationStyle::Underline), 0xf6c945ff);
        assert_eq!(annotation_color("invalid", PdfAnnotationStyle::Highlight), 0xf6c94560);
    }

    #[test]
    fn page_bookmarks_round_trip_through_their_locator() {
        for page in [0usize, 1, 41, 339] {
            assert_eq!(pdf_page_from_cfi(&pdf_page_cfi(page)), Some(page), "page {page} must survive a round trip");
        }
    }

    #[test]
    fn page_locators_match_the_cbz_scheme() {
        // Both paginated readers write the same synthetic spine steps, so a
        // locator stays readable if a book's format is ever reclassified.
        assert_eq!(pdf_page_cfi(0), "epubcfi(/6/2)");
        assert_eq!(pdf_page_cfi(4), "epubcfi(/6/10)");
    }

    #[test]
    fn a_real_epub_locator_is_not_mistaken_for_a_page() {
        assert_eq!(pdf_page_from_cfi("not a locator"), None);
        assert_eq!(pdf_page_from_cfi("epubcfi(/6/0)"), None, "spine step 0 is below the first page");
    }

    #[test]
    fn a_bookmark_is_named_after_its_page_and_section() {
        assert_eq!(bookmark_label(42, Some("Method")), "Page 42 \u{2014} Method");
    }

    #[test]
    fn a_pdf_without_a_usable_outline_falls_back_to_the_page_alone() {
        assert_eq!(bookmark_label(42, None), "Page 42");
        assert_eq!(bookmark_label(42, Some("   ")), "Page 42", "a blank outline title must not leave a dangling dash");
    }
}
