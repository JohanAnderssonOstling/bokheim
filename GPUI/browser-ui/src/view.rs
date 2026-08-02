use std::sync::Arc;

use bokheim_ui_components as components;
use epub_svelte_lib::BookLocator;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Entity, FocusHandle, Focusable, IntoElement, KeyBinding, Render, SharedString, Window, uniform_list};
use gpui_component::IconName;
use gpui_component::input::Input;

use crate::model::{BookCard, BrowserSection};
use crate::{BrowserView, DismissSettings, FocusSearch, OpenReader, ShowAuthors, ShowFolders, ShowHome, ShowSearch, ShowSettings, ShowSync};

impl BrowserView {
    pub(crate) fn active_library_name(&self) -> SharedString {
        self.libraries.iter().find(|entry| Some(entry.library_id().as_str()) == self.active_library_id.as_deref()).map(|entry| SharedString::from(entry.library_name().to_owned())).unwrap_or_else(|| SharedString::from("No library"))
    }

    fn render_library_switcher(&self, entity: Entity<Self>, theme: components::BrowserTheme) -> impl IntoElement {
        let active_library_id = self.active_library_id.clone();
        let libraries = self.libraries.iter().map(|entry| (entry.library_id().as_str().to_owned(), SharedString::from(entry.library_name().to_owned()))).collect::<Vec<_>>();

        components::library_switcher_popover(self.active_library_name()).content(move |_, _, popover_cx| {
            let popover = popover_cx.entity();
            let mut panel = components::library_switcher_panel().child(components::library_switcher_title("Libraries", theme));
            for (index, (library_id, library_name)) in libraries.iter().enumerate() {
                let selected = active_library_id.as_deref() == Some(library_id.as_str());
                let target = entity.clone();
                let target_library_id = library_id.clone();
                let popover = popover.clone();
                panel = panel.child(components::library_switcher_item(format!("library-switcher-{index}"), library_name.clone(), selected).on_click(move |_, window, cx| {
                    target.update(cx, |this, cx| {
                        if this.active_library_id.as_deref() != Some(target_library_id.as_str()) {
                            this.set_library(target_library_id.clone(), cx);
                        }
                    });
                    popover.update(cx, |state, cx| {
                        state.dismiss(window, cx);
                    });
                }));
            }
            panel
        })
    }

    fn render_section_sidebar(&self, theme: components::BrowserTheme) -> impl IntoElement {
        components::sidebar(theme)
            .child(components::nav_rail_item("section-home", IconName::LayoutDashboard, "Home", !self.search_active && self.section == BrowserSection::Home, theme).on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(ShowHome), cx);
            }))
            .child(components::nav_rail_item("section-folders", IconName::BookOpen, "Library", !self.search_active && self.section == BrowserSection::Folders, theme).on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(ShowFolders), cx);
            }))
            .child(components::nav_rail_item("section-authors", IconName::User, "Authors", !self.search_active && self.section == BrowserSection::Authors, theme).on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(ShowAuthors), cx);
            }))
            .child(components::nav_rail_item("section-search", IconName::Search, "Search", self.section == BrowserSection::Search, theme).on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(ShowSearch), cx);
            }))
            .child(components::nav_rail_item("section-sync", IconName::Globe, "Cloud", self.section == BrowserSection::Sync, theme).on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(ShowSync), cx);
            }))
            .child(components::spacer())
            .child(components::nav_rail_item("section-settings", IconName::Settings, "Settings", self.settings_open, theme).on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(ShowSettings), cx);
            }))
    }

    pub(crate) fn render_book_card(card: BookCard, library_id: String, open_reader: OpenReader, element_id: String, theme: components::BrowserTheme) -> AnyElement {
        let locator = BookLocator::parse(&library_id, card.id).expect("library entries always contain validated ids");
        let title = card.title.to_string();
        let cover = card.cover.clone();
        let progress = card.progress.clone();
        components::book_card(element_id)
            .when_some(cover, |card, cover| card.child(components::book_cover(theme).child(components::book_cover_image(cover))))
            .when(card.cover.is_none(), |card| card.child(components::missing_book_cover(theme).child("No cover")))
            .child(components::book_card_title().child(card.title))
            .child(components::book_card_author(theme).child(card.author))
            .when_some(progress, |card, progress| card.child(components::book_progress(progress, theme)))
            .on_click(move |_, _, cx| {
                open_reader(locator.clone(), title.clone(), cx);
            })
            .into_any_element()
    }

    pub(crate) fn render_book_grid(&self, entity: Entity<Self>, columns: usize, theme: components::BrowserTheme) -> AnyElement {
        self.render_book_grid_cards(entity, self.books.clone(), columns, "browser-book-grid".to_owned(), false, theme)
    }

    pub(crate) fn render_book_grid_cards(&self, entity: Entity<Self>, books: Arc<Vec<BookCard>>, columns: usize, list_id: String, embedded: bool, theme: components::BrowserTheme) -> AnyElement {
        let Some(library_id) = self.active_library_id.clone() else {
            return components::browser_inline_message("No library configured", theme).into_any_element();
        };
        if books.is_empty() {
            return components::browser_inline_message("No books here yet", theme).into_any_element();
        }
        let open_reader = self.open_reader.clone();
        let row_count = books.len().div_ceil(columns);
        let id_prefix = list_id.clone();
        let cover_target = entity;
        let list = uniform_list(list_id, row_count, move |range, _, cx| {
            let mut visible_ids = Vec::new();
            for row in range.clone() {
                let start = row * columns;
                let end = (start + columns).min(books.len());
                visible_ids.extend(books[start..end].iter().filter(|card| card.cover.is_none()).map(|card| card.id));
            }
            if !visible_ids.is_empty() {
                cover_target.update(cx, |this, cx| {
                    this.queue_cover_load(visible_ids, cx);
                });
            }
            range
                .map(|row| {
                    let mut element = components::book_grid_row();
                    let start = row * columns;
                    let end = (start + columns).min(books.len());
                    for card in &books[start..end] {
                        element = element.child(Self::render_book_card(card.clone(), library_id.clone(), open_reader.clone(), format!("{id_prefix}-book-{}", card.id), theme));
                    }
                    element
                })
                .collect::<Vec<_>>()
        });
        if embedded { components::embedded_virtual_list(list, row_count as f32 * components::BOOK_GRID_ROW_HEIGHT).into_any_element() } else { components::virtual_list(list).into_any_element() }
    }

    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.set_section(BrowserSection::Search, cx);
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn on_show_home(&mut self, _: &ShowHome, _: &mut Window, cx: &mut Context<Self>) {
        self.set_section(BrowserSection::Home, cx);
    }

    fn on_show_authors(&mut self, _: &ShowAuthors, _: &mut Window, cx: &mut Context<Self>) {
        self.set_section(BrowserSection::Authors, cx);
    }

    fn on_show_folders(&mut self, _: &ShowFolders, _: &mut Window, cx: &mut Context<Self>) {
        self.set_section(BrowserSection::Folders, cx);
    }

    fn on_show_search(&mut self, _: &ShowSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.set_section(BrowserSection::Search, cx);
        self.search_input.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn on_show_settings(&mut self, _: &ShowSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = true;
        cx.notify();
    }

    fn on_show_sync(&mut self, _: &ShowSync, _: &mut Window, cx: &mut Context<Self>) {
        self.set_section(BrowserSection::Sync, cx);
        self.refresh_cloud(cx);
    }

    fn on_dismiss_settings(&mut self, _: &DismissSettings, _: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            self.settings_open = false;
            cx.notify();
        }
    }

    pub(crate) fn interface_font_name(&self) -> SharedString {
        SharedString::from(self.browsing_preferences.interface_font().font_name())
    }

    pub(crate) fn heading_font_name(&self) -> SharedString {
        SharedString::from(self.browsing_preferences.heading_font().font_name())
    }
}

impl Focusable for BrowserView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = components::browser_theme(cx);
        let section_width = (f32::from(window.viewport_size().width) - components::NAV_RAIL_WIDTH - components::CONTENT_PADDING * 2.0).max(components::BOOK_COVER_WIDTH);
        let book_columns = (((section_width + components::BOOK_GRID_GAP) / (components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP)).floor() as usize).max(1);
        let author_book_width = (section_width - components::AUTHOR_LIST_PANEL_WIDTH - components::CONTENT_PADDING).max(components::BOOK_COVER_WIDTH);
        let author_book_columns = (((author_book_width + components::BOOK_GRID_GAP) / (components::BOOK_COVER_WIDTH + components::BOOK_GRID_GAP)).floor() as usize).max(1);
        let author_filter = self.author_filter_input.read(cx).value().to_string();
        let body = match self.section {
            BrowserSection::Home => self.render_home(theme),
            BrowserSection::Authors => self.render_authors(entity.clone(), author_book_columns, &author_filter, theme),
            BrowserSection::Folders => self.render_folders(entity.clone(), book_columns, theme),
            BrowserSection::Search => {
                let mut page = components::browser_search_page().child(components::browser_section_title("Search", self.heading_font_name())).child(components::browser_search_box().child(Input::new(&self.search_input)));
                if self.search_active {
                    page = page.child(components::browser_section_title("Search results", self.heading_font_name())).child(self.render_book_grid(entity.clone(), book_columns, theme));
                } else {
                    page = page.child(components::browser_inline_message("Type at least 2 characters to search by title or author", theme));
                }
                page.into_any_element()
            }
            BrowserSection::Sync => self.render_cloud(entity.clone(), theme),
            BrowserSection::Settings => self.render_folders(entity.clone(), book_columns, theme),
        };

        let topbar_left = components::browser_topbar_left().child(components::library_switcher_group().child(self.render_library_switcher(entity.clone(), theme)));
        let mut topbar_center = components::browser_topbar_center();
        let mut topbar_right = components::browser_topbar_right();
        if self.section == BrowserSection::Folders {
            topbar_center = topbar_center.child(self.render_folder_breadcrumb(entity.clone()));
            topbar_right = topbar_right
                .child(components::topbar_action_button("add-book", "Add book", IconName::BookOpen, theme).on_click({
                    let target = entity.clone();
                    move |_, window, cx| {
                        target.update(cx, |this, cx| {
                            this.choose_book_to_add(window, cx);
                        });
                    }
                }))
                .child(components::topbar_action_button("new-folder", "New folder", IconName::Plus, theme).on_click({
                    let target = entity.clone();
                    move |_, window, cx| {
                        target.update(cx, |this, cx| {
                            this.show_new_folder_dialog(window, cx);
                        });
                    }
                }));
        }
        let topbar = components::browser_topbar(theme).child(topbar_left).child(topbar_center).child(topbar_right);

        let settings_max_height = (f32::from(window.viewport_size().height) - 44.0).max(280.0);
        components::browser_page(self.interface_font_name(), self.browsing_preferences.ui_font_size_px(), theme)
            .key_context("Browser")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_show_home))
            .on_action(cx.listener(Self::on_show_authors))
            .on_action(cx.listener(Self::on_show_folders))
            .on_action(cx.listener(Self::on_show_search))
            .on_action(cx.listener(Self::on_show_settings))
            .on_action(cx.listener(Self::on_show_sync))
            .on_action(cx.listener(Self::on_dismiss_settings))
            .child(topbar)
            .child(components::browser_body().child(self.render_section_sidebar(theme)).child(components::library_section_content().child(body)))
            .when(self.settings_open, |page| page.child(self.render_browsing_settings(entity, theme, settings_max_height)))
    }
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-f", FocusSearch, Some("Browser")),
        KeyBinding::new("ctrl-1", ShowHome, Some("Browser")),
        KeyBinding::new("ctrl-2", ShowFolders, Some("Browser")),
        KeyBinding::new("ctrl-3", ShowAuthors, Some("Browser")),
        KeyBinding::new("ctrl-4", ShowSearch, Some("Browser")),
        KeyBinding::new("ctrl-5", ShowSettings, Some("Browser")),
        KeyBinding::new("ctrl-6", ShowSync, Some("Browser")),
        KeyBinding::new("escape", DismissSettings, Some("Browser")),
    ]);
}
