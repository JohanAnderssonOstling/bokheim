//! The selected-library chooser rendered at the top of the navigation rail.
//!
//! `NavigationView` owns the open state and renders both the trigger and the
//! animation-free centered overlay while reading passive library state.

use std::path::Path;

use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, MouseButton, PathPromptOptions, PromptLevel, SharedString};
use gpui_component::WindowExt;
use gpui_component::notification::Notification;

use crate::{FocusNextListItem, FocusPreviousListItem};
use epub_svelte_lib::LibraryId;
use ui_components as components;

use super::{NavEvent, NavigationView};

#[derive(Clone)]
struct LibraryChoice {
    id: LibraryId,
    name: SharedString,
}

impl NavigationView {
    pub(crate) fn library_switcher_overlay(&self, emitter: Entity<Self>, cx: &gpui::App) -> Option<gpui::AnyElement> {
        self.switcher_open.then(|| self.render_library_switcher_overlay(emitter, cx))
    }

    fn choose_library_folders(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Choose library folders".into()) });
        let backend = self.backend.clone();
        cx.spawn(async move |navigation, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            if paths.is_empty() {
                return;
            }
            let operation = backend.spawn(move |core| async move {
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
                    match core.create_library(library_name_for_folder(&path), Some(path)) {
                        Ok(entry) => {
                            known_paths.push(entry.library_path().to_path_buf());
                            created_ids.push(*entry.library_id());
                        }
                        Err(error) => errors.push(format!("{display_path}: {error}")),
                    }
                }
                Ok::<_, String>((created_ids, errors))
            });
            let result = operation.await.unwrap_or_else(|error| Err(error.to_string()));
            let _ = navigation.update_in(cx, |_, window, cx| match result {
                Ok((created_ids, errors)) => {
                    if !errors.is_empty() {
                        window.push_notification(Notification::warning(format!("Some folders could not be added: {}", errors.join("; "))).id1::<NavigationView>("library-add-warning"), cx);
                    }
                    if let Some(library_id) = created_ids.last() {
                        cx.emit(NavEvent::SelectLibrary(*library_id));
                    }
                }
                Err(error) => {
                    window.push_notification(Notification::error(format!("Could not add library folders: {error}")).id1::<NavigationView>("library-add-error").autohide(false), cx);
                }
            });
        })
        .detach();
    }

    fn libraries(&self, cx: &gpui::App) -> Vec<LibraryChoice> {
        let mut choices = self.library_state.read(cx).entries().iter().map(|library| LibraryChoice { id: *library.library_id(), name: library.library_name().to_owned().into() }).collect::<Vec<_>>();
        choices.sort_by_cached_key(|library| library.name.to_lowercase());
        choices
    }

    fn active_library_name(&self, cx: &gpui::App) -> SharedString {
        let state = self.library_state.read(cx);
        state.entries().iter().find(|library| library.library_id() == state.library_id()).map(|library| library.library_name().to_owned().into()).unwrap_or_else(|| "Library".into())
    }

    fn render_library_switcher_overlay(&self, emitter: Entity<Self>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let active_library_id = *self.library_state.read(cx).library_id();
        let libraries = self.libraries(cx);
        let add = emitter.clone();
        let mut panel =
            components::library_switcher_panel().key_context("BrowserArrowList").on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx)).on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx)).child(
                components::library_switcher_header("Libraries", theme).child(components::library_switcher_add_button(theme).on_click(move |_, _, cx| {
                    add.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.notify();
                        navigation.choose_library_folders(cx);
                    });
                })),
            );

        for library in libraries {
            let library_id = library.id.clone();
            let selected = active_library_id == library_id;
            let select = emitter.clone();
            let select_id = library_id.clone();
            let remove = emitter.clone();
            let remove_id = library_id.clone();
            panel = panel.child(components::library_switcher_row(
                components::library_switcher_item(format!("library-switcher-{library_id}"), library.name.clone(), selected).on_click(move |_, _, cx| {
                    select.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.notify();
                        cx.emit(NavEvent::SelectLibrary(select_id.clone()));
                    });
                }),
                components::library_switcher_delete_button(format!("library-switcher-delete-{library_id}"), library.name, theme).on_click(move |_, window, cx| {
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        "Delete this library?",
                        Some("Local database and state are removed immediately. Server deletion happens now when possible, or automatically after the next sign-in. Local files and stored server blobs are kept."),
                        &["Delete library", "Cancel"],
                        cx,
                    );
                    remove.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.notify();
                        let backend = navigation.backend.clone();
                        let library_id = remove_id.clone();
                        cx.spawn(async move |navigation, cx| {
                            if answer.await.ok() != Some(0) {
                                return;
                            }
                            let deletion = backend.spawn(move |core| async move { core.delete_library(&library_id).await.map_err(|error| error.to_string()) });
                            if let Err(error) = deletion.await.unwrap_or_else(|error| Err(error.to_string())) {
                                let _ = navigation.update_in(cx, |_, window, cx| {
                                    window.push_notification(Notification::error(error).id1::<NavigationView>("library-removal-error"), cx);
                                });
                            }
                        })
                        .detach();
                    });
                }),
            ));
        }

        let dismiss = emitter;
        gpui::deferred(
            components::library_switcher_overlay()
                .child(components::library_switcher_scrim(theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss.update(cx, |navigation, cx| navigation.close_library_switcher(cx));
                }))
                .child(components::library_switcher_dialog(theme).child(panel)),
        )
        .with_priority(1)
        .into_any_element()
    }
    pub(super) fn render_library_switcher_trigger(&self, emitter: Entity<Self>, cx: &gpui::App) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        components::library_switcher_group(theme).child(components::library_switcher_trigger(self.active_library_name(cx), theme).on_click(move |_, _, cx| {
            emitter.update(cx, |navigation, cx| {
                if !navigation.switcher_open {
                    navigation.switcher_open = true;
                    cx.notify();
                }
            });
        }))
    }
}

fn library_name_for_folder(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| path.display().to_string())
}
