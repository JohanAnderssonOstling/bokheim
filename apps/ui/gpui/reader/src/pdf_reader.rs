use epub_svelte_lib::library::ResolvedBook;
use epub_svelte_lib::library::marks::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation, ReaderBookmark};
use epub_svelte_lib::{BookLocator, LibraryBackend};
use gpui::prelude::*;
use gpui::{App, Context, Entity, FocusHandle, Focusable, KeyBinding, Pixels, Point, Render, ScrollStrategy, Subscription, Window, px};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::tree::{TreeItem, TreeState};
use pdf_reader_core::{NormalizedRect, PdfOutlineEntry};
use pdf_view_gpui::{MAX_VISIBLE_PAGE_COUNT, MIN_VISIBLE_PAGE_COUNT, PdfAnnotationOverlay, PdfAnnotationStyle, PdfView, PdfViewEvent, PdfZoomMode};
use ui_components as components;

use std::rc::Rc;

use crate::epub::ReaderWindowId;
use crate::invalidation::Component;
use crate::pdf::toc::{PdfTocState, nest_pdf_outline, pdf_toc_tree_items};
use crate::settings::{PdfZoomModePreference, ReaderPreferences, ReaderSettings};
use crate::shell::annotation_editor;
use crate::shell::chrome;
use crate::shell::marks::MarksState;
use crate::shell::persistence::{self, PositionWriter};
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, PreviousLine, PreviousPage, ToggleSearch, ToggleToc};

const PDF_READER_TOOLBAR_HEIGHT: f32 = 58.0;
/// One zoom step, in pixels per PDF point.
const ZOOM_STEP: f32 = 0.1;

pub(crate) struct PendingPdfReader {
    pub(crate) id: ReaderWindowId,
    pub(crate) locator: BookLocator,
    pub(crate) title: String,
    pub(crate) library: LibraryBackend,
    pub(crate) book: ResolvedBook,
    pub(crate) initial_page: usize,
    pub(crate) initial_full_page_position: f32,
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
    /// Matches the EPUB reading surface: page navigation gets the permanent
    /// toolbar out of the way until the pointer returns to the top edge.
    chrome_hidden: bool,
    /// Whether the side panel is showing bookmarks rather than annotations.
    showing_bookmarks: bool,
    marks: MarksState,
    annotation_note_input: Entity<InputState>,
    annotation_popup_anchor: Option<Point<Pixels>>,
    current_page: usize,
    page_count: usize,
    positions: PositionWriter<(usize, f32)>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PdfReaderView {
    pub(crate) fn new(pending: PendingPdfReader, window: &mut Window, cx: &mut Context<Self>) -> Self {
        #[cfg(debug_assertions)]
        crate::invalidation::initialize_performance_measurements();
        let zoom_mode = stored_zoom_mode(ReaderSettings::preferences(cx));
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
            pdf.set_zoom_mode(zoom_mode, cx);
            pdf.set_trim_margins(trim_margins, cx);
            pdf.set_annotations(annotation_overlays(&annotations), cx);
            pdf.set_full_page_position(pending.initial_full_page_position, cx);
            pdf
        });
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        let annotation_note_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a note"));
        let pdf_subscription = cx.subscribe_in(&pdf, window, |this, pdf, event: &PdfViewEvent, window, cx| match event {
            PdfViewEvent::PageChanged { current, total } => {
                let current_page = current.saturating_sub(1);
                let full_page_position = pdf.read(cx).full_page_position();
                if !this.toc.loaded {
                    let outline = pdf.read(cx).info().map(|info| info.outline().to_vec()).unwrap_or_default();
                    this.set_toc(&outline, cx);
                    // The shell took the keyboard before there were pages to
                    // focus. Hand it to the document now that one exists, so
                    // paging works without clicking the page first.
                    this.focus_document(window, cx);
                }
                let unchanged = this.current_page == current_page && this.page_count == *total;
                this.current_page = current_page;
                this.page_count = *total;
                this.update_active_toc(current_page, cx);
                if unchanged {
                    return;
                }
                this.positions.record((current_page, full_page_position), "PDF page");
                crate::invalidation::notify(cx, Component::PdfShell, "page_changed");
            }
            PdfViewEvent::SearchChanged { current, total } => {
                if this.search_match == (*current, *total) {
                    return;
                }
                this.search_match = (*current, *total);
                crate::invalidation::notify(cx, Component::PdfShell, "match_changed");
            }
            PdfViewEvent::AnnotationActivated { id, position } => {
                this.annotations_visible = true;
                this.toc_visible = false;
                this.showing_bookmarks = false;
                this.annotation_popup_anchor = Some(*position);
                this.select_annotation(id.clone(), window, cx);
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
        // Only the page elements establish `PdfReaderDocument`, and there are
        // none until the document has been laid out, so the shell holds the
        // keyboard until then. The first page report hands it over.
        focus.focus(window, cx);
        let page_content_hash = pending.locator.content_hash();
        let positions = PositionWriter::new(
            cx,
            "persist PDF page",
            move |library, (page, full_page_position): &(usize, f32)| library.update_pdf_reading_position(page_content_hash.clone(), *page as u32, *full_page_position).map_err(|error| error.to_string()),
            library.clone(),
        );
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
            chrome_hidden: false,
            showing_bookmarks: false,
            marks: MarksState { annotations, bookmarks, ..MarksState::default() },
            annotation_note_input,
            annotation_popup_anchor: None,
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
        // Focus is still on the search input that is about to disappear.
        self.focus_document(window, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    /// Returns keyboard control to the page view.
    ///
    /// Navigation is bound in the `PdfReaderDocument` context, which only the
    /// PDF view establishes while it holds focus, so anything that moves focus
    /// elsewhere has to hand it back or the paging keys stop working until the
    /// page is clicked.
    fn focus_document(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.pdf.read(cx).focus_handle().focus(window, cx);
    }

    /// Nudges the zoom, which also switches the layout to the mode where the
    /// zoom is what is held fixed and the page count follows.
    fn adjust_zoom(&self, delta: f32, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| pdf.change_zoom(delta, cx));
        let zoom = self.pdf.read(cx).zoom();
        ReaderSettings::update(cx, |preferences| {
            preferences.pdf_zoom_mode = PdfZoomModePreference::Fixed;
            preferences.pdf_zoom = zoom;
        });
        crate::invalidation::notify(cx, Component::PdfShell, "zoom_changed");
    }

    fn set_zoom_mode(&self, zoom_mode: PdfZoomMode, cx: &mut Context<Self>) {
        self.pdf.update(cx, |pdf, cx| pdf.set_zoom_mode(zoom_mode, cx));
        ReaderSettings::update(cx, |preferences| store_zoom_mode(zoom_mode, preferences));
        crate::invalidation::notify(cx, Component::PdfShell, "zoom_mode_changed");
    }

    /// Sets the page count, which also switches the layout to the mode where
    /// the count is what is held fixed and the zoom follows.
    fn adjust_visible_page_count(&self, delta: isize, cx: &mut Context<Self>) {
        let current = self.pdf.read(cx).visible_page_count();
        let next = current.saturating_add_signed(delta).clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT);
        if current == next {
            return;
        }
        self.set_zoom_mode(PdfZoomMode::FitWidth { columns: next }, cx);
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

    fn reset_appearance(&self, cx: &mut Context<Self>) {
        let defaults = ReaderPreferences::default();
        ReaderSettings::update(cx, |preferences| {
            preferences.pdf_page_count = defaults.pdf_page_count;
            preferences.pdf_trim_margins = defaults.pdf_trim_margins;
            preferences.pdf_zoom_mode = defaults.pdf_zoom_mode;
            preferences.pdf_zoom = defaults.pdf_zoom;
        });
        self.pdf.update(cx, |pdf, cx| {
            pdf.set_zoom_mode(stored_zoom_mode(&defaults), cx);
            pdf.set_trim_margins(defaults.pdf_trim_margins, cx);
        });
        crate::invalidation::notify(cx, Component::PdfShell, "appearance_reset");
    }

    fn refresh_annotation_overlays(&self, cx: &mut Context<Self>) {
        let overlays = annotation_overlays(&self.marks.annotations);
        self.pdf.update(cx, |pdf, cx| pdf.set_annotations(overlays, cx));
    }

    fn create_annotation(&mut self, open_notes: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.pdf.read(cx).selection_snapshot() else { return };
        let rects = selection.rects.iter().map(|rect| PdfAnnotationRect::new(rect.left(), rect.top(), rect.width(), rect.height())).collect::<Vec<_>>();
        let anchor = PdfAnnotationAnchor::new(selection.page_index as u32, rects, None);
        let anchor = AnnotationAnchor::pdf(anchor);
        let now = crate::shell::state_panel::now_millis();
        let annotation = ReaderAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            content_hash: self.locator.content_hash(),
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
        if open_notes {
            self.annotation_popup_anchor = Some(window.mouse_position());
            self.annotation_note_input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
            self.annotation_note_input.read(cx).focus_handle(cx).focus(window, cx);
        }
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
                    this.showing_bookmarks = false;
                    this.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(Some(annotation.id.clone()), cx));
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
        self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(self.marks.selected_id.clone(), cx));
        self.annotation_note_input.update(cx, |input, cx| input.set_value(note, window, cx));
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn close_annotation_editor(&mut self, cx: &mut Context<Self>) {
        self.marks.selected_id = None;
        self.annotation_popup_anchor = None;
        self.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(None, cx));
        crate::invalidation::notify(cx, Component::PdfShell, "annotation_editor_closed");
    }

    fn save_selected_annotation(&mut self, style: Option<AnnotationStyle>, color: Option<&'static str>, cx: &mut Context<Self>) {
        let Some(id) = self.marks.selected_id.clone() else { return };
        let Some(mut annotation) = self.marks.annotation(&id).cloned() else { return };
        annotation.note = self.annotation_note_input.read(cx).value().to_string();
        if let Some(style) = style {
            annotation.style = style;
        }
        if let Some(color) = color {
            annotation.color = color.to_owned();
        }
        annotation.modified_at = crate::shell::state_panel::now_millis();
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
        let modified_at = crate::shell::state_panel::now_millis();
        let stored_id = id.clone();
        persistence::write(
            cx,
            self.library.clone(),
            "delete PDF annotation",
            move |library| library.delete_annotation(&stored_id, modified_at).map_err(|error| error.to_string()),
            move |this, cx, ()| {
                this.marks.remove_annotation(&id);
                if this.marks.selected_id.is_none() {
                    this.annotation_popup_anchor = None;
                    this.pdf.update(cx, |pdf, cx| pdf.set_active_annotation(None, cx));
                }
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
        let now = crate::shell::state_panel::now_millis();
        let bookmark =
            ReaderBookmark { id: uuid::Uuid::new_v4().to_string(), content_hash: self.locator.content_hash(), cfi: pdf_page_cfi(self.current_page.saturating_sub(1)), label: self.bookmark_label(), created_at: now, modified_at: now };
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
        let deleted_at = crate::shell::state_panel::now_millis();
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
    fn render_sidebar_tabs(&self, show_annotations: bool, outline_available: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        crate::shell::state_panel::reader_sidebar_tabs("pdf-sidebar-tabs", &cx.entity(), show_annotations, outline_available, cx, |annotations, this, cx| {
            this.annotations_visible = annotations;
            this.set_toc_visible(!annotations, cx);
            crate::invalidation::notify(cx, Component::PdfShell, "sidebar_tab_changed");
        })
    }

    fn render_annotations(&self, outline_available: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let tabs_target = cx.entity();
        let panel = components::reader_state_panel(theme).child(self.render_sidebar_tabs(true, outline_available, cx)).child(crate::shell::state_panel::reader_state_tabs(
            "pdf-state-tabs",
            &tabs_target,
            self.showing_bookmarks,
            &self.marks,
            cx,
            |showing_bookmarks, this, cx| {
                this.showing_bookmarks = showing_bookmarks;
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            },
        ));

        let wording = crate::shell::state_panel::StatePanelWording {
            bookmark_tooltip: "Bookmark the current page",
            no_bookmarks: "No bookmarks yet. Use Bookmark here to save this page.",
            no_annotations: "Select PDF text, then use Highlight or Note.",
        };
        let actions = crate::shell::state_panel::StatePanelActions {
            add_bookmark: Rc::new(|this: &mut Self, cx| this.add_bookmark(cx)),
            go_to_bookmark: Rc::new(|bookmark: &ReaderBookmark, this: &mut Self, cx| this.navigate_to_bookmark(&bookmark.cfi, cx)),
            delete_bookmark: Rc::new(|id, this: &mut Self, cx| this.delete_bookmark(id, cx)),
            select_annotation: Rc::new(|id, window, this: &mut Self, cx| {
                this.annotation_popup_anchor = None;
                this.select_annotation(id, window, cx);
            }),
            go_to_annotation: Rc::new(|annotation: &ReaderAnnotation, this: &mut Self, cx| this.navigate_to_annotation(annotation, cx)),
            delete_annotation: Rc::new(|id, this: &mut Self, cx| this.delete_annotation(id, cx)),
        };
        // A PDF always has a current page, so there is always somewhere for a
        // bookmark to point.
        panel.children(crate::shell::state_panel::reader_state_rows(&tabs_target, &self.marks, self.showing_bookmarks, true, &wording, &actions, theme)).into_any_element()
    }

    fn render_annotation_popup(&self, annotation: ReaderAnnotation, geometry: annotation_editor::Geometry, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let entity = cx.entity();
        let delete_id = annotation.id.clone();
        // Saving a style or a colour leaves the editor open so several can be
        // tried; only closing and deleting dismiss it. Those two have to hand
        // the keyboard back, because the note input inside the editor may be
        // what holds it, and focus left on an element that no longer renders
        // puts every key binding out of reach.
        let save = |id: &'static str, label: &'static str, style: Option<AnnotationStyle>, color: Option<&'static str>| {
            let target = entity.clone();
            annotation_editor::Action::new(id, label, move |_, cx| {
                let style = style.clone();
                target.update(cx, |this, cx| this.save_selected_annotation(style, color, cx));
            })
        };
        let close_target = entity.clone();
        let delete_target = entity.clone();
        let rows = vec![
            vec![
                save("pdf-popup-save", "Save", None, None),
                annotation_editor::Action::new("pdf-popup-close", "Close", move |window, cx| {
                    close_target.update(cx, |this, cx| {
                        this.close_annotation_editor(cx);
                        this.focus_document(window, cx);
                    });
                }),
                annotation_editor::Action::new("pdf-popup-delete", "Delete", move |window, cx| {
                    let id = delete_id.clone();
                    delete_target.update(cx, |this, cx| {
                        this.delete_annotation(id, cx);
                        this.focus_document(window, cx);
                    });
                }),
            ],
            vec![
                save("pdf-popup-highlight", "Highlight", Some(AnnotationStyle::Highlight), None),
                save("pdf-popup-underline", "Underline", Some(AnnotationStyle::Underline), None),
                save("pdf-popup-squiggly", "Squiggly", Some(AnnotationStyle::Squiggly), None),
                save("pdf-popup-strike", "Strike", Some(AnnotationStyle::Strikethrough), None),
            ],
            vec![
                save("pdf-popup-yellow", "Yellow", None, Some("#f6c945")),
                save("pdf-popup-blue", "Blue", None, Some("#60a5fa")),
                save("pdf-popup-pink", "Pink", None, Some("#f472b6")),
                save("pdf-popup-green", "Green", None, Some("#4ade80")),
            ],
        ];
        annotation_editor::editor(geometry, &annotation.exact_text, &self.annotation_note_input, rows, theme)
    }

    fn render_toc(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let toc = crate::shell::state_panel::reader_toc_tree(&self.toc_tree, &entity, theme, |id, window, this: &mut Self, cx| {
            this.navigate_to_toc(id, cx);
            this.focus_document(window, cx);
        });
        components::reader_toc(theme).child(self.render_sidebar_tabs(false, true, cx)).child(toc).into_any_element()
    }

    fn render_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let zoom = self.pdf.read(cx).zoom();
        let zoom_mode = self.pdf.read(cx).zoom_mode();
        let visible_page_count = self.pdf.read(cx).visible_page_count();
        let trim_margins = ReaderSettings::preferences(cx).pdf_trim_margins;
        components::reader_settings_dropdown(theme)
            .child(
                components::reader_settings_section("Page layout", theme)
                    .child(components::reader_setting_row("Margins").child(components::reader_choice_button_group(
                        "pdf-margin-group",
                        [
                            components::reader_choice_button("pdf-margin-off", "Off", !trim_margins).on_click(cx.listener(|this, _, _, cx| this.set_trim_margins(false, cx))),
                            components::reader_choice_button("pdf-margin-auto", "Auto", trim_margins).on_click(cx.listener(|this, _, _, cx| this.set_trim_margins(true, cx))),
                        ],
                    )))
                    .child(
                        components::reader_setting_row("Fit").child(components::reader_choice_button_group(
                            "pdf-fit-group",
                            [
                                components::reader_choice_button("pdf-fit-width", "Fit width", matches!(zoom_mode, PdfZoomMode::FitWidth { .. }))
                                    .on_click(cx.listener(move |this, _, _, cx| this.set_zoom_mode(PdfZoomMode::FitWidth { columns: visible_page_count }, cx))),
                                components::reader_choice_button("pdf-fit-zoom", "Zoom", matches!(zoom_mode, PdfZoomMode::Fixed { .. })).on_click(cx.listener(move |this, _, _, cx| this.set_zoom_mode(PdfZoomMode::Fixed { zoom }, cx))),
                                components::reader_choice_button("pdf-fit-height", "Fit height", zoom_mode == PdfZoomMode::FitHeight).on_click(cx.listener(|this, _, _, cx| this.set_zoom_mode(PdfZoomMode::FitHeight, cx))),
                            ],
                        )),
                    )
                    // Only the value the mode holds fixed is adjustable; the
                    // other one is derived from it, and fit height derives
                    // both.
                    .children(matches!(zoom_mode, PdfZoomMode::FitWidth { .. }).then(|| {
                        components::reader_setting_row("Pages").child(components::reader_stepper_group(
                            components::reader_stepper_button("pdf-pages-decrease", "−", theme).on_click(cx.listener(|this, _, _, cx| this.adjust_visible_page_count(-1, cx))),
                            visible_page_count.to_string(),
                            components::reader_stepper_button("pdf-pages-increase", "+", theme).on_click(cx.listener(|this, _, _, cx| this.adjust_visible_page_count(1, cx))),
                            theme,
                        ))
                    }))
                    .children(matches!(zoom_mode, PdfZoomMode::Fixed { .. }).then(|| {
                        components::reader_setting_row("Zoom").child(components::reader_stepper_group(
                            components::reader_stepper_button("pdf-zoom-out", "−", theme).tooltip("Zoom out (Ctrl+-)").on_click(cx.listener(|this, _, _, cx| this.adjust_zoom(-ZOOM_STEP, cx))),
                            format!("{:.0}%", zoom * 100.0),
                            components::reader_stepper_button("pdf-zoom-in", "+", theme).tooltip("Zoom in (Ctrl++)").on_click(cx.listener(|this, _, _, cx| this.adjust_zoom(ZOOM_STEP, cx))),
                            theme,
                        ))
                    })),
            )
            .into_any_element()
    }

    fn on_previous_page(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        self.hide_chrome(cx);
        self.previous_page(cx);
    }

    fn on_next_page(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        self.hide_chrome(cx);
        self.next_page(cx);
    }

    fn on_previous_line(&mut self, _: &PreviousLine, _: &mut Window, cx: &mut Context<Self>) {
        self.hide_chrome(cx);
        self.pdf.update(cx, |pdf, cx| pdf.previous_item(cx));
    }

    fn on_next_line(&mut self, _: &NextLine, _: &mut Window, cx: &mut Context<Self>) {
        self.hide_chrome(cx);
        self.pdf.update(cx, |pdf, cx| pdf.next_item(cx));
    }

    fn hide_chrome(&mut self, cx: &mut Context<Self>) {
        if self.chrome_hidden {
            return;
        }
        self.chrome_hidden = true;
        crate::invalidation::notify(cx, Component::PdfShell, "chrome_hidden");
    }

    fn show_chrome(&mut self, cx: &mut Context<Self>) {
        if !self.chrome_hidden {
            return;
        }
        self.chrome_hidden = false;
        crate::invalidation::notify(cx, Component::PdfShell, "chrome_shown");
    }

    fn on_toggle_toc(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.show_chrome(cx);
        self.set_toc_visible(!self.toc_visible, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn set_toc_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.toc_visible = visible;
        ReaderSettings::update(cx, |preferences| preferences.toc_visible = visible);
    }

    fn on_focus_search(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.show_chrome(cx);
        if self.search_active {
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        self.search_active = true;
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
    }

    fn on_toggle_search(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.show_chrome(cx);
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
        self.adjust_zoom(ZOOM_STEP, cx);
    }

    fn on_decrease_scale(&mut self, _: &DecreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_zoom(-ZOOM_STEP, cx);
    }
}

impl Focusable for PdfReaderView {
    /// Focusing the reader means focusing the document.
    ///
    /// Navigation keys are bound in the `PdfReaderDocument` context, which only
    /// the page elements establish, so anything that focuses this view has to
    /// land on the document or the keyboard goes dead. Before a page has been
    /// laid out there is nothing to focus, so the shell stands in.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.page_count > 0 { self.pdf.read(cx).focus_handle() } else { self.focus.clone() }
    }
}

impl Render for PdfReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::invalidation::record_render(Component::PdfShell);
        let theme = components::browser_theme(cx);
        window.set_window_title(&self.title);
        let entity = cx.entity();
        let pdf_document_focus = self.pdf.read(cx).focus_handle();
        let outline_available = self.pdf.read(cx).info().is_some_and(|info| !info.outline().is_empty());
        let sidebar_visible = self.annotations_visible || (self.toc_visible && outline_available);
        let toc_width = chrome::sidebar_width(sidebar_visible, cx);
        let available_width = (f32::from(window.viewport_size().width) - toc_width).max(0.0);
        self.pdf.update(cx, |pdf, cx| {
            pdf.set_available_width(available_width, cx);
        });
        let (visible_page_range, selection_available, visible_column_count) = {
            let pdf = self.pdf.read(cx);
            (pdf.visible_page_range(), pdf.selection_snapshot().is_some(), pdf.visible_page_count())
        };
        let page_indicator = format_page_indicator(visible_page_range, self.page_count);
        let progress = pdf_progress(visible_page_range, self.page_count);
        let section_title = self.active_outline_title().unwrap_or(&self.title).to_owned();

        let mut body = components::reader_document_body().child(components::reader_content().child(self.pdf.clone()));
        if let Some(annotation) = self.marks.selected_id.as_ref().and_then(|id| self.marks.annotation(id)).cloned() {
            let viewport = window.viewport_size();
            let geometry = annotation_editor::geometry(self.annotation_popup_anchor, f32::from(viewport.width), f32::from(viewport.height), toc_width, PDF_READER_TOOLBAR_HEIGHT, visible_column_count);
            body = body.child(self.render_annotation_popup(annotation, geometry, cx));
        }
        if self.search_active {
            body = body.child(
                components::reader_search_bar(theme)
                    .child(components::reader_search_box(theme).key_context("ReaderSearch").child(Input::new(&self.search_input).appearance(false).size_full()))
                    .child(components::reader_match_count(format!("{}/{}", self.search_match.0, self.search_match.1), theme))
                    .child(components::reader_square_button("pdf-search-previous", "‹", theme).on_click(cx.listener(|this, _, _, cx| this.navigate_search(-1, cx))))
                    .child(components::reader_square_button("pdf-search-next", "›", theme).on_click(cx.listener(|this, _, _, cx| this.navigate_search(1, cx))))
                    .child(components::reader_square_button("pdf-search-close", "×", theme).on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))),
            );
        }
        let content = if self.annotations_visible {
            chrome::with_sidebar(self.render_annotations(outline_available, cx), body.into_any_element(), cx)
        } else if self.toc_visible && outline_available {
            chrome::with_sidebar(self.render_toc(entity.clone(), theme, cx), body.into_any_element(), cx)
        } else {
            body.into_any_element()
        };
        let settings = self.render_settings(cx);
        let left = gpui::div().min_w_0().flex_1().flex().items_center().gap(gpui::px(components::SPACE_SM)).child(
            components::reader_control_button_icon("pdf-sidebar", "Toggle contents and annotations", gpui_component::IconName::PanelLeftOpen, theme).on_click(cx.listener(move |this, _, _, cx| {
                if this.annotations_visible || (this.toc_visible && outline_available) {
                    this.annotations_visible = false;
                    this.set_toc_visible(false, cx);
                } else if outline_available {
                    this.annotations_visible = false;
                    this.set_toc_visible(true, cx);
                } else {
                    this.annotations_visible = true;
                }
                crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
            })),
        );
        let right = gpui::div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap(gpui::px(components::SPACE_SM))
            .children(selection_available.then(|| {
                components::reader_control_button("pdf-highlight-selection", "Highlight", theme).tooltip("Save selected PDF text as a highlight").on_click(cx.listener(|this, _, window, cx| this.create_annotation(false, window, cx)))
            }))
            .children(selection_available.then(|| {
                components::reader_control_button("pdf-note-selection", "Note", theme).tooltip("Highlight selected PDF text and open annotations").on_click(cx.listener(|this, _, window, cx| this.create_annotation(true, window, cx)))
            }))
            .child(components::reader_control_button_icon("pdf-search", "Search", gpui_component::IconName::Search, theme).on_click(cx.listener(|this, _, window, cx| {
                if !this.search_active {
                    this.search_active = true;
                    crate::invalidation::notify(cx, Component::PdfShell, "search_opened");
                }
                this.search_input.read(cx).focus_handle(cx).focus(window, cx);
            })))
            .child(components::reader_progress(progress, page_indicator, theme))
            // The PDF view element is what establishes `PdfReaderDocument`, so
            // its handle — not this page's — is the one the page keys need.
            .child(components::reader_settings_popover(Some(&pdf_document_focus), theme).child(settings));
        let toolbar = components::reader_toolbar(theme).child(left).child(components::reader_toolbar_title(section_title)).child(right);

        // The page container is only focusable until the document has pages of
        // its own to hold the keyboard. A focusable container steals focus from
        // the document on any click that reaches it — the toolbar's background,
        // the sidebar, the margin around a page — and navigation is bound in
        // the document's key context, so the paging keys would go dead until
        // the page was clicked again. The container still sits on the dispatch
        // path either way, so `Reader` bindings keep firing.
        let document_ready = self.page_count > 0;
        let page = components::reader_app_page(window, cx, theme)
            .key_context("Reader")
            .when(!document_ready, |page| page.track_focus(&self.focus))
            .on_action(cx.listener(Self::on_previous_page))
            .on_action(cx.listener(Self::on_next_page))
            .on_action(cx.listener(Self::on_previous_line))
            .on_action(cx.listener(Self::on_next_line))
            .on_action(cx.listener(Self::on_toggle_toc))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_toggle_search))
            .on_action(cx.listener(Self::on_close_search))
            .on_action(cx.listener(Self::on_increase_scale))
            .on_action(cx.listener(Self::on_decrease_scale))
            .children((!self.chrome_hidden).then(|| toolbar.into_any_element()))
            .child(components::reader_content().child(content));
        let kobo_tap_zone_top = px(if self.chrome_hidden { 0.0 } else { PDF_READER_TOOLBAR_HEIGHT });
        let page = page
            .children((cfg!(feature = "kobo") && !sidebar_visible).then(|| {
                gpui::div()
                    .id("kobo-pdf-previous-page-tap-zone")
                    .absolute()
                    .left_0()
                    .top(kobo_tap_zone_top)
                    .bottom_0()
                    .w(gpui::relative(0.25))
                    .on_click(cx.listener(|reader: &mut Self, _, _, cx| {
                        reader.hide_chrome(cx);
                        reader.previous_page(cx);
                    }))
                    .into_any_element()
            }))
            .children((cfg!(feature = "kobo") && !sidebar_visible).then(|| {
                gpui::div()
                    .id("kobo-pdf-next-page-tap-zone")
                    .absolute()
                    .right_0()
                    .top(kobo_tap_zone_top)
                    .bottom_0()
                    .w(gpui::relative(0.25))
                    .on_click(cx.listener(|reader: &mut Self, _, _, cx| {
                        reader.hide_chrome(cx);
                        reader.next_page(cx);
                    }))
                    .into_any_element()
            }))
            .children((cfg!(feature = "kobo") && !sidebar_visible).then(|| {
                gpui::div()
                    .id("kobo-pdf-toggle-toolbar-tap-zone")
                    .absolute()
                    .left(gpui::relative(0.25))
                    .right(gpui::relative(0.25))
                    .top(kobo_tap_zone_top)
                    .bottom_0()
                    .on_click(cx.listener(|reader: &mut Self, _, _, cx| {
                        if reader.chrome_hidden {
                            reader.show_chrome(cx);
                        } else {
                            reader.hide_chrome(cx);
                        }
                    }))
                    .into_any_element()
            }));
        page.children(
            (self.chrome_hidden && !cfg!(feature = "kobo"))
                .then(|| components::reader_chrome_reveal().on_mouse_move(cx.listener(|reader: &mut Self, _, _, cx| reader.show_chrome(cx))).on_click(cx.listener(|reader: &mut Self, _, _, cx| reader.show_chrome(cx))).into_any_element()),
        )
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

/// Encodes a zero-based page as a PDF bookmark locator.
///
/// `ReaderBookmark::cfi` is a durable, synchronized field, so paginated
/// PDF bookmarks do not use EPUB CFIs and instead store the page index directly.
fn pdf_page_cfi(page_index: usize) -> String {
    page_index.to_string()
}

/// Reads a page index back out of [`pdf_page_cfi`], returning `None` for anything
/// that is not a plain integer.
fn pdf_page_from_cfi(value: &str) -> Option<usize> {
    if !value.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    value.parse::<usize>().ok()
}

/// Rebuilds the layout mode from what was persisted.
fn stored_zoom_mode(preferences: &ReaderPreferences) -> PdfZoomMode {
    match preferences.pdf_zoom_mode {
        PdfZoomModePreference::FitWidth => PdfZoomMode::FitWidth { columns: usize::from(preferences.pdf_page_count).clamp(MIN_VISIBLE_PAGE_COUNT, MAX_VISIBLE_PAGE_COUNT) },
        PdfZoomModePreference::Fixed => PdfZoomMode::Fixed { zoom: preferences.pdf_zoom },
        PdfZoomModePreference::FitHeight => PdfZoomMode::FitHeight,
    }
}

/// Records a layout mode, keeping the page count and zoom that the other modes
/// will be restored with.
fn store_zoom_mode(zoom_mode: PdfZoomMode, preferences: &mut ReaderPreferences) {
    match zoom_mode {
        PdfZoomMode::FitWidth { columns } => {
            preferences.pdf_zoom_mode = PdfZoomModePreference::FitWidth;
            preferences.pdf_page_count = columns as u8;
        }
        PdfZoomMode::Fixed { zoom } => {
            preferences.pdf_zoom_mode = PdfZoomModePreference::Fixed;
            preferences.pdf_zoom = zoom;
        }
        PdfZoomMode::FitHeight => preferences.pdf_zoom_mode = PdfZoomModePreference::FitHeight,
    }
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

fn pdf_progress(visible_page_range: Option<(usize, usize)>, page_count: usize) -> String {
    let Some((_, last)) = visible_page_range else {
        return "0%".to_owned();
    };
    if page_count == 0 {
        return "0%".to_owned();
    }
    format!("{:.0}%", (last.saturating_add(1) as f32 / page_count as f32 * 100.0).clamp(0.0, 100.0))
}

pub(crate) fn bind_pdf_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousPage, Some("PdfReaderDocument")),
        KeyBinding::new("right", NextPage, Some("PdfReaderDocument")),
        KeyBinding::new("up", PreviousLine, Some("PdfReaderDocument")),
        KeyBinding::new("down", NextLine, Some("PdfReaderDocument")),
        KeyBinding::new("pageup", PreviousPage, Some("PdfReaderDocument")),
        KeyBinding::new("pagedown", NextPage, Some("PdfReaderDocument")),
        KeyBinding::new("ctrl-=", IncreaseScale, Some("PdfReaderDocument")),
        KeyBinding::new("ctrl--", DecreaseScale, Some("PdfReaderDocument")),
    ]);
}

#[cfg(test)]
mod tests {
    use super::{annotation_color, bookmark_label, format_page_indicator, pdf_page_cfi, pdf_page_from_cfi, pdf_progress};
    use gpui::{point, px};
    use pdf_view_gpui::PdfAnnotationStyle;

    #[test]
    fn page_indicator_reports_single_pages_and_spreads() {
        assert_eq!(format_page_indicator(None, 0), "0/0");
        assert_eq!(format_page_indicator(Some((0, 0)), 12), "1/12");
        assert_eq!(format_page_indicator(Some((1, 2)), 12), "2–3/12");
    }

    #[test]
    fn pdf_progress_uses_the_last_visible_page() {
        assert_eq!(pdf_progress(None, 12), "0%");
        assert_eq!(pdf_progress(Some((0, 0)), 12), "8%");
        assert_eq!(pdf_progress(Some((10, 11)), 12), "100%");
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
    fn page_locators_are_integer_page_indices() {
        assert_eq!(pdf_page_cfi(0), "0");
        assert_eq!(pdf_page_cfi(4), "4");
    }

    #[test]
    fn a_real_epub_locator_is_not_mistaken_for_a_page() {
        assert_eq!(pdf_page_from_cfi("not a locator"), None);
        assert_eq!(pdf_page_from_cfi("epubcfi(/6/10)"), None, "PDF locators are integer page indices");
        assert_eq!(pdf_page_from_cfi("0a"), None, "suffixes are not valid");
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
