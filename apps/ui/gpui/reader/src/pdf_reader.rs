use std::time::{SystemTime, UNIX_EPOCH};

use epub_svelte_lib::library::marks::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation, ReaderBookmark};
use epub_svelte_lib::library::ResolvedBook;
use epub_svelte_lib::{BookLocator, LibraryBackend};
use gpui::prelude::*;
use gpui::{px, App, Context, Entity, FocusHandle, Focusable, KeyBinding, Pixels, Point, Render, ScrollStrategy, Subscription, Window};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::tab::TabBar;
use gpui_component::tree::{tree, TreeItem, TreeState};
use gpui_component::Sizable;
use pdf_reader_core::{NormalizedRect, PdfOutlineEntry};
use pdf_view_gpui::{PdfAnnotationOverlay, PdfAnnotationStyle, PdfView, PdfViewEvent, PdfZoomMode, MAX_VISIBLE_PAGE_COUNT, MIN_VISIBLE_PAGE_COUNT};
use ui_components as components;

use crate::epub::marks::MarksState;
use crate::epub::ReaderWindowId;
use crate::invalidation::Component;
use crate::pdf::toc::{nest_pdf_outline, pdf_toc_tree_items, PdfTocState};
use crate::settings::{PdfZoomModePreference, ReaderPreferences, ReaderSettings};
use crate::shell::chrome;
use crate::shell::persistence::{self, PositionWriter};
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, PreviousLine, PreviousPage, ToggleSearch, ToggleToc};

const PDF_READER_TOOLBAR_HEIGHT: f32 = 58.0;
/// One zoom step, in pixels per PDF point.
const ZOOM_STEP: f32 = 0.1;
const PDF_ANNOTATION_POPUP_WIDTH: f32 = 360.0;
const PDF_ANNOTATION_POPUP_MAX_HEIGHT: f32 = 420.0;
const PDF_ANNOTATION_POPUP_INSET: f32 = 12.0;
const PDF_ANNOTATION_POPUP_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct PdfAnnotationPopupGeometry {
    left: f32,
    edge_offset: f32,
    max_height: f32,
    above: bool,
}

fn pdf_annotation_popup_geometry(anchor: Option<Point<Pixels>>, viewport_width: f32, viewport_height: f32, sidebar_width: f32, column_count: usize) -> PdfAnnotationPopupGeometry {
    let document_width = (viewport_width - sidebar_width).max(1.0);
    let document_height = (viewport_height - PDF_READER_TOOLBAR_HEIGHT).max(1.0);
    let column_count = column_count.max(1) as f32;
    let column_width = document_width / column_count;
    let (anchor_x, anchor_y) = anchor.map(|point| (f32::from(point.x) - sidebar_width, f32::from(point.y) - PDF_READER_TOOLBAR_HEIGHT)).unwrap_or((column_width / 2.0, document_height / 3.0));
    let anchor_x = anchor_x.clamp(0.0, document_width - f32::EPSILON);
    let anchor_y = anchor_y.clamp(0.0, document_height);
    let column = (anchor_x / column_width).floor().min(column_count - 1.0);
    let column_center = (column + 0.5) * column_width;
    let panel_width = PDF_ANNOTATION_POPUP_WIDTH.min((document_width - PDF_ANNOTATION_POPUP_INSET * 2.0).max(1.0));
    let maximum_left = (document_width - panel_width).max(0.0);
    let side_inset = PDF_ANNOTATION_POPUP_INSET.min(maximum_left / 2.0);
    let left = (column_center - panel_width / 2.0).clamp(side_inset, (maximum_left - side_inset).max(side_inset));
    let room_above = (anchor_y - PDF_ANNOTATION_POPUP_GAP - PDF_ANNOTATION_POPUP_INSET).max(1.0);
    let room_below = (document_height - anchor_y - PDF_ANNOTATION_POPUP_GAP - PDF_ANNOTATION_POPUP_INSET).max(1.0);
    let above = room_above > room_below;
    PdfAnnotationPopupGeometry {
        left,
        edge_offset: if above { document_height - anchor_y + PDF_ANNOTATION_POPUP_GAP } else { anchor_y + PDF_ANNOTATION_POPUP_GAP },
        max_height: (if above { room_above } else { room_below }).min(PDF_ANNOTATION_POPUP_MAX_HEIGHT),
        above,
    }
}

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
    positions: PositionWriter<usize>,
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
            pdf
        });
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let toc_tree = cx.new(|cx| TreeState::new(cx));
        let annotation_note_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add a note"));
        let pdf_subscription = cx.subscribe_in(&pdf, window, |this, pdf, event: &PdfViewEvent, window, cx| match event {
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
        self.focus.focus(window, cx);
        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
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

    fn now_millis() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as i64).unwrap_or(0)
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
        let now = Self::now_millis();
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
        let now = Self::now_millis();
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
    fn render_sidebar_tabs(&self, show_annotations: bool, outline_available: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let target = cx.entity();
        let mut tabs = TabBar::new("pdf-sidebar-tabs").segmented().small().selected_index(if outline_available { usize::from(show_annotations) } else { 0 });
        if outline_available {
            tabs = tabs.child("Contents");
        }
        tabs.child("Annotations")
            .on_click(move |index, _, cx| {
                target.update(cx, |this, cx| {
                    let annotations = !outline_available || *index == 1;
                    this.annotations_visible = annotations;
                    this.set_toc_visible(!annotations, cx);
                    crate::invalidation::notify(cx, Component::PdfShell, "sidebar_tab_changed");
                });
            })
            .into_any_element()
    }

    fn render_annotations(&self, outline_available: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let tabs_target = cx.entity();
        let mut panel = components::reader_state_panel(theme).child(self.render_sidebar_tabs(true, outline_available, cx)).child(
            TabBar::new("pdf-state-tabs")
                .segmented()
                .small()
                .selected_index(usize::from(self.showing_bookmarks))
                .child(format!("Highlights ({})", self.marks.annotations.len()))
                .child(format!("Bookmarks ({})", self.marks.bookmarks.len()))
                .on_click(move |index, _, cx| {
                    tabs_target.update(cx, |this, cx| {
                        this.showing_bookmarks = *index == 1;
                        crate::invalidation::notify(cx, Component::PdfShell, "state_changed");
                    });
                }),
        );

        if self.showing_bookmarks {
            panel = panel.child(
                components::reader_sidebar_action_button("pdf-bookmark-current", "Bookmark here", gpui_component::IconName::Star, true, theme)
                    .tooltip("Bookmark the current page")
                    .on_click(cx.listener(|this, _, _, cx| this.add_bookmark(cx))),
            );
            if self.marks.bookmarks.is_empty() {
                panel = panel.child(components::reader_state_note("No bookmarks yet. Use Bookmark here to save this page.", theme));
            }
            for (index, bookmark) in self.marks.bookmarks.iter().cloned().enumerate() {
                let cfi = bookmark.cfi.clone();
                let id = bookmark.id.clone();
                panel = panel.child(
                    components::reader_state_card(("pdf-bookmark", index), false, theme).child(components::reader_state_quote(bookmark.label)).child(
                        components::reader_state_actions()
                            .child(
                                components::reader_sidebar_action_button(("pdf-bookmark-go", index), "Go", gpui_component::IconName::ArrowRight, true, theme).on_click(cx.listener(move |this, _, _, cx| this.navigate_to_bookmark(&cfi, cx))),
                            )
                            .child(
                                components::reader_sidebar_action_button(("pdf-bookmark-delete", index), "Delete", gpui_component::IconName::Delete, true, theme)
                                    .on_click(cx.listener(move |this, _, _, cx| this.delete_bookmark(id.clone(), cx))),
                            ),
                    ),
                );
            }
            return panel.into_any_element();
        }

        if self.marks.annotations.is_empty() {
            panel = panel.child(components::reader_state_note("Select PDF text, then use Highlight or Note.", theme));
        }
        for (index, annotation) in self.marks.annotations.iter().cloned().enumerate() {
            let selected = self.marks.selected_id.as_deref() == Some(annotation.id.as_str());
            let select_id = annotation.id.clone();
            let delete_id = annotation.id.clone();
            let navigate_annotation = annotation.clone();
            let note = annotation.note.clone();
            let card = components::reader_state_card(("pdf-annotation", index), selected, theme)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.annotation_popup_anchor = None;
                    this.select_annotation(select_id.clone(), window, cx);
                }))
                .child(components::reader_state_quote(format!("“{}”", annotation.exact_text)))
                .when(!note.is_empty(), |card| card.child(components::reader_state_note(note, theme)))
                .child(
                    components::reader_state_actions()
                        .child(
                            components::reader_sidebar_action_button(("pdf-annotation-go", index), "Go", gpui_component::IconName::ArrowRight, true, theme)
                                .on_click(cx.listener(move |this, _, _, cx| this.navigate_to_annotation(&navigate_annotation, cx))),
                        )
                        .child(
                            components::reader_sidebar_action_button(("pdf-annotation-delete", index), "Delete", gpui_component::IconName::Delete, true, theme)
                                .on_click(cx.listener(move |this, _, _, cx| this.delete_annotation(delete_id.clone(), cx))),
                        ),
                );
            panel = panel.child(card);
        }
        panel.into_any_element()
    }

    fn render_annotation_popup(&self, annotation: ReaderAnnotation, geometry: PdfAnnotationPopupGeometry, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let delete_id = annotation.id.clone();
        components::reader_annotation_editor(theme)
            .absolute()
            .left(px(geometry.left))
            .max_h(px(geometry.max_height))
            .when(geometry.above, |panel| panel.bottom(px(geometry.edge_offset)))
            .when(!geometry.above, |panel| panel.top(px(geometry.edge_offset)))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(components::reader_state_quote(format!("“{}”", annotation.exact_text)))
            .child(components::reader_annotation_note_editor(theme).child(Input::new(&self.annotation_note_input).appearance(false).size_full()))
            .child(
                components::reader_state_actions()
                    .child(components::reader_control_button("pdf-popup-save", "Save", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, None, cx))))
                    .child(components::reader_control_button("pdf-popup-close", "Close", theme).on_click(cx.listener(|this, _, _, cx| this.close_annotation_editor(cx))))
                    .child(components::reader_control_button("pdf-popup-delete", "Delete", theme).on_click(cx.listener(move |this, _, _, cx| this.delete_annotation(delete_id.clone(), cx)))),
            )
            .child(
                components::reader_state_actions()
                    .child(components::reader_control_button("pdf-popup-highlight", "Highlight", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(Some(AnnotationStyle::Highlight), None, cx))))
                    .child(components::reader_control_button("pdf-popup-underline", "Underline", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(Some(AnnotationStyle::Underline), None, cx))))
                    .child(components::reader_control_button("pdf-popup-squiggly", "Squiggly", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(Some(AnnotationStyle::Squiggly), None, cx))))
                    .child(components::reader_control_button("pdf-popup-strike", "Strike", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(Some(AnnotationStyle::Strikethrough), None, cx)))),
            )
            .child(
                components::reader_state_actions()
                    .child(components::reader_control_button("pdf-popup-yellow", "Yellow", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, Some("#f6c945"), cx))))
                    .child(components::reader_control_button("pdf-popup-blue", "Blue", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, Some("#60a5fa"), cx))))
                    .child(components::reader_control_button("pdf-popup-pink", "Pink", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, Some("#f472b6"), cx))))
                    .child(components::reader_control_button("pdf-popup-green", "Green", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, Some("#4ade80"), cx)))),
            )
            .into_any_element()
    }

    fn render_toc(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let toc = tree(&self.toc_tree, move |index, entry, _selected, _window, _cx| {
            let item = entry.item();
            let id = item.id.to_string();
            let navigation_target = entity.clone();
            let title = components::reader_toc_title(("pdf-toc-tree-title", index), item.label.clone(), theme).on_click(move |_, _, cx| {
                navigation_target.update(cx, |this, cx| this.navigate_to_toc(&id, cx));
            });
            components::reader_toc_tree_item(("pdf-toc-tree-item", index), ("pdf-toc-tree-disclosure", index), entry.depth(), entry.is_folder(), entry.is_expanded(), title, theme)
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
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
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
            let geometry = pdf_annotation_popup_geometry(self.annotation_popup_anchor, f32::from(viewport.width), f32::from(viewport.height), toc_width, visible_column_count);
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

        components::app_page("IBM Plex Sans", 16, theme)
            .key_context("Reader")
            .track_focus(&self.focus)
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
            .children(self.chrome_hidden.then(|| components::reader_chrome_reveal().on_mouse_move(cx.listener(|reader: &mut Self, _, _, cx| reader.show_chrome(cx))).into_any_element()))
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
    if first == last {
        format!("{}/{page_count}", first + 1)
    } else {
        format!("{}–{}/{page_count}", first + 1, last + 1)
    }
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
    use super::{annotation_color, bookmark_label, format_page_indicator, pdf_annotation_popup_geometry, pdf_page_cfi, pdf_page_from_cfi, pdf_progress};
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
    fn annotation_popup_tracks_the_clicked_pdf_column_and_available_height() {
        let left = pdf_annotation_popup_geometry(Some(point(px(120.0), px(180.0))), 1200.0, 800.0, 0.0, 3);
        let right = pdf_annotation_popup_geometry(Some(point(px(1020.0), px(700.0))), 1200.0, 800.0, 0.0, 3);
        assert!(left.left < right.left);
        assert!(!left.above);
        assert!(right.above);
        assert!(left.max_height <= 420.0 && right.max_height <= 420.0);
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
