//! Creation and folder-opening workflows for the application library catalogue.

use std::path::Path;

use epub_svelte_lib::app::LibraryEntry;
use gpui::prelude::*;
use gpui::{App, AppContext, Context, EventEmitter, PathPromptOptions, SharedString};
use gpui_component::WindowExt;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputState};

use crate::services::AppServices;

#[derive(Clone)]
pub(crate) enum LibrarySetupEvent {
    CatalogueReady { libraries: Vec<LibraryEntry>, activate_library_id: Option<String>, warning: Option<SharedString> },
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
                            created_ids.push(entry.library_id().as_str().to_owned());
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
                    cx.emit(LibrarySetupEvent::CatalogueReady { libraries, activate_library_id: created_ids.last().cloned(), warning: (!errors.is_empty()).then(|| format!("Some folders could not be added: {}", errors.join("; ")).into()) })
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
            Ok::<_, String>((entry.library_id().as_str().to_owned(), libraries))
        });
        cx.spawn(async move |setup, cx| {
            let Ok(result) = operation.await else { return };
            let _ = setup.update(cx, |_, cx| match result {
                Ok((library_id, libraries)) => cx.emit(LibrarySetupEvent::CatalogueReady { libraries, activate_library_id: Some(library_id), warning: None }),
                Err(error) => cx.emit(LibrarySetupEvent::Error(format!("Could not create library: {error}").into())),
            });
        })
        .detach();
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
