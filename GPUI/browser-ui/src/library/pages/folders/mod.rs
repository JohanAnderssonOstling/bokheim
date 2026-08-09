//! Directory browsing: subfolders above, books below.
//!
//! Owns the folder path. `load_folders` and `load_folder_books` were near
//! duplicates in the old shell — one loader serves both now that the page owns
//! the path it is loading for.

use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{App, AppContext, Context, EventEmitter, IntoElement, Render, Window};
use gpui_component::WindowExt;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputState};

use crate::library::services::{FolderBrowseRows, LibraryContext};
use crate::library::widgets::BookGrid;
use crate::model::FolderCrumb;
use crate::stores::heading_font;

const ROOT_FOLDER_ID: &str = "__root__";

/// Raised only for operations owned above the selected-library page tree.
#[derive(Clone)]
pub(crate) enum FoldersPageEvent {
    PathChanged(Vec<FolderCrumb>),
}

pub(crate) struct FoldersPage {
    ui: LibraryContext,
    folder_path: Vec<FolderCrumb>,
    directories: Vec<FolderCrumb>,
    books: BookGrid,
    error: Option<gpui::SharedString>,
}

impl EventEmitter<FoldersPageEvent> for FoldersPage {}

impl FoldersPage {
    pub(crate) fn new(ui: LibraryContext, folder_path: Vec<FolderCrumb>, _cx: &mut Context<Self>) -> Self {
        let books = BookGrid::new(ui.clone(), false);
        Self { ui, folder_path, directories: Vec::new(), books, error: None }
    }

    fn folder_id(&self) -> String {
        self.folder_path.last().map(|crumb| crumb.id.clone()).unwrap_or_else(|| ROOT_FOLDER_ID.to_owned())
    }

    pub(crate) fn choose_book_to_add(&mut self, cx: &mut Context<Self>) {
        let folder_id = self.folder_id();
        self.ui.clone().choose_and_import_book(folder_id, cx, |page, result, cx| match result {
            Ok(()) => page.refresh(cx),
            Err(error) => {
                log::error!("failed to add book: {error}");
                page.error = Some(format!("Could not add book: {error}").into());
                cx.notify();
            }
        });
    }

    fn show_new_folder_dialog(&mut self, cx: &mut Context<Self>) {
        let page = cx.entity();
        self.ui.with_window(cx, move |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder name"));
            window.open_dialog(cx, move |dialog, _, _| {
                let input = input.clone();
                let page = page.clone();
                dialog.title("New folder").child(Input::new(&input)).button_props(DialogButtonProps::default().ok_text("Create").show_cancel(true)).on_ok(move |_, _, cx: &mut App| {
                    let name = input.read(cx).value().trim().to_owned();
                    if name.is_empty() {
                        return false;
                    }
                    page.update(cx, |page, cx| page.create_folder(name.clone(), cx));
                    true
                })
            });
        });
    }

    fn create_folder(&mut self, name: String, cx: &mut Context<Self>) {
        let parent_id = self.folder_id();
        let operation = self.ui.create_folder(parent_id, name);
        cx.spawn(async move |page, cx| {
            let result = operation.await;
            let _ = page.update(cx, |page, cx| {
                if let Err(error) = result {
                    page.error = Some(format!("Could not create folder: {error}").into());
                    cx.notify();
                } else {
                    page.refresh(cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn set_folder_path(&mut self, folder_path: Vec<FolderCrumb>, cx: &mut Context<Self>) {
        if self.folder_path == folder_path {
            return;
        }
        self.folder_path = folder_path;
        cx.emit(FoldersPageEvent::PathChanged(self.folder_path.clone()));
        self.refresh(cx);
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let folder_id = self.folder_id();
        match self.ui.library.folder(&folder_id) {
            Ok(data) => {
                self.error = None;
                self.install(data, cx);
            }
            Err(error) => {
                log::error!("failed to load Folders section: {error}");
                self.error = Some(error.to_string().into());
                cx.notify();
            }
        }
    }

    fn install(&mut self, data: FolderBrowseRows, cx: &mut Context<Self>) {
        let directories_changed = self.directories != data.directories;
        self.directories = data.directories;
        self.books.set_books(data.books, cx);
        if directories_changed {
            cx.notify();
        }
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let contracts = &self.ui.contracts();
        let heading_font = heading_font(cx);
        let emitter = cx.entity();
        let actions = components::browser_topbar_right().child(components::topbar_action_button("empty-add-book", contracts.library_actions.add_book_label.clone(), gpui_component::IconName::BookOpen, theme).on_click(move |_, _, cx| {
            emitter.update(cx, |page, cx| page.choose_book_to_add(cx));
        }));
        let (title, description) = if self.folder_path.is_empty() {
            (&contracts.browsing_states.empty_library_title, &contracts.browsing_states.empty_library_description)
        } else {
            (&contracts.browsing_states.empty_folder_title, &contracts.browsing_states.empty_folder_description)
        };
        components::browser_actionable_empty(title.clone(), description.clone(), actions, heading_font, theme).into_any_element()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let actions = self.ui.contracts().library_actions.clone();
        let page = cx.entity();
        let mut breadcrumb = components::folder_breadcrumb();
        if self.folder_path.is_empty() {
            breadcrumb = breadcrumb.child(components::folder_breadcrumb_item("Library", true));
        } else {
            let page = page.clone();
            breadcrumb = breadcrumb.child(components::folder_breadcrumb_item("Library", false).on_click(move |_, _, cx| {
                page.update(cx, |page, cx| page.set_folder_path(Vec::new(), cx));
            }));
        }
        for (index, crumb) in self.folder_path.iter().enumerate() {
            if index + 1 == self.folder_path.len() {
                breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(crumb.name.clone(), true));
            } else {
                let path = self.folder_path[..=index].to_vec();
                let page = page.clone();
                breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(crumb.name.clone(), false).on_click(move |_, _, cx| {
                    page.update(cx, |page, cx| page.set_folder_path(path.clone(), cx));
                }));
            }
        }
        let add_page = page.clone();
        let folder_page = page;
        let right = components::browser_topbar_right()
            .child(components::topbar_action_button("add-book", actions.add_book_label.clone(), gpui_component::IconName::BookOpen, theme).on_click(move |_, _, cx| {
                add_page.update(cx, |page, cx| page.choose_book_to_add(cx));
            }))
            .child(components::topbar_action_button("new-folder", actions.new_folder_label.clone(), gpui_component::IconName::Plus, theme).on_click(move |_, _, cx| {
                folder_page.update(cx, |page, cx| page.show_new_folder_dialog(cx));
            }));
        components::browser_topbar(theme).child(components::browser_topbar_left().child(breadcrumb)).child(right).into_any_element()
    }

    fn render_folder(&self, directory: &FolderCrumb, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let page = cx.entity();
        let directory = directory.clone();
        let label = format!("{} ({})", directory.name, directory.book_count);
        let id = directory.id.clone();
        let open = components::folder_button(format!("folder-button-{id}"), label, theme).on_click(move |_, _, cx| {
            page.update(cx, |page, cx| {
                let mut path = page.folder_path.clone();
                path.push(directory.clone());
                page.set_folder_path(path, cx);
            });
        });
        components::folder_card(open, None::<gpui::AnyElement>).into_any_element()
    }
}

impl Render for FoldersPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let mut dashboard = components::folder_dashboard();
        if self.directories.is_empty() && self.books.is_empty() {
            dashboard = dashboard.child(self.render_empty(cx));
        } else {
            if !self.directories.is_empty() {
                let cards = self.directories.iter().map(|directory| self.render_folder(directory, cx)).collect::<Vec<_>>();
                dashboard = dashboard.child(components::folder_buttons().children(cards));
            }
            dashboard = dashboard.child(self.books.render("folder-books", window));
        }

        let entity = cx.entity();
        let retry_label = self.ui.contracts().browsing_states.retry_label.clone();
        let body = components::browser_error_banner(self.error.clone(), retry_label.into(), theme, dashboard.into_any_element(), move |cx| {
            entity.update(cx, |page, cx| page.refresh(cx));
        });
        components::library_section_content().child(self.render_header(cx)).child(body).into_any_element()
    }
}
