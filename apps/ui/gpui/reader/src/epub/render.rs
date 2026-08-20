use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::settings::ReaderSettings;
use crate::shell::chrome;
use crate::{
    CloseSearch, DecreaseColumnWidth, DecreaseFont, DecreaseScale, FocusReaderSearch, IncreaseColumnWidth, IncreaseFont, IncreaseScale, NavigateHistoryBack, NavigateHistoryForward, NextLine, NextPage, NextSection, PreviousLine,
    PreviousPage, PreviousSection, ToggleFullscreen, ToggleSearch, ToggleToc,
};
use crate::shell::state_panel;
use crate::shell::annotation_editor;
use std::rc::Rc;

use app_contract::{READER_NAVIGATION_SHORTCUTS, ReaderNavigationAction, ReaderNavigationKey, ReaderNavigationShortcut};
use epub_svelte_lib::library::marks::{AnnotationStyle, ReaderAnnotation, ReaderBookmark};
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, Pixels, Point, Render, Window, px};
use gpui_component::input::Input;
use gpui_component::menu::ContextMenuExt;
use ui_components as components;

const READER_TOOLBAR_HEIGHT: f32 = 58.0;
/// Keeps glyph ink and logical-to-framebuffer rounding inside the Kobo panel.
/// This is a renderer guard band, not an authored EPUB margin.
const KOBO_READER_EDGE_INSET: f32 = 2.0;
/// Places the annotation editor, deriving the column count the way the
/// renderer does.
///
/// The EPUB view is not told how many columns it ended up with — it asks for a
/// column width and the renderer fits as many as will go, capped by the
/// reader's preference. The panel has to land on the same column the click did,
/// so that arithmetic is repeated here before the shared placement runs.
fn annotation_popup_geometry(
    anchor: Option<Point<Pixels>>, viewport_width: f32, viewport_height: f32, sidebar_width: f32, requested_column_width: f32, scale: f32, maximum_columns: u8,
) -> annotation_editor::Geometry {
    let document_width = annotation_editor::document_width(viewport_width, sidebar_width);
    let scale = scale.max(0.01);
    let minimum_gap = 20.0 * scale;
    let requested_column_width = requested_column_width.max(1.0) * scale;
    let derived_columns = ((document_width + minimum_gap) / (requested_column_width + minimum_gap)).floor().max(1.0) as u8;
    let column_count = derived_columns.min(maximum_columns.max(1));
    annotation_editor::geometry(anchor, viewport_width, viewport_height, sidebar_width, READER_TOOLBAR_HEIGHT, usize::from(column_count))
}

impl ReaderView {
    fn render_sidebar_tabs(&self, show_annotations: bool, cx: &mut Context<Self>) -> AnyElement {
        crate::shell::state_panel::reader_sidebar_tabs("reader-sidebar-tabs", &cx.entity(), show_annotations, true, cx, |annotations, this, cx| {
            this.reader_state_visible = annotations;
            this.set_toc_visible(!annotations, cx);
            crate::invalidation::notify(cx, Component::ReaderShell, "sidebar_tab_changed");
        })
    }

    fn render_toc(&self, entity: Entity<Self>, theme: components::BrowserTheme, cx: &mut Context<Self>) -> AnyElement {
        let toc = state_panel::reader_toc_tree(&self.toc_tree, &entity, theme, |link, window, this: &mut Self, cx| {
            this.navigate_to(link.to_owned(), cx);
            this.focus_document(window, cx);
            #[cfg(feature = "kobo")]
            {
                this.toc_visible = false;
                crate::invalidation::notify(cx, Component::ReaderShell, "toc_navigated");
            }
        });
        components::reader_toc(theme).child(self.render_sidebar_tabs(false, cx)).child(toc).into_any_element()
    }

    fn render_reader_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = components::browser_theme(cx);
        let tabs_target = cx.entity();
        let panel = components::reader_state_panel(theme).child(self.render_sidebar_tabs(true, cx)).child(
            crate::shell::state_panel::reader_state_tabs("reader-state-tabs", &tabs_target, self.reader_state_bookmarks, &self.marks, cx, |showing_bookmarks, this, cx| {
                this.reader_state_bookmarks = showing_bookmarks;
                crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
            }),
        );

        let wording = state_panel::StatePanelWording {
            bookmark_tooltip: "Bookmark the current location",
            no_bookmarks: "No bookmarks yet. Use Bookmark here to save this location.",
            no_annotations: "Select text, then use Highlight to create an annotation.",
        };
        let actions = state_panel::StatePanelActions {
            add_bookmark: Rc::new(|this: &mut Self, cx| this.add_bookmark(cx)),
            go_to_bookmark: Rc::new(|bookmark: &ReaderBookmark, this: &mut Self, cx| this.navigate_to_cfi(bookmark.cfi.clone(), cx)),
            delete_bookmark: Rc::new(|id, this: &mut Self, cx| this.delete_bookmark(id, cx)),
            select_annotation: Rc::new(|id, window, this: &mut Self, cx| {
                this.annotation_popup_anchor = None;
                this.select_annotation(id, window, cx);
            }),
            go_to_annotation: Rc::new(|annotation: &ReaderAnnotation, this: &mut Self, cx| this.navigate_to_cfi(annotation.anchor.epub_cfi_value().unwrap_or_default().to_owned(), cx)),
            delete_annotation: Rc::new(|id, this: &mut Self, cx| this.delete_annotation(id, cx)),
        };
        // A bookmark records where the reader is, so there has to be a position
        // to record before the button means anything.
        let can_bookmark = self.current_cfi.is_some();
        panel.children(state_panel::reader_state_rows(&cx.entity(), &self.marks, self.reader_state_bookmarks, can_bookmark, &wording, &actions, theme)).into_any_element()
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
                        let selection_hit = selection_owner.read(cx).selection.is_some() && selection_renderer.read(cx).selection_contains(position);
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
            // Saving a style or a colour leaves the editor open so several can
            // be tried; only closing and deleting dismiss it. Those two have to
            // hand the keyboard back, because the note input inside the editor
            // may be what holds it, and focus left on an element that no longer
            // renders puts every key binding out of reach.
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
                    save("reader-annotation-popup-save", "Save", None, None),
                    annotation_editor::Action::new("reader-annotation-popup-close", "Close", move |window, cx| {
                        close_target.update(cx, |this, cx| {
                            this.close_annotation_popup(cx);
                            this.focus_document(window, cx);
                        });
                    }),
                    annotation_editor::Action::new("reader-annotation-popup-delete", "Delete", move |window, cx| {
                        let id = delete_id.clone();
                        delete_target.update(cx, |this, cx| {
                            this.delete_annotation(id, cx);
                            this.focus_document(window, cx);
                        });
                    }),
                ],
                vec![
                    save("reader-annotation-popup-highlight", "Highlight", Some(AnnotationStyle::Highlight), None),
                    save("reader-annotation-popup-underline", "Underline", Some(AnnotationStyle::Underline), None),
                    save("reader-annotation-popup-squiggly", "Squiggly", Some(AnnotationStyle::Squiggly), None),
                    save("reader-annotation-popup-strike", "Strike", Some(AnnotationStyle::Strikethrough), None),
                ],
                vec![
                    save("reader-annotation-popup-yellow", "Yellow", None, Some("#f6c945")),
                    save("reader-annotation-popup-blue", "Blue", None, Some("#60a5fa")),
                    save("reader-annotation-popup-pink", "Pink", None, Some("#f472b6")),
                    save("reader-annotation-popup-green", "Green", None, Some("#4ade80")),
                ],
            ];
            document_body = document_body.child(annotation_editor::editor(popup, &annotation.exact_text, &self.annotation_note_input, rows, theme));
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

        // The page container is only focusable while the publication is
        // loading, when it is the one thing that can hold the keyboard. A
        // focusable container steals focus from the document on any click that
        // reaches it — a margin, the toolbar's background, the sidebar — and
        // navigation is bound in the document's key context, so the paging keys
        // would go dead until the text was clicked again. Dropping the handle
        // once the document exists leaves clicks on the chrome harmless; the
        // container still sits on the dispatch path, so `Reader` bindings keep
        // firing.
        let document_ready = self.document_focus(cx).is_some();
        let page = components::reader_app_page(window, cx, theme)
            .key_context("Reader")
            .when(!document_ready, |page| page.track_focus(&self.focus))
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
