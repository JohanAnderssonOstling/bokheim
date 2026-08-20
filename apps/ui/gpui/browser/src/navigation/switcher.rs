//! The selected-library chooser rendered at the top of the navigation rail.
//!
//! `NavigationView` owns the open state and renders both the trigger and the
//! animation-free centered overlay while reading passive library state.

use gpui::prelude::*;
use gpui::{Entity, IntoElement, MouseButton, PromptLevel, SharedString};
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
    fn libraries(&self, cx: &gpui::App) -> Vec<LibraryChoice> {
        let mut choices = self.library_state.read(cx).entries().iter().map(|library| LibraryChoice { id: *library.library_id(), name: library.library_name().to_owned().into() }).collect::<Vec<_>>();
        choices.sort_by_cached_key(|library| library.name.to_lowercase());
        choices
    }

    fn active_library_name(&self, cx: &gpui::App) -> SharedString {
        let state = self.library_state.read(cx);
        state.entries().iter().find(|library| state.library_id().is_some_and(|id| library.library_id() == id)).map(|library| library.library_name().to_owned().into()).unwrap_or_else(|| "No library".into())
    }

    pub(super) fn render_library_switcher_overlay(&self, emitter: Entity<Self>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let active_library_id = self.library_state.read(cx).library_id().copied();
        let libraries = self.libraries(cx);
        let add = emitter.clone();
        let mut panel =
            components::library_switcher_panel().key_context("BrowserArrowList").on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx)).on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx)).child(
                components::library_switcher_header("Libraries", theme).child(components::library_switcher_add_button(theme).on_click(move |_, _, cx| {
                    add.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.emit(NavEvent::LibrarySwitcherVisibilityChanged);
                        cx.emit(NavEvent::OpenLibrary);
                    });
                })),
            );

        for library in libraries {
            let library_id = library.id.clone();
            let selected = active_library_id == Some(library_id);
            let select = emitter.clone();
            let select_id = library_id.clone();
            let remove = emitter.clone();
            let remove_id = library_id.clone();
            panel = panel.child(components::library_switcher_row(
                components::library_switcher_item(format!("library-switcher-{library_id}"), library.name.clone(), selected).on_click(move |_, _, cx| {
                    select.update(cx, |navigation, cx| {
                        navigation.switcher_open = false;
                        cx.emit(NavEvent::LibrarySwitcherVisibilityChanged);
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
                        cx.emit(NavEvent::LibrarySwitcherVisibilityChanged);
                        let backend = navigation.backend.clone();
                        let library_id = remove_id.clone();
                        cx.spawn(async move |navigation, cx| {
                            if answer.await.ok() != Some(0) {
                                return;
                            }
                            let deletion = backend.spawn(move |core| async move { core.delete_library(&library_id).await.map_err(|error| error.to_string()) });
                            match deletion.await.unwrap_or_else(|error| Err(error.to_string())) {
                                Ok(()) => {
                                    let _ = navigation.update(cx, |_, cx| cx.emit(NavEvent::LibraryRemoved));
                                }
                                Err(error) => {
                                    let _ = navigation.update_in(cx, |_, window, cx| {
                                        window.push_notification(Notification::error(error).id1::<NavigationView>("library-removal-error"), cx);
                                    });
                                }
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
                    cx.emit(NavEvent::LibrarySwitcherVisibilityChanged);
                }
            });
        }))
    }
}
