use crate::epub::{LoadState, ReaderView};
use crate::invalidation::Component;
use crate::settings::ReaderSettings;
use crate::shell::chrome;
use crate::{CloseSearch, DecreaseColumnWidth, DecreaseFont, DecreaseScale, FocusReaderSearch, IncreaseColumnWidth, IncreaseFont, IncreaseScale, NavigateHistoryBack, NavigateHistoryForward, NextPage, PreviousPage, ToggleSearch, ToggleToc};
use ui_components as components;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, Render, Window, px};
use gpui_component::input::Input;
use gpui_component::menu::ContextMenuExt;
use gpui_component::tree::tree;

impl ReaderView {
    fn render_toc(&self, entity: Entity<Self>) -> AnyElement {
        let toc = tree(&self.toc_tree, move |index, entry, _selected, _window, _cx| {
            let item = entry.item();
            let link = item.id.to_string();
            let navigation_target = entity.clone();
            let title = components::reader_toc_title(("toc-tree-title", index), item.label.clone()).on_click(move |_, _, cx| {
                navigation_target.update(cx, |this, cx| {
                    this.navigate_to(link.clone(), cx);
                    #[cfg(feature = "kobo")]
                    {
                        this.toc_visible = false;
                        crate::invalidation::notify(cx, Component::ReaderShell, "toc_navigated");
                    }
                });
            });
            components::reader_toc_tree_item(("toc-tree-item", index), ("toc-tree-disclosure", index), entry.depth(), entry.is_folder(), entry.is_expanded(), title)
        });
        components::reader_toc().child(toc).into_any_element()
    }

    fn render_reader_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = components::reader_state_panel().child(
            components::reader_state_tabs()
                .child(components::reader_choice_button("reader-annotations-tab", format!("Annotations ({})", self.marks.annotations.len()), !self.reader_state_bookmarks).on_click(cx.listener(|this, _, _, cx| {
                    this.reader_state_bookmarks = false;
                    crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
                })))
                .child(components::reader_choice_button("reader-bookmarks-tab", format!("Bookmarks ({})", self.marks.bookmarks.len()), self.reader_state_bookmarks).on_click(cx.listener(|this, _, _, cx| {
                    this.reader_state_bookmarks = true;
                    crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
                }))),
        );

        if self.reader_state_bookmarks {
            panel = panel
                .child(components::reader_control_button_state("reader-bookmark-current", "Bookmark here", self.current_cfi.is_some()).tooltip("Bookmark the current location").on_click(cx.listener(|this, _, _, cx| this.add_bookmark(cx))));
            if self.marks.bookmarks.is_empty() {
                panel = panel.child(components::reader_state_note("No bookmarks yet. Use Bookmark here to save this location."));
            }
            for (index, bookmark) in self.marks.bookmarks.iter().cloned().enumerate() {
                let cfi = bookmark.cfi.clone();
                let id = bookmark.id.clone();
                panel = panel.child(
                    components::reader_state_card(("reader-bookmark", index), false).child(components::reader_state_quote(bookmark.label)).child(
                        components::reader_state_actions()
                            .child(components::reader_control_button(("reader-bookmark-go", index), "Go").on_click(cx.listener(move |this, _, _, cx| this.navigate_to_cfi(cfi.clone(), cx))))
                            .child(components::reader_control_button(("reader-bookmark-delete", index), "Delete").on_click(cx.listener(move |this, _, _, cx| this.delete_bookmark(id.clone(), cx)))),
                    ),
                );
            }
        } else {
            if self.marks.annotations.is_empty() {
                panel = panel.child(components::reader_state_note("Select text, then use Highlight to create an annotation."));
            }
            for (index, annotation) in self.marks.annotations.iter().cloned().enumerate() {
                let selected = self.marks.selected_id.as_deref() == Some(annotation.id.as_str());
                let cfi = annotation.anchor.epub_cfi_value().unwrap_or_default().to_owned();
                let id = annotation.id.clone();
                let delete_id = annotation.id.clone();
                let note = annotation.note.clone();
                let card = components::reader_state_card(("reader-annotation", index), selected)
                    .on_click(cx.listener(move |this, _, window, cx| this.select_annotation(id.clone(), window, cx)))
                    .child(components::reader_state_quote(format!("\u{201C}{}\u{201D}", annotation.exact_text)))
                    .when(!note.is_empty(), |card| card.child(components::reader_state_note(note)))
                    .child(
                        components::reader_state_actions()
                            .child(components::reader_control_button(("reader-annotation-go", index), "Go").on_click(cx.listener(move |this, _, _, cx| this.navigate_to_cfi(cfi.clone(), cx))))
                            .child(components::reader_control_button(("reader-annotation-delete", index), "Delete").on_click(cx.listener(move |this, _, _, cx| this.delete_annotation(delete_id.clone(), cx)))),
                    );
                panel = panel.child(card);
            }
        }
        panel.into_any_element()
    }

    fn on_previous_page(&mut self, _: &PreviousPage, _: &mut Window, cx: &mut Context<Self>) {
        self.previous_page(cx);
    }

    fn on_next_page(&mut self, _: &NextPage, _: &mut Window, cx: &mut Context<Self>) {
        self.next_page(cx);
    }

    fn on_toggle_toc(&mut self, _: &ToggleToc, _: &mut Window, cx: &mut Context<Self>) {
        self.set_toc_visible(!self.toc_visible, cx);
        crate::invalidation::notify(cx, Component::ReaderShell, "state_changed");
    }

    pub(crate) fn set_toc_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.toc_visible = visible;
        ReaderSettings::update(cx, |preferences| preferences.toc_visible = visible);
    }

    fn on_focus_reader_search(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.active {
            self.set_search_active(true, cx);
        }
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn on_toggle_search(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
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
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::invalidation::record_render(Component::ReaderShell);
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
            document_body = document_body.child(
                components::reader_annotation_editor()
                    .child(
                        components::reader_annotation_editor_header()
                            .child(components::section_title("Annotation"))
                            .child(components::reader_square_button("reader-annotation-popup-close", "×").tooltip("Close").on_click(cx.listener(|this, _, _, cx| this.close_annotation_popup(cx)))),
                    )
                    .child(components::reader_state_quote(format!("“{}”", annotation.exact_text)))
                    .child(components::reader_note_editor().child(Input::new(&self.annotation_note_input).appearance(false).size_full()))
                    .child(
                        components::reader_state_actions()
                            .child(components::reader_control_button("reader-annotation-popup-save", "Save").on_click(cx.listener(|this, _, _, cx| this.save_selected_annotation(None, cx))))
                            .child(components::reader_control_button("reader-annotation-popup-delete", "Delete").on_click(cx.listener(move |this, _, _, cx| this.delete_annotation(delete_id.clone(), cx)))),
                    ),
            );
        }
        if self.search.active {
            let (current, total) = self.search.match_position.unwrap_or((0, 0));
            document_body = document_body.child(
                components::reader_search_bar()
                    .child(components::reader_search_box().key_context("ReaderSearch").child(Input::new(&self.search_input).appearance(false).size_full()))
                    .child(components::reader_match_count(format!("{current}/{total}")))
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
                    .child(components::reader_square_button("reader-search-previous", "‹").tooltip("Previous match (Shift+Enter)").on_click(cx.listener(|this, _, _, cx| this.navigate_search(-1, cx))))
                    .child(components::reader_square_button("reader-search-next", "›").tooltip("Next match (Enter)").on_click(cx.listener(|this, _, _, cx| this.navigate_search(1, cx))))
                    .child(components::reader_square_button("reader-search-close", "×").tooltip("Close search (Esc)").on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))),
            );
        }
        if let Some(preview) = self.footnote.clone() {
            let mut panel = components::reader_footnote_panel().child(components::section_title("Footnote"));
            for block in preview.blocks {
                panel = panel.child(components::reader_state_quote(block));
            }
            panel = panel.child(components::reader_state_actions().child(components::reader_control_button("reader-footnote-open", "Go to note").on_click(cx.listener(|this, _, _, cx| this.navigate_from_footnote(cx)))).child(
                components::reader_control_button("reader-footnote-close", "Close").on_click(cx.listener(|this, _, _, cx| {
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
            let sidebar = if self.reader_state_visible { self.render_reader_state(cx) } else { self.render_toc(entity.clone()) };
            chrome::with_sidebar(sidebar, document_body.into_any_element(), cx)
        } else {
            document_body.into_any_element()
        };

        let page = components::page()
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
            .child(self.toolbar_view.clone());
        page.child(components::reader_content().child(content))
    }
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-left", NavigateHistoryBack, Some("Reader")),
        KeyBinding::new("alt-right", NavigateHistoryForward, Some("Reader")),
        KeyBinding::new("ctrl-f", ToggleSearch, Some("Reader")),
        KeyBinding::new("escape", CloseSearch, Some("Reader")),
        KeyBinding::new("ctrl-=", IncreaseScale, Some("ReaderDocument")),
        KeyBinding::new("ctrl--", DecreaseScale, Some("ReaderDocument")),
        KeyBinding::new("ctrl-alt-=", IncreaseColumnWidth, Some("ReaderDocument")),
        KeyBinding::new("ctrl-alt--", DecreaseColumnWidth, Some("ReaderDocument")),
        KeyBinding::new("alt-=", IncreaseFont, Some("Reader")),
        KeyBinding::new("alt--", DecreaseFont, Some("Reader")),
    ]);
}
