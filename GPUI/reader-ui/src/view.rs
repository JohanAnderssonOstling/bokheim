use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, Render, Window};
use gpui_component::input::Input;
use gpui_component::tree::tree;
use html_view_core::RendererCommand;

use crate::model::{LoadState, ReaderSettings, ReaderView};
use crate::{
    CloseSearch, DecreaseColumnWidth, DecreaseFont, DecreaseScale, FocusReaderSearch, IncreaseColumnWidth, IncreaseFont, IncreaseScale, NavigateHistoryBack, NavigateHistoryForward, NextPage, PreviousPage, ToggleLineNumbers, ToggleSearch,
    ToggleToc,
};

impl ReaderView {
    fn render_toc(&self, entity: Entity<Self>) -> AnyElement {
        let toc = tree(&self.toc_tree, move |index, entry, _selected, _window, _cx| {
            let item = entry.item();
            let link = item.id.to_string();
            let navigation_target = entity.clone();
            components::reader_toc_tree_item(("toc-tree-item", index), entry.depth(), entry.is_folder(), entry.is_expanded(), item.label.clone()).on_click(move |_, _, cx| {
                navigation_target.update(cx, |this, cx| {
                    this.navigate_to(link.clone(), cx);
                });
            })
        });
        components::reader_toc().child(toc).into_any_element()
    }

    fn render_settings_panel(entity: Entity<Self>, current: crate::ReaderPreferences) -> AnyElement {
        let decrease_column = entity.clone();
        let increase_column = entity.clone();
        let decrease_font = entity.clone();
        let increase_font = entity.clone();
        let decrease_scale = entity.clone();
        let increase_scale = entity.clone();
        let toggle_lines = entity;

        components::reader_settings_dropdown()
            .child(components::section_title("Reader settings"))
            .child(
                components::reader_setting_row("Column width")
                    .child(components::reader_square_button("column-width-smaller", "−").tooltip("Decrease column width (Ctrl+Alt+-)").on_click(move |_, _, cx| {
                        decrease_column.update(cx, |this, cx| {
                            this.adjust_column_width(-40.0, cx);
                        });
                    }))
                    .child(components::reader_setting_value(format!("{:.0}px", current.column_width,)))
                    .child(components::reader_square_button("column-width-larger", "+").tooltip("Increase column width (Ctrl+Alt++)").on_click(move |_, _, cx| {
                        increase_column.update(cx, |this, cx| {
                            this.adjust_column_width(40.0, cx);
                        });
                    })),
            )
            .child(
                components::reader_setting_row("Font size")
                    .child(components::reader_square_button("font-size-smaller", "−").tooltip("Decrease font size (Alt+↓)").on_click(move |_, _, cx| {
                        decrease_font.update(cx, |this, cx| {
                            this.adjust_font_size(-1.0, cx);
                        });
                    }))
                    .child(components::reader_setting_value(format!("{:.0}px", current.font_size,)))
                    .child(components::reader_square_button("font-size-larger", "+").tooltip("Increase font size (Alt+↑)").on_click(move |_, _, cx| {
                        increase_font.update(cx, |this, cx| {
                            this.adjust_font_size(1.0, cx);
                        });
                    })),
            )
            .child(
                components::reader_setting_row("Zoom")
                    .child(components::reader_square_button("reader-zoom-out", "−").tooltip("Zoom out (Ctrl+-)").on_click(move |_, _, cx| {
                        decrease_scale.update(cx, |this, cx| {
                            this.adjust_scale(-0.1, cx);
                        });
                    }))
                    .child(components::reader_setting_value(format!("{:.1}x", current.scale,)))
                    .child(components::reader_square_button("reader-zoom-in", "+").tooltip("Zoom in (Ctrl++)").on_click(move |_, _, cx| {
                        increase_scale.update(cx, |this, cx| {
                            this.adjust_scale(0.1, cx);
                        });
                    })),
            )
            .child(components::reader_setting_row("Line numbers").child(components::reader_line_numbers_switch(current.line_numbers).on_click(move |enabled, _, cx| {
                let enabled = *enabled;
                toggle_lines.update(cx, |this, cx| {
                    this.update_preferences(cx, |preferences| {
                        preferences.line_numbers = enabled;
                    });
                    this.with_renderer(cx, |renderer, cx| {
                        renderer.apply(RendererCommand::SetLineNumbers(enabled), cx);
                    });
                    cx.notify();
                });
            })))
            .child(
                components::reader_shortcuts()
                    .child(components::section_title("Keyboard shortcuts"))
                    .child(components::reader_shortcut("↑ / ↓ or mouse wheel: previous / next line"))
                    .child(components::reader_shortcut("← / →: previous / next page"))
                    .child(components::reader_shortcut("Ctrl + ← / →: previous / next chapter"))
                    .child(components::reader_shortcut("Alt + ← / →: link history"))
                    .child(components::reader_shortcut("Ctrl + F: toggle search"))
                    .child(components::reader_shortcut("Enter / Shift+Enter in search: next / previous match"))
                    .child(components::reader_shortcut("Esc: close search"))
                    .child(components::reader_shortcut("L: toggle line numbers"))
                    .child(components::reader_shortcut("Ctrl + C: copy current selection"))
                    .child(components::reader_shortcut("Ctrl + +/-: zoom"))
                    .child(components::reader_shortcut("Ctrl + Alt + +/-: column width +/-"))
                    .child(components::reader_shortcut("Alt + ↑ / ↓: font size +/-")),
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
        self.toc_visible = !self.toc_visible;
        cx.notify();
    }

    fn on_focus_reader_search(&mut self, _: &FocusReaderSearch, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_active {
            self.set_search_active(true, cx);
        }
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn on_toggle_search(&mut self, _: &ToggleSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_active {
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

    fn on_toggle_line_numbers(&mut self, _: &ToggleLineNumbers, _: &mut Window, cx: &mut Context<Self>) {
        let enabled = !cx.global::<ReaderSettings>().preferences.line_numbers;
        self.update_preferences(cx, |preferences| {
            preferences.line_numbers = enabled;
        });
        self.with_renderer(cx, |renderer, cx| {
            renderer.apply(RendererCommand::SetLineNumbers(enabled), cx);
        });
        cx.notify();
    }
}

impl Focusable for ReaderView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ReaderView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&self.book_title);
        let entity = cx.entity();
        let resize_target = entity.clone();
        let previous_match = entity.clone();
        let next_match = entity.clone();
        let close_search = entity.clone();
        let history_back_target = entity.clone();
        let history_forward_target = entity.clone();
        let toc_target = entity.clone();
        let search_target = entity.clone();
        let document = match &self.load_state {
            LoadState::Loading => components::status("Loading…").into_any_element(),
            LoadState::Ready(renderer) => renderer.clone().into_any_element(),
            LoadState::Failed => components::empty_document().into_any_element(),
        };

        let mut document_body = components::reader_document_body().child(components::reader_content().child(document));
        if self.search_active {
            let (current, total) = self.search_match.unwrap_or((0, 0));
            document_body = document_body.child(
                components::reader_search_bar()
                    .child(components::reader_search_box().key_context("ReaderSearch").child(Input::new(&self.search_input).appearance(false).size_full()))
                    .child(components::reader_match_count(format!("{current}/{total}")))
                    .child(components::reader_square_button("reader-search-previous", "‹").tooltip("Previous match (Shift+Enter)").on_click(move |_, _, cx| {
                        previous_match.update(cx, |this, cx| {
                            this.navigate_search(-1, cx);
                        });
                    }))
                    .child(components::reader_square_button("reader-search-next", "›").tooltip("Next match (Enter)").on_click(move |_, _, cx| {
                        next_match.update(cx, |this, cx| {
                            this.navigate_search(1, cx);
                        });
                    }))
                    .child(components::reader_square_button("reader-search-close", "×").tooltip("Close search (Esc)").on_click(move |_, window, cx| {
                        close_search.update(cx, |this, cx| {
                            this.close_search(window, cx);
                        });
                    })),
            );
        }

        let content = if self.toc_visible {
            let width = cx.global::<ReaderSettings>().preferences.toc_width;
            components::reader_split(self.render_toc(entity.clone()), document_body, width)
                .on_resize(move |state, _, cx| {
                    let Some(width) = state.read(cx).sizes().first().copied() else {
                        return;
                    };
                    let width = f32::from(width);
                    resize_target.update(cx, |this, cx| {
                        let current = cx.global::<ReaderSettings>().preferences.toc_width;
                        if (current - width).abs() >= 1.0 {
                            this.update_preferences(cx, |preferences| {
                                preferences.toc_width = width;
                            });
                        }
                    });
                })
                .into_any_element()
        } else {
            document_body.into_any_element()
        };

        let history_back = self.can_history_back;
        let history_forward = self.can_history_forward;
        let preferences = cx.global::<ReaderSettings>().preferences.clone();
        let settings_panel = Self::render_settings_panel(entity.clone(), preferences);
        let toolbar = components::reader_toolbar()
            .child(components::reader_control_button_state("reader-history-back", "←", history_back).tooltip("Link history back (Alt+←)").on_click(move |_, _, cx| {
                history_back_target.update(cx, |this, cx| {
                    if this.can_history_back {
                        this.navigate_history(-1, cx);
                    }
                });
            }))
            .child(components::reader_control_button_state("reader-history-forward", "→", history_forward).tooltip("Link history forward (Alt+→)").on_click(move |_, _, cx| {
                history_forward_target.update(cx, |this, cx| {
                    if this.can_history_forward {
                        this.navigate_history(1, cx);
                    }
                });
            }))
            .child(components::reader_control_button("reader-toc", "Contents").tooltip("Toggle table of contents").on_click(move |_, _, cx| {
                toc_target.update(cx, |this, cx| {
                    this.toc_visible = !this.toc_visible;
                    cx.notify();
                });
            }))
            .child(components::reader_title(self.display_title()))
            .child(components::reader_control_button("reader-search", "Search").tooltip("Search in book (Ctrl+F)").on_click(move |_, window, cx| {
                search_target.update(cx, |this, cx| {
                    if !this.search_active {
                        this.set_search_active(true, cx);
                    }
                    this.search_input.read(cx).focus_handle(cx).focus(window, cx);
                });
            }))
            .child(components::reader_settings_popover().child(settings_panel));

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
            .on_action(cx.listener(Self::on_toggle_line_numbers))
            .child(toolbar);
        page.child(components::reader_content().child(content))
    }
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PreviousPage, Some("ReaderDocument")),
        KeyBinding::new("right", NextPage, Some("ReaderDocument")),
        KeyBinding::new("alt-left", NavigateHistoryBack, Some("Reader")),
        KeyBinding::new("alt-right", NavigateHistoryForward, Some("Reader")),
        KeyBinding::new("ctrl-f", ToggleSearch, Some("Reader")),
        KeyBinding::new("escape", CloseSearch, Some("Reader")),
        KeyBinding::new("ctrl-=", IncreaseScale, Some("ReaderDocument")),
        KeyBinding::new("ctrl--", DecreaseScale, Some("ReaderDocument")),
        KeyBinding::new("ctrl-alt-=", IncreaseColumnWidth, Some("ReaderDocument")),
        KeyBinding::new("ctrl-alt--", DecreaseColumnWidth, Some("ReaderDocument")),
        KeyBinding::new("alt-up", IncreaseFont, Some("Reader")),
        KeyBinding::new("alt-down", DecreaseFont, Some("Reader")),
    ]);
}
