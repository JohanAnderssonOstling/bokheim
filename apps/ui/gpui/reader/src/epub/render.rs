use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::settings::ReaderSettings;
use crate::shell::chrome;
use crate::{
    CloseSearch, DecreaseColumnWidth, DecreaseFont, DecreaseScale, FocusReaderSearch, IncreaseColumnWidth, IncreaseFont, IncreaseScale, NavigateHistoryBack, NavigateHistoryForward, NextLine, NextPage, NextSection, PreviousLine,
    PreviousPage, PreviousSection, ToggleFullscreen, ToggleSearch, ToggleToc,
};
use app_contract::{READER_NAVIGATION_SHORTCUTS, ReaderNavigationAction, ReaderNavigationKey, ReaderNavigationShortcut};
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, Pixels, Point, Render, Window, px};
use gpui_component::Sizable;
use gpui_component::input::Input;
use gpui_component::menu::ContextMenuExt;
use gpui_component::tab::TabBar;
use gpui_component::tree::tree;
use ui_components as components;

const READER_TOOLBAR_HEIGHT: f32 = 58.0;
/// Keeps glyph ink and logical-to-framebuffer rounding inside the Kobo panel.
/// This is a renderer guard band, not an authored EPUB margin.
const KOBO_READER_EDGE_INSET: f32 = 2.0;
const ANNOTATION_POPUP_WIDTH: f32 = 360.0;
const ANNOTATION_POPUP_MAX_HEIGHT: f32 = 420.0;
const ANNOTATION_POPUP_INSET: f32 = 12.0;
const ANNOTATION_POPUP_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct AnnotationPopupGeometry {
    left: f32,
    edge_offset: f32,
    max_height: f32,
    above: bool,
}

fn annotation_popup_geometry(anchor: Option<Point<Pixels>>, viewport_width: f32, viewport_height: f32, sidebar_width: f32, requested_column_width: f32, scale: f32, maximum_columns: u8) -> AnnotationPopupGeometry {
    let document_width = (viewport_width - sidebar_width).max(1.0);
    let document_height = (viewport_height - READER_TOOLBAR_HEIGHT).max(1.0);
    let scale = scale.max(0.01);
    let minimum_gap = 20.0 * scale;
    let requested_column_width = requested_column_width.max(1.0) * scale;
    let derived_columns = ((document_width + minimum_gap) / (requested_column_width + minimum_gap)).floor().max(1.0) as u8;
    let column_count = derived_columns.min(maximum_columns.max(1));
    let column_width = document_width / f32::from(column_count);

    let (anchor_x, anchor_y) = anchor.map(|point| (f32::from(point.x) - sidebar_width, f32::from(point.y) - READER_TOOLBAR_HEIGHT)).unwrap_or((column_width / 2.0, document_height / 3.0));
    let anchor_x = anchor_x.clamp(0.0, document_width - f32::EPSILON);
    let anchor_y = anchor_y.clamp(0.0, document_height);
    let column = (anchor_x / column_width).floor().min(f32::from(column_count - 1));
    let column_center = (column + 0.5) * column_width;
    let panel_width = ANNOTATION_POPUP_WIDTH.min((document_width - ANNOTATION_POPUP_INSET * 2.0).max(1.0));
    let maximum_left = (document_width - panel_width).max(0.0);
    let side_inset = ANNOTATION_POPUP_INSET.min(maximum_left / 2.0);
    let left = (column_center - panel_width / 2.0).clamp(side_inset, maximum_left - side_inset);

    let room_above = (anchor_y - ANNOTATION_POPUP_GAP - ANNOTATION_POPUP_INSET).max(1.0);
    let room_below = (document_height - anchor_y - ANNOTATION_POPUP_GAP - ANNOTATION_POPUP_INSET).max(1.0);
    let above = room_above > room_below;
    AnnotationPopupGeometry {
        left,
        edge_offset: if above { document_height - anchor_y + ANNOTATION_POPUP_GAP } else { anchor_y + ANNOTATION_POPUP_GAP },
        max_height: (if above { room_above } else { room_below }).min(ANNOTATION_POPUP_MAX_HEIGHT),
        above,
    }
}

impl ReaderView {
    fn render_sidebar_tabs(&self, show_annotations: bool, cx: &mut Context<Self>) -> AnyElement {
        let target = cx.entity();
        TabBar::new("reader-sidebar-tabs")
            .segmented()
            .small()
            .selected_index(usize::from(show_annotations))
            .child("Contents")
            .child("Annotations")
            .on_click(move |index, _, cx| {
                target.update(cx, |this, cx| {
                    let annotations = *index == 1;
                    this.reader_state_visible = annotations;
                    this.set_toc_visible(!annotations, cx);
                    crate::invalidation::notify(cx, Component::ReaderShell, "sidebar_tab_changed");
                });
            })
            .into_any_element()
    }

    fn render_toc(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let toc = tree(&self.toc_tree, move |index, entry, _selected, _window, _cx| {
            let item = entry.item();
            let link = item.id.to_string();
            let navigation_target = entity.clone();
            let title = components::reader_toc_title(("toc-tree-title", index), item.label.clone(), theme).on_click(move |_, window, cx| {
                navigation_target.update(cx, |this, cx| {
                    this.navigate_to(link.clone(), cx);
                    // The tree binds the arrow keys for its own selection, so
                    // it holds focus while it is being used. Following an entry
                    // means "take me there to read", so hand the keyboard back
                    // to the document rather than leaving paging keys dead.
                    this.focus_document(window, cx);
                    #[cfg(feature = "kobo")]
                    {
                        this.toc_visible = false;
                        crate::invalidation::notify(cx, Component::ReaderShell, "toc_navigated");
                    }
                });
            });
            components::reader_toc_tree_item(("toc-tree-item", index), ("toc-tree-disclosure", index), entry.depth(), entry.is_folder(), entry.is_expanded(), title, theme)
        });
        components::reader_toc(theme).child(self.render_sidebar_tabs(false, cx)).child(toc).into_any_element()
    }

    fn render_reader_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let tabs_target = cx.entity();
        let mut panel = components::reader_state_panel(theme).child(self.render_sidebar_tabs(true, cx)).child(
            TabBar::new("reader-state-tabs")
                .segmented()
                .small()
                .selected_index(usize::from(self.reader_state_bookmarks))
                .child(format!("Highlights ({})", self.marks.annotations.len()))
                .child(format!("Bookmarks ({})", self.marks.bookmarks.len()))
                .on_click(move |index, _, cx| {
                    tabs_target.update(cx, |this, cx| {
                        this.reader_state_bookmarks = *index == 1;
                        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
                    });
                }),
        );

        if self.reader_state_bookmarks {
            panel = panel.child(
                components::reader_sidebar_action_button("reader-bookmark-current", "Bookmark here", gpui_component::IconName::Star, self.current_cfi.is_some())
                    .tooltip("Bookmark the current location")
                    .on_click(cx.listener(|this, _, _, cx| this.add_bookmark(cx))),
            );
            if self.marks.bookmarks.is_empty() {
                panel = panel.child(components::reader_state_note("No bookmarks yet. Use Bookmark here to save this location.", theme));
            }
            for (index, bookmark) in self.marks.bookmarks.iter().cloned().enumerate() {
                let cfi = bookmark.cfi.clone();
                let id = bookmark.id.clone();
                panel = panel.child(
                    components::reader_state_card(("reader-bookmark", index), false, theme).child(components::reader_state_quote(bookmark.label)).child(
                        components::reader_state_actions()
                            .child(components::reader_sidebar_action_button(("reader-bookmark-go", index), "Go", gpui_component::IconName::ArrowRight, true).on_click(cx.listener(move |this, _, _, cx| this.navigate_to_cfi(cfi.clone(), cx))))
                            .child(
                                components::reader_sidebar_action_button(("reader-bookmark-delete", index), "Delete", gpui_component::IconName::Delete, true).on_click(cx.listener(move |this, _, _, cx| this.delete_bookmark(id.clone(), cx))),
                            ),
                    ),
                );
            }
        } else {
            if self.marks.annotations.is_empty() {
                panel = panel.child(components::reader_state_note("Select text, then use Highlight to create an annotation.", theme));
            }
            for (index, annotation) in self.marks.annotations.iter().cloned().enumerate() {
                let selected = self.marks.selected_id.as_deref() == Some(annotation.id.as_str());
                let cfi = annotation.anchor.epub_cfi_value().unwrap_or_default().to_owned();
                let id = annotation.id.clone();
                let delete_id = annotation.id.clone();
                let note = annotation.note.clone();
                let card = components::reader_state_card(("reader-annotation", index), selected, theme)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.annotation_popup_anchor = None;
                        this.select_annotation(id.clone(), window, cx);
                    }))
                    .child(components::reader_state_quote(format!("\u{201C}{}\u{201D}", annotation.exact_text)))
                    .when(!note.is_empty(), |card| card.child(components::reader_state_note(note, theme)))
                    .child(
                        components::reader_state_actions()
                            .child(
                                components::reader_sidebar_action_button(("reader-annotation-go", index), "Go", gpui_component::IconName::ArrowRight, true).on_click(cx.listener(move |this, _, _, cx| this.navigate_to_cfi(cfi.clone(), cx))),
                            )
                            .child(
                                components::reader_sidebar_action_button(("reader-annotation-delete", index), "Delete", gpui_component::IconName::Delete, true)
                                    .on_click(cx.listener(move |this, _, _, cx| this.delete_annotation(delete_id.clone(), cx))),
                            ),
                    );
                panel = panel.child(card);
            }
        }
        panel.into_any_element()
    }

    fn on_previous_page(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        self.hide_chrome(cx);
        self.previous_page(cx);
    }

    fn on_next_page(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        self.hide_chrome(cx);
        self.next_page(cx);
    }

    /// Turning a page is the signal that reading has started in earnest, so the
    /// toolbar steps aside until the pointer comes looking for it.
    fn hide_chrome(&mut self, cx: &mut Context<Self>) {
        if self.chrome_hidden {
            return;
        }
        self.chrome_hidden = true;
        crate::invalidation::notify(cx, Component::ReaderShell, "chrome_hidden");
    }

    fn show_chrome(&mut self, cx: &mut Context<Self>) {
        if !self.chrome_hidden {
            return;
        }
        self.chrome_hidden = false;
        crate::invalidation::notify(cx, Component::ReaderShell, "chrome_shown");
    }

    fn on_toggle_fullscreen(&mut self, _: &ToggleFullscreen, window: &mut Window, _: &mut Context<Self>) {
        window.toggle_fullscreen();
    }

    fn on_toggle_toc(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.show_chrome(cx);
        self.set_toc_visible(!self.toc_visible, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
    }

    pub(crate) fn set_toc_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.toc_visible = visible;
        ReaderSettings::update(cx, |preferences| preferences.toc_visible = visible);
    }

    fn on_focus_reader_search(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.show_chrome(cx);
        if !self.search.active {
            self.set_search_active(true, cx);
        }
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn on_toggle_search(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.show_chrome(cx);
        if self.search.active {
            self.close_search(window, cx);
        } else {
            self.set_search_active(true, cx);
            self.search_input.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    fn on_close_search(&mut self, _: &CloseSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.close_search(window, cx);
    }

    fn on_history_back(&mut self, _: &NavigateHistoryBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.can_history_back {
            self.navigate_history(-1, cx);
        }
    }

    fn on_history_forward(&mut self, _: &NavigateHistoryForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.can_history_forward {
            self.navigate_history(1, cx);
        }
    }

    fn on_increase_font(&mut self, _: &IncreaseFont, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font_size(1.0, cx);
    }

    fn on_decrease_font(&mut self, _: &DecreaseFont, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_font_size(-1.0, cx);
    }

    fn on_increase_scale(&mut self, _: &IncreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(0.1, cx);
    }

    fn on_decrease_scale(&mut self, _: &DecreaseScale, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_scale(-0.1, cx);
    }

    fn on_increase_column_width(&mut self, _: &IncreaseColumnWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_column_width(40.0, cx);
    }

    fn on_decrease_column_width(&mut self, _: &DecreaseColumnWidth, _: &mut Window, cx: &mut Context<Self>) {
        self.adjust_column_width(-40.0, cx);
    }
}

impl Focusable for ReaderView {
    /// Focusing the reader means focusing the document.
    ///
    /// Navigation keys are bound in the `ReaderDocument` context, which only
    /// the HTML view establishes, so anything that focuses this view has to
    /// land on the document or the keyboard goes dead. Answering with the
    /// document handle keeps that true for every caller instead of leaving a
    /// list of call sites to remember. Before the publication is ready there
    /// is no document, so the page container stands in.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.document_focus(cx).unwrap_or_else(|| self.focus.clone())
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::invalidation::record_render(Component::ReaderShell);
        let theme = components::browser_theme(cx);
        window.set_window_title(&self.book_title);
        let entity = cx.entity();
        let document = match &self.load_state {
            LoadState::Loading => components::reader_content().child(components::empty_document()).into_any_element(),
            LoadState::Ready(renderer) => {
                let selection_renderer = renderer.clone();
                let selection_owner = entity.clone();
                let highlight_target = entity.clone();
                let citation_target = entity.clone();
                let table_renderer = renderer.clone();
                let image_renderer = renderer.clone();
                components::reader_content()
                    .id("epub-reader-document")
                    .when(cfg!(feature = "kobo"), |document| document.px(px(KOBO_READER_EDGE_INSET)))
                    .child(renderer.clone())
                    .context_menu(move |menu, window, cx| {
                        let position = window.mouse_position();
                        let selection_hit = selection_owner.read(cx).marks.selection.is_some() && selection_renderer.read(cx).selection_contains(position);
                        let table_hit = table_renderer.read(cx).table_at(position);
                        let image_hit = image_renderer.read(cx).image_at(position);
                        let mut menu = components::ReaderContextMenu::new(menu);
                        if selection_hit {
                            let highlight_target = highlight_target.clone();
                            let citation_target = citation_target.clone();
                            menu = menu.selection(move |window, cx| highlight_target.update(cx, |this, cx| this.highlight_selection(window, cx)), move |_, cx| citation_target.update(cx, |this, cx| this.copy_selection_citation(cx)));
                        }
                        if table_hit {
                            let copy_markdown = table_renderer.clone();
                            let copy_unstyled_html = table_renderer.clone();
                            let copy_styled_html = table_renderer.clone();
                            menu = menu.table(
                                move |_, cx| {
                                    copy_markdown.update(cx, |renderer, _| {
                                        renderer.copy_table_at(position);
                                    });
                                },
                                move |_, cx| {
                                    copy_unstyled_html.update(cx, |renderer, _| {
                                        renderer.copy_table_unstyled_html_at(position);
                                    });
                                },
                                move |_, cx| {
                                    copy_styled_html.update(cx, |renderer, _| {
                                        renderer.copy_table_styled_html_at(position);
                                    });
                                },
                            );
                        }
                        if image_hit {
                            let copy_image = image_renderer.clone();
                            menu = menu.image(move |_, cx| {
                                copy_image.update(cx, |renderer, _| {
                                    renderer.copy_image_at(position);
                                });
                            });
                        }
                        menu.finish()
                    })
                    .into_any_element()
            }
            LoadState::Failed => components::reader_content().child(components::empty_document()).into_any_element(),
        };

        let mut document_body = components::reader_document_body().child(document);
        if let Some(annotation) = self.marks.selected_id.as_ref().and_then(|id| self.marks.annotation(id)).cloned() {
            let delete_id = annotation.id.clone();
            let sidebar_visible = self.reader_state_visible || self.toc_visible;
            let sidebar_width = chrome::sidebar_width(sidebar_visible, cx);
            let viewport = window.viewport_size();
            let preferences = ReaderSettings::preferences(cx);
            let popup = annotation_popup_geometry(self.annotation_popup_anchor, f32::from(viewport.width), f32::from(viewport.height), sidebar_width, preferences.column_width as f32, preferences.scale as f32, self.desired_column_count);
            document_body = document_body.child(
                components::reader_annotation_editor(theme)
                    .absolute()
                    .left(px(popup.left))
                    .max_h(px(popup.max_height))
                    .when(popup.above, |panel| panel.bottom(px(popup.edge_offset)))
                    .when(!popup.above, |panel| panel.top(px(popup.edge_offset)))
                    .child(components::reader_state_quote(format!("“{}”", annotation.exact_text)))
                    .child(components::reader_annotation_note_editor(theme).child(Input::new(&self.annotation_note_input).appearance(false).size_full()))
                    .child(
                        components::reader_state_actions()
                            .child(components::reader_control_button("reader-annotation-popup-save", "Save", theme).on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, cx))))
                            .child(components::reader_control_button("reader-annotation-popup-delete", "Delete", theme).on_click(cx.listener(move |this, _, _, cx| this.delete_annotation(delete_id.clone(), cx)))),
                    ),
            );
        }
        if self.search.active {
            let (current, total) = self.search.match_position.unwrap_or((0, 0));
            document_body = document_body.child(
                components::reader_search_bar(theme)
                    .child(components::reader_search_box(theme).key_context("ReaderSearch").child(Input::new(&self.search_input).appearance(false).size_full()))
                    .child(components::reader_match_count(format!("{current}/{total}"), theme))
                    .child(
                        components::reader_choice_button("reader-search-case", "Aa", self.search.filters.match_case)
                            .tooltip("Match case")
                            .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.match_case = !filters.match_case, cx))),
                    )
                    .child(
                        components::reader_choice_button("reader-search-word", "Word", self.search.filters.whole_word)
                            .tooltip("Match whole words")
                            .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.whole_word = !filters.whole_word, cx))),
                    )
                    .child(
                        components::reader_choice_button("reader-search-diacritics", "Marks", self.search.filters.match_diacritics)
                            .tooltip("Match diacritics exactly")
                            .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.match_diacritics = !filters.match_diacritics, cx))),
                    )
                    .child(
                        components::reader_choice_button("reader-search-scope", if self.search.filters.whole_book { "Book" } else { "Section" }, self.search.filters.whole_book)
                            .tooltip("Search current section or whole book")
                            .on_click(cx.listener(|this, _, _, cx| this.update_search_options(|filters| filters.whole_book = !filters.whole_book, cx))),
                    )
                    .child(components::reader_square_button("reader-search-previous", "‹", theme).tooltip("Previous match (Shift+Enter)").on_click(cx.listener(|this, _, _, cx| this.navigate_search(-1, cx))))
                    .child(components::reader_square_button("reader-search-next", "›", theme).tooltip("Next match (Enter)").on_click(cx.listener(|this, _, _, cx| this.navigate_search(1, cx))))
                    .child(components::reader_square_button("reader-search-close", "×", theme).tooltip("Close search (Esc)").on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))),
            );
        }
        if let Some((preview, prepared)) = self.footnote.clone() {
            let mut panel = components::reader_footnote_panel(theme).child(components::section_title("Footnote"));
            // The note arrives laid out to this panel's width, so it is drawn
            // with the document's own typography rather than flattened to text.
            if let LoadState::Ready(renderer) = &self.load_state {
                panel = panel.child(html_view_gpui::HtmlNoteElement::new(renderer.clone(), prepared));
            }
            panel = panel.child(components::reader_state_actions().child(components::reader_control_button("reader-footnote-open", "Go to note", theme).on_click(cx.listener(|this, _, _, cx| this.navigate_from_footnote(cx)))).child(
                components::reader_control_button("reader-footnote-close", "Close", theme).on_click(cx.listener(|this, _, _, cx| {
                    this.footnote = None;
                    crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
                })),
            ));
            document_body = document_body.child(panel);
        }
        let sidebar_visible = self.reader_state_visible || self.toc_visible;
        let split_sidebar_width = chrome::sidebar_width(sidebar_visible, cx);
        if let Some((uri, image)) = self.image_preview.clone() {
            let copy_image = entity.clone();
            let close_image = entity.clone();
            let viewport = window.viewport_size();
            let reader_width = (f32::from(viewport.width) - split_sidebar_width).max(1.0);
            let reader_height = (f32::from(viewport.height) - 58.0).max(1.0);
            document_body = document_body.child(components::ReaderImagePopup::new(uri, image, gpui::size(px(reader_width), px(reader_height))).on_copy(move |_, cx| copy_image.update(cx, |this, cx| this.copy_preview_image(cx))).on_dismiss(
                move |_, cx| {
                    close_image.update(cx, |this, cx| {
                        this.image_preview = None;
                        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
                    })
                },
            ));
        }

        let content = if sidebar_visible {
            let sidebar = if self.reader_state_visible { self.render_reader_state(cx) } else { self.render_toc(entity.clone(), theme, cx) };
            chrome::with_sidebar(sidebar, document_body.into_any_element(), cx)
        } else {
            document_body.into_any_element()
        };

        let page = components::reader_app_page(window, cx, theme)
            .key_context("Reader")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_previous_page))
            .on_action(cx.listener(Self::on_next_page))
            .on_action(cx.listener(Self::on_toggle_toc))
            .on_action(cx.listener(Self::on_focus_reader_search))
            .on_action(cx.listener(Self::on_toggle_search))
            .on_action(cx.listener(Self::on_close_search))
            .on_action(cx.listener(Self::on_history_back))
            .on_action(cx.listener(Self::on_history_forward))
            .on_action(cx.listener(Self::on_increase_font))
            .on_action(cx.listener(Self::on_decrease_font))
            .on_action(cx.listener(Self::on_increase_scale))
            .on_action(cx.listener(Self::on_decrease_scale))
            .on_action(cx.listener(Self::on_increase_column_width))
            .on_action(cx.listener(Self::on_decrease_column_width))
            .on_action(cx.listener(Self::on_toggle_fullscreen))
            .children((!self.chrome_hidden).then(|| self.toolbar_view.clone().into_any_element()))
            .child(components::reader_content().child(content));
        let kobo_tap_zone_top = px(if self.chrome_hidden { 0.0 } else { READER_TOOLBAR_HEIGHT });
        let page = page
            .children((cfg!(feature = "kobo") && !sidebar_visible).then(|| {
                gpui::div()
                    .id("kobo-epub-previous-page-tap-zone")
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
                    .id("kobo-epub-next-page-tap-zone")
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
                    .id("kobo-epub-toggle-toolbar-tap-zone")
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

/// The key context the HTML document element establishes while it holds focus.
///
/// Navigation binds here rather than to `Reader` so that page keys — `space`
/// above all — never fire while the search input has focus.
const DOCUMENT_CONTEXT: &str = "ReaderDocument";

/// GPUI's name for a navigation key.
fn navigation_key_name(key: ReaderNavigationKey) -> &'static str {
    match key {
        ReaderNavigationKey::ArrowUp => "up",
        ReaderNavigationKey::ArrowDown => "down",
        ReaderNavigationKey::ArrowLeft => "left",
        ReaderNavigationKey::ArrowRight => "right",
        ReaderNavigationKey::PageUp => "pageup",
        ReaderNavigationKey::PageDown => "pagedown",
        ReaderNavigationKey::Space => "space",
    }
}

/// Renders one contract shortcut as a GPUI keystroke.
///
/// `primary_modifier` is Ctrl on Linux and Windows and Cmd on macOS, matching
/// how the contract documents it.
fn navigation_keystroke(shortcut: &ReaderNavigationShortcut) -> String {
    let mut keystroke = String::new();
    if shortcut.primary_modifier {
        keystroke.push_str(if cfg!(target_os = "macos") { "cmd-" } else { "ctrl-" });
    }
    if shortcut.alt {
        keystroke.push_str("alt-");
    }
    if shortcut.shift {
        keystroke.push_str("shift-");
    }
    keystroke.push_str(navigation_key_name(shortcut.key));
    keystroke
}

/// Binds one contract shortcut to the reader action that performs it.
fn navigation_binding(shortcut: &ReaderNavigationShortcut) -> KeyBinding {
    let keystroke = navigation_keystroke(shortcut);
    let context = Some(DOCUMENT_CONTEXT);
    match shortcut.action {
        ReaderNavigationAction::PreviousLine => KeyBinding::new(&keystroke, PreviousLine, context),
        ReaderNavigationAction::NextLine => KeyBinding::new(&keystroke, NextLine, context),
        ReaderNavigationAction::PreviousPage => KeyBinding::new(&keystroke, PreviousPage, context),
        ReaderNavigationAction::NextPage => KeyBinding::new(&keystroke, NextPage, context),
        ReaderNavigationAction::PreviousSection => KeyBinding::new(&keystroke, PreviousSection, context),
        ReaderNavigationAction::NextSection => KeyBinding::new(&keystroke, NextSection, context),
        ReaderNavigationAction::HistoryBack => KeyBinding::new(&keystroke, NavigateHistoryBack, context),
        ReaderNavigationAction::HistoryForward => KeyBinding::new(&keystroke, NavigateHistoryForward, context),
    }
}

pub(crate) fn bind_keys(cx: &mut App) {
    // Navigation is derived from the shared contract so that desktop and the
    // web reader cannot drift apart. Everything below it is desktop-only
    // chrome that the contract does not describe.
    let mut bindings = READER_NAVIGATION_SHORTCUTS.iter().map(navigation_binding).collect::<Vec<_>>();
    bindings.extend([
        KeyBinding::new("f11", ToggleFullscreen, Some("Reader")),
        KeyBinding::new("ctrl-f", ToggleSearch, Some("Reader")),
        KeyBinding::new("escape", CloseSearch, Some("Reader")),
        KeyBinding::new("ctrl-=", IncreaseScale, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("ctrl-+", IncreaseScale, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("ctrl--", DecreaseScale, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("ctrl-alt-=", IncreaseColumnWidth, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("ctrl-alt-+", IncreaseColumnWidth, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("ctrl-alt--", DecreaseColumnWidth, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("alt-=", IncreaseFont, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("alt-+", IncreaseFont, Some(DOCUMENT_CONTEXT)),
        KeyBinding::new("alt--", DecreaseFont, Some(DOCUMENT_CONTEXT)),
    ]);
    cx.bind_keys(bindings);
}

#[cfg(test)]
mod key_binding_tests {
    use super::*;

    #[test]
    fn every_contract_shortcut_becomes_a_binding() {
        let bindings = READER_NAVIGATION_SHORTCUTS.iter().map(navigation_binding).count();
        assert_eq!(bindings, READER_NAVIGATION_SHORTCUTS.len(), "a contract shortcut must not be dropped on the way to GPUI");
    }

    #[test]
    fn modifiers_are_rendered_in_gpui_order() {
        let plain = ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowRight, false, false, false, ReaderNavigationAction::NextPage);
        let history = ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowLeft, false, true, false, ReaderNavigationAction::HistoryBack);
        let section = ReaderNavigationShortcut::new(ReaderNavigationKey::ArrowRight, true, false, false, ReaderNavigationAction::NextSection);

        assert_eq!(navigation_keystroke(&plain), "right");
        assert_eq!(navigation_keystroke(&history), "alt-left");
        assert_eq!(navigation_keystroke(&section), if cfg!(target_os = "macos") { "cmd-right" } else { "ctrl-right" });
    }

    #[test]
    fn space_turns_the_page_and_is_document_scoped() {
        let space = READER_NAVIGATION_SHORTCUTS.iter().find(|shortcut| shortcut.key == ReaderNavigationKey::Space).expect("the contract binds space");
        assert_eq!(space.action, ReaderNavigationAction::NextPage);
        assert_eq!(navigation_keystroke(space), "space");
    }

    #[test]
    fn annotation_popup_centers_in_the_clicked_column() {
        let left = annotation_popup_geometry(Some(Point::new(px(100.0), px(200.0))), 1200.0, 800.0, 0.0, 500.0, 1.0, 2);
        let right = annotation_popup_geometry(Some(Point::new(px(900.0), px(200.0))), 1200.0, 800.0, 0.0, 500.0, 1.0, 2);

        assert_eq!(left.left, 120.0);
        assert_eq!(right.left, 720.0);
    }

    #[test]
    fn annotation_popup_uses_the_roomier_vertical_side() {
        let near_top = annotation_popup_geometry(Some(Point::new(px(100.0), px(140.0))), 1200.0, 800.0, 0.0, 600.0, 1.0, 2);
        let near_bottom = annotation_popup_geometry(Some(Point::new(px(100.0), px(700.0))), 1200.0, 800.0, 0.0, 600.0, 1.0, 2);

        assert!(!near_top.above);
        assert!(near_bottom.above);
    }
}
