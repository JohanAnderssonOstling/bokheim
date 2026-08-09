//! The selected-library chooser rendered at the top of the navigation rail.
//!
//! Owns its trigger and overlay while reading the passive library state.

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription, Window};

use crate::library::{LibraryState, LibraryStateChanged};
use crate::{FocusNextListItem, FocusPreviousListItem};
use bokheim_ui_components as components;

#[derive(Clone)]
pub(crate) enum LibrarySwitcherEvent {
    Select(String),
    Remove(String),
    Create,
    OpenFolder,
}

#[derive(Clone)]
struct LibraryChoice {
    id: String,
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
        let mut choices = self.state.read(cx).entries().iter().map(|library| LibraryChoice { id: library.library_id().as_str().to_owned(), name: library.library_name().to_owned().into() }).collect::<Vec<_>>();
        choices.sort_by_cached_key(|library| library.name.to_lowercase());
        choices
    }

    fn active_library_name(&self, cx: &gpui::App) -> SharedString {
        let state = self.state.read(cx);
        state.entries().iter().find(|library| Some(library.library_id().as_str()) == state.library_id()).map(|library| library.library_name().to_owned().into()).unwrap_or_else(|| "No library".into())
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    fn render_overlay(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = components::browser_theme(cx);
        let emitter = cx.entity();
        let active_library_id = self.state.read(cx).library_id().map(str::to_owned);
        let mut panel = components::library_switcher_panel()
            .key_context("BrowserArrowList")
            .on_action(|_: &FocusPreviousListItem, window, cx| window.focus_prev(cx))
            .on_action(|_: &FocusNextListItem, window, cx| window.focus_next(cx))
            .child(components::library_switcher_title("Libraries", theme));

        for library in self.libraries(cx) {
            let library_id = library.id.clone();
            let selected = active_library_id.as_deref() == Some(library_id.as_str());
            let select = emitter.clone();
            let select_id = library_id.clone();
            panel = panel.child(components::library_switcher_item(format!("library-switcher-{library_id}"), library.name.clone(), selected).on_click(move |_, _, cx| {
                select.update(cx, |switcher, cx| {
                    switcher.open = false;
                    cx.notify();
                    cx.emit(LibrarySwitcherEvent::Select(select_id.clone()));
                });
            }));
        }

        panel = panel.child(components::library_switcher_actions_label("Library actions", theme));
        if let Some(active_library_id) = active_library_id {
            let remove = emitter.clone();
            panel = panel.child(components::library_switcher_action_item("library-switcher-remove", "Delete selected library", gpui_component::IconName::Delete, theme).on_click(move |_, _, cx| {
                remove.update(cx, |switcher, cx| {
                    switcher.open = false;
                    cx.notify();
                    cx.emit(LibrarySwitcherEvent::Remove(active_library_id.clone()));
                });
            }));
        }
        let create = emitter.clone();
        panel = panel.child(components::library_switcher_action_item("library-switcher-create", "Create library…", gpui_component::IconName::Plus, theme).on_click(move |_, _, cx| {
            create.update(cx, |switcher, cx| {
                switcher.open = false;
                cx.notify();
                cx.emit(LibrarySwitcherEvent::Create);
            });
        }));
        let open = emitter.clone();
        panel = panel.child(components::library_switcher_action_item("library-switcher-open", "Open library…", gpui_component::IconName::FolderOpen, theme).on_click(move |_, _, cx| {
            open.update(cx, |switcher, cx| {
                switcher.open = false;
                cx.notify();
                cx.emit(LibrarySwitcherEvent::OpenFolder);
            });
        }));

        gpui::deferred(gpui::anchored().anchor(gpui::Anchor::TopLeft).position(gpui::point(gpui::px(0.0), gpui::px(72.0))).child(components::library_switcher_dialog(theme).child(panel))).with_priority(1).into_any_element()
    }
}

impl Render for LibrarySwitcherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let emitter = cx.entity();
        components::library_switcher_group()
            .child(components::library_switcher_trigger(self.active_library_name(cx)).on_click(move |_, _, cx| {
                emitter.update(cx, |switcher, cx| {
                    switcher.open = true;
                    cx.notify();
                });
            }))
            .when(self.open, |group| group.child(self.render_overlay(cx)))
    }
}
