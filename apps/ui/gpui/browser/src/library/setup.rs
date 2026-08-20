//! Creation and folder-opening workflows for the configured library list.

use std::path::Path;

use epub_svelte_lib::LibraryId;
use epub_svelte_lib::app::LibraryEntry;
use gpui::prelude::*;
use gpui::{App, AppContext, Context, EventEmitter, IntoElement, PathPromptOptions, Render, SharedString, Window, px};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputState};
use gpui_component::{IconName, WindowExt};

use crate::services::AppServices;
use crate::stores::interface_font;
use ui_components as components;

#[derive(Clone)]
pub(crate) enum LibrarySetupEvent {
    LibrariesAdded { libraries: Vec<LibraryEntry>, activate_library_id: Option<LibraryId>, warning: Option<SharedString> },
    OpenSettings,
    OpenAccount,
    Error(SharedString),
}

pub(crate) struct LibrarySetup {
    services: AppServices,
}

impl EventEmitter<LibrarySetupEvent> for LibrarySetup {}

impl LibrarySetup {
    pub(crate) fn new(services: AppServices) -> Self {
        Self { services }
    }

    pub(crate) fn show_create_dialog(&mut self, cx: &mut Context<Self>) {
        let setup = cx.entity();
        self.services.with_window(cx, move |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Library name"));
            window.open_dialog(cx, move |dialog, _, _| {
                let input = input.clone();
                let setup = setup.clone();
                dialog.title("New library").child(Input::new(&input)).button_props(DialogButtonProps::default().ok_text("Create").show_cancel(true)).on_ok(move |_, _, cx: &mut App| {
                    let name = input.read(cx).value().trim().to_owned();
                    if name.is_empty() {
                        return false;
                    }
                    setup.update(cx, |setup, cx| setup.create_library(name.clone(), cx));
                    true
                })
            });
        });
    }

    pub(crate) fn choose_library_folders(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Choose library folders".into()) });
        let core = self.services.backend.clone();
        cx.spawn(async move |setup, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            if paths.is_empty() {
                return;
            }
            let operation = core.spawn(move |core| async move {
                let mut known_paths = core.libraries().map_err(|error| error.to_string())?.into_iter().map(|entry| entry.library_path().to_path_buf()).collect::<Vec<_>>();
                let mut created_ids = Vec::new();
                let mut errors = Vec::new();
                for path in paths {
                    let display_path = path.display().to_string();
                    if !path.is_dir() {
                        errors.push(format!("{display_path}: not a folder"));
                        continue;
                    }
                    let canonical_path = match path.canonicalize() {
                        Ok(path) => path,
                        Err(error) => {
                            errors.push(format!("{display_path}: {error}"));
                            continue;
                        }
                    };
                    if known_paths.contains(&canonical_path) {
                        continue;
                    }
                    let name = library_name_for_folder(&path);
                    match core.create_library(name, Some(path)) {
                        Ok(entry) => {
                            known_paths.push(entry.library_path().to_path_buf());
                            created_ids.push(*entry.library_id());
                        }
                        Err(error) => errors.push(format!("{display_path}: {error}")),
                    }
                }
                let libraries = core.libraries().map_err(|error| error.to_string())?;
                Ok::<_, String>((created_ids, libraries, errors))
            });
            let Ok(result) = operation.await else { return };
            let _ = setup.update(cx, |_, cx| match result {
                Ok((created_ids, libraries, errors)) => {
                    cx.emit(LibrarySetupEvent::LibrariesAdded { libraries, activate_library_id: created_ids.last().cloned(), warning: (!errors.is_empty()).then(|| format!("Some folders could not be added: {}", errors.join("; ")).into()) })
                }
                Err(error) => cx.emit(LibrarySetupEvent::Error(format!("Could not add library folders: {error}").into())),
            });
        })
        .detach();
    }

    fn create_library(&mut self, name: String, cx: &mut Context<Self>) {
        let core = self.services.backend.clone();
        let operation = core.spawn(move |core| async move {
            let entry = core.create_library(name, None).map_err(|error| error.to_string())?;
            let libraries = core.libraries().map_err(|error| error.to_string())?;
            Ok::<_, String>((*entry.library_id(), libraries))
        });
        cx.spawn(async move |setup, cx| {
            let Ok(result) = operation.await else { return };
            let _ = setup.update(cx, |_, cx| match result {
                Ok((library_id, libraries)) => cx.emit(LibrarySetupEvent::LibrariesAdded { libraries, activate_library_id: Some(library_id), warning: None }),
                Err(error) => cx.emit(LibrarySetupEvent::Error(format!("Could not create library: {error}").into())),
            });
        })
        .detach();
    }
}

impl Render for LibrarySetup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let states = &self.services.contracts.browsing_states;
        let setup = cx.entity();
        let create = setup.clone();
        let open = setup.clone();
        let settings = setup.clone();
        let account = setup;
        let primary_actions = components::action_row()
            .child(components::primary_action_button("first-run-create", states.create_library_label.clone(), IconName::Plus, false).on_click(move |_, _, cx| {
                create.update(cx, |setup, cx| setup.show_create_dialog(cx));
            }))
            .child(components::primary_action_button("first-run-open", "Open folder…", IconName::FolderOpen, false).on_click(move |_, _, cx| {
                open.update(cx, |setup, cx| setup.choose_library_folders(cx));
            }));
        let secondary_actions = components::action_row()
            .child(components::secondary_action_button("first-run-settings", "Settings", IconName::Settings, false).on_click(move |_, _, cx| {
                settings.update(cx, |_, cx| cx.emit(LibrarySetupEvent::OpenSettings));
            }))
            .child(components::secondary_action_button("first-run-sync", "Sign in", IconName::User, false).on_click(move |_, _, cx| {
                account.update(cx, |_, cx| cx.emit(LibrarySetupEvent::OpenAccount));
            }));

        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(18.0))
            .bg(theme.page_bg)
            .child(gpui::div().text_size(gpui::rems(28.0 / 16.0)).font_family(interface_font(cx)).font_weight(gpui::FontWeight::BOLD).text_color(theme.text).child(states.no_libraries_title.clone()))
            .child(gpui::div().max_w(px(420.0)).text_align(gpui::TextAlign::Center).text_color(theme.text_muted).child(states.no_libraries_description.clone()))
            .child(primary_actions)
            .child(secondary_actions)
    }
}

fn library_name_for_folder(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_name_falls_back_to_the_full_path_when_there_is_no_file_name() {
        assert_eq!(library_name_for_folder(Path::new("/books/fiction")), "fiction");
        assert_eq!(library_name_for_folder(Path::new("/")), "/");
    }
}
