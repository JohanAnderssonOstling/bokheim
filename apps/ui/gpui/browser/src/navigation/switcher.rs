//! The selected-library chooser rendered at the top of the navigation rail.
//!
//! Owns its trigger and animation-free centered overlay while reading the passive library state.

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, IntoElement, MouseButton, Render, SharedString, Subscription, Window};

use crate::library::{LibraryState, LibraryStateChanged};
use crate::{FocusNextListItem, FocusPreviousListItem};
use epub_svelte_lib::LibraryId;
use ui_components as components;

#[derive(Clone)]
pub(crate) enum LibrarySwitcherEvent {
    Select(LibraryId),
    Remove(LibraryId),
    OpenFolder,
    VisibilityChanged,
}

#[derive(Clone)]
struct LibraryChoice {
    id: LibraryId,
    name: SharedString,
}

pub(crate) struct LibrarySwitcherView {
    state: Entity<LibraryState>,
    open: bool,
    _state_subscription: Subscription,
}

impl EventEmitter<LibrarySwitcherEvent> for LibrarySwitcherView {}

impl LibrarySwitcherView {
    pub(crate) fn new(state: Entity<LibraryState>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&state, |_: &mut Self, _, _: &LibraryStateChanged, cx| cx.notify());
        Self { state, open: false, _state_subscription: subscription }
    }

    fn libraries(&self, cx: &gpui::App) -> Vec<LibraryChoice> {
        let mut choices = self.state.read(cx).entries().iter().map(|library| LibraryChoice { id: *library.library_id(), name: library.library_name().to_owned().into() }).collect::<Vec<_>>();
        choices.sort_by_cached_key(|library| library.name.to_lowercase());
        choices
    }

    fn active_library_name(&self, cx: &gpui::App) -> SharedString {
        let state = self.state.read(cx);
        state.entries().iter().find(|library| state.library_id().is_some_and(|id| library.library_id() == id)).map(|library| library.library_name().to_owned().into()).unwrap_or_else(|| "No library".into())
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        cx.notify();
        cx.emit(LibrarySwitcherEvent::VisibilityChanged);
    }

    pub(crate) fn render_overlay(&self, emitter: Entity<Self>, cx: &gpui::App) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let active_library_id = self.state.read(cx).library_id().copied();
        let libraries = self.libraries(cx);
        let add = emitter.clone();
        let mut panel =
            components::library_switcher_panel().key_context("BrowserArrowList").on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx)).on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx)).child(
                components::library_switcher_header("Libraries", theme).child(components::library_switcher_add_button(theme).on_click(move |_, _, cx| {
                    add.update(cx, |switcher, cx| {
                        switcher.open = false;
                        cx.emit(LibrarySwitcherEvent::VisibilityChanged);
                        cx.emit(LibrarySwitcherEvent::OpenFolder);
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
                    select.update(cx, |switcher, cx| {
                        switcher.open = false;
                        cx.emit(LibrarySwitcherEvent::VisibilityChanged);
                        cx.emit(LibrarySwitcherEvent::Select(select_id.clone()));
                    });
                }),
                components::library_switcher_delete_button(format!("library-switcher-delete-{library_id}"), library.name, theme).on_click(move |_, _, cx| {
                    remove.update(cx, |switcher, cx| {
                        switcher.open = false;
                        cx.emit(LibrarySwitcherEvent::VisibilityChanged);
                        cx.emit(LibrarySwitcherEvent::Remove(remove_id.clone()));
                    });
                }),
            ));
        }

        let dismiss = emitter;
        gpui::deferred(
            components::library_switcher_overlay()
                .child(components::library_switcher_scrim(theme).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    dismiss.update(cx, |switcher, cx| switcher.close(cx));
                }))
                .child(components::library_switcher_dialog(theme).child(panel)),
        )
        .with_priority(1)
        .into_any_element()
    }
}

impl Render for LibrarySwitcherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let emitter = cx.entity();
        components::library_switcher_group().child(components::library_switcher_trigger(self.active_library_name(cx)).on_click(move |_, _, cx| {
            emitter.update(cx, |switcher, cx| {
                if !switcher.open {
                    switcher.open = true;
                    cx.notify();
                    cx.emit(LibrarySwitcherEvent::VisibilityChanged);
                }
            });
        }))
    }
}
