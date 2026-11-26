//! Desktop dropdown and centered mobile overlay for adding a library.

use ui_text::text;

#[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use library_backend::ReaderImportSource;

use gpui::prelude::*;
use gpui::{Anchor, AnyElement, App, ClickEvent, Context, DismissEvent, Entity, FocusHandle, Focusable, KeyDownEvent, MouseButton, Subscription, Window, anchored, deferred, div, px};
use gpui_component::menu::PopupMenu;
use gpui_component::{FocusTrapElement, IconName};
use ui_components as components;
use ui_components::Button;

use super::Sidebar;

#[derive(Clone, Copy)]
enum AddLibraryChoice {
    Create,
    #[cfg(not(target_arch = "arm"))]
    OpenFolder,
    #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    Import(ReaderImportSource),
}

impl AddLibraryChoice {
    const ALL: &[Self] = &[
        Self::Create,
        #[cfg(not(target_arch = "arm"))]
        Self::OpenFolder,
    ];

    #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    const IMPORTS: &[Self] = &[Self::Import(ReaderImportSource::Readest), Self::Import(ReaderImportSource::Foliate)];

    fn label(self) -> &'static str {
        match self {
            Self::Create => text!(browse::create_new_library),
            #[cfg(not(target_arch = "arm"))]
            Self::OpenFolder => text!(browse::open_library_from_folder),
            #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            Self::Import(source) => source.name(),
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Create => IconName::Plus,
            #[cfg(not(target_arch = "arm"))]
            Self::OpenFolder => IconName::FolderOpen,
            #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            Self::Import(_) => IconName::BookOpen,
        }
    }
}

pub(super) struct AddLibraryChoices {
    choices: &'static [AddLibraryChoice],
    import: bool,
    menu: Option<(Entity<PopupMenu>, Subscription)>,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    selected: usize,
}

impl Sidebar {
    pub(super) fn open_add_library_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_library_choices(AddLibraryChoice::ALL, false, window, cx);
    }

    #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    pub(super) fn open_import_library_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_library_choices(AddLibraryChoice::IMPORTS, true, window, cx);
    }

    fn open_library_choices(&mut self, choices: &'static [AddLibraryChoice], import: bool, window: &mut Window, cx: &mut Context<Self>) {
        let previous_focus = window.focused(cx);
        let focus = cx.focus_handle();
        let menu = if cx.supports_touch_input() || components::uses_mobile_navigation(window) {
            focus.focus(window, cx);
            None
        } else {
            let sidebar = cx.entity();
            let menu_choices = choices;
            let menu = PopupMenu::build(window, cx, move |mut menu, _, _| {
                for &choice in menu_choices {
                    let sidebar = sidebar.clone();
                    menu = menu.item(components::menu_item_with_icon(choice.label(), choice.icon(), move |_, window, cx| {
                        sidebar.update(cx, |sidebar, cx| sidebar.add_library(choice, window, cx));
                    }));
                }
                menu
            });
            let subscription = cx.subscribe_in(&menu, window, |sidebar, _, _: &DismissEvent, _, cx| {
                sidebar.add_library_choices = None;
                cx.notify();
            });
            menu.focus_handle(cx).focus(window, cx);
            Some((menu, subscription))
        };
        self.add_library_choices = Some(AddLibraryChoices { choices, import, menu, focus, previous_focus, selected: 0 });
        cx.notify();
    }

    pub(super) fn close_add_library_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(choices) = self.add_library_choices.take() {
            if let Some(focus) = choices.previous_focus {
                focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    fn add_library(&mut self, choice: AddLibraryChoice, window: &mut Window, cx: &mut Context<Self>) {
        self.close_add_library_choices(window, cx);

        self.switcher_open = false;
        match choice {
            AddLibraryChoice::Create => self.open_create_library_dialog(window, cx),
            #[cfg(not(target_arch = "arm"))]
            AddLibraryChoice::OpenFolder => self.choose_library_folders(cx),
            #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            AddLibraryChoice::Import(source) => {
                let sidebar = cx.entity();
                crate::services::after_menu_dismiss(window, cx, move |window, cx| sidebar.update(cx, |sidebar, cx| sidebar.open_reader_library_import(source, window, cx)));
            }
        }
        cx.notify();
    }

    pub(crate) fn render_add_library_control(&self, emitter: Entity<Self>, button: Button, in_switcher: bool) -> AnyElement {
        self.render_library_choice_control(emitter, button, in_switcher, false)
    }

    #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    pub(super) fn render_import_library_control(&self, emitter: Entity<Self>, button: Button) -> AnyElement {
        self.render_library_choice_control(emitter, button, true, true)
    }

    fn render_library_choice_control(&self, emitter: Entity<Self>, button: Button, in_switcher: bool, import: bool) -> AnyElement {
        let was_open = self.add_library_choices.as_ref().is_some_and(|choices| choices.import == import);
        let activate = move |window: &mut Window, cx: &mut App| {
            emitter.update(cx, |sidebar, cx| {
                if was_open {
                    sidebar.close_add_library_choices(window, cx);
                } else {
                    #[cfg(all(not(feature = "kobo"), any(target_os = "linux", target_os = "macos", target_os = "windows")))]
                    if import {
                        sidebar.switcher_cursor = super::SwitcherCursor::Import;
                        sidebar.open_import_library_choices(window, cx);
                        return;
                    }
                    sidebar.switcher_cursor = super::SwitcherCursor::Add;
                    sidebar.open_add_library_choices(window, cx);
                }
            });
        };
        let pointer_activate = activate.clone();
        div()
            .flex_none()
            .child(
                button
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        pointer_activate(window, cx);
                    })
                    .on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        if matches!(event, ClickEvent::Keyboard(_)) {
                            activate(window, cx);
                        }
                    }),
            )
            .children(
                self.add_library_choices
                    .as_ref()
                    .filter(|choices| self.switcher_open == in_switcher && choices.import == import)
                    .and_then(|choices| choices.menu.as_ref())
                    .map(|(menu, _)| deferred(anchored().anchor(Anchor::TopRight).snap_to_window_with_margin(px(8.0)).child(menu.clone())).with_priority(3)),
            )
            .into_any_element()
    }

    fn add_library_overlay_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choices) = self.add_library_choices.as_mut() else { return };
        match event.keystroke.key.as_str() {
            "up" => choices.selected = choices.selected.saturating_sub(1),
            "down" => choices.selected = (choices.selected + 1).min(choices.choices.len() - 1),
            "enter" | "space" if !choices.focus.is_focused(window) => return,
            "enter" | "space" => {
                let choice = choices.choices[choices.selected];
                self.add_library(choice, window, cx);
            }
            "escape" => self.close_add_library_choices(window, cx),
            _ => return,
        }
        if let Some(choices) = &self.add_library_choices {
            choices.focus.focus(window, cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_add_library_overlay(&self, emitter: Entity<Self>, window: &Window, cx: &App) -> Option<AnyElement> {
        let choices = self.add_library_choices.as_ref().filter(|choices| choices.menu.is_none())?;
        let theme = components::browser_theme(cx);
        let close = emitter.clone();
        let header = components::library_switcher_header(if choices.import { text!(common::import) } else { text!(common::add_library) }, theme).child(
            components::outlined_icon_button("add-library-overlay-close", text!(common::close), IconName::Close).on_click(move |_, window, cx| {
                close.update(cx, |sidebar, cx| sidebar.close_add_library_choices(window, cx));
            }),
        );
        let keys = emitter.clone();
        let mut panel = components::modal_form_surface(theme)
            .id("add-library-overlay")
            .debug_selector(|| "add-library-overlay".into())
            .w(gpui::rems(22.5))
            .gap(px(components::SPACE_XS))
            .track_focus(&choices.focus)
            .on_key_down(move |event, window, cx| keys.update(cx, |sidebar, cx| sidebar.add_library_overlay_key(event, window, cx)))
            .child(header);
        for (index, &choice) in choices.choices.iter().enumerate() {
            let select = emitter.clone();
            panel = panel.child(components::library_switcher_item(("add-library-choice", index), choice.label(), false, index == choices.selected, theme).flex_none().w_full().min_h(gpui::rems(components::TOUCH_TARGET_REM)).on_click(
                move |_, window, cx| {
                    cx.stop_propagation();
                    select.update(cx, |sidebar, cx| sidebar.add_library(choice, window, cx));
                },
            ));
        }
        Some(
            deferred(components::anchor_to_window(
                components::modal_overlay()
                    .child(components::modal_scrim("add-library-overlay-scrim", theme).on_click(move |_, window, cx| {
                        emitter.update(cx, |sidebar, cx| sidebar.close_add_library_choices(window, cx));
                    }))
                    .child(panel.focus_trap("add-library-overlay", &choices.focus)),
                window,
            ))
            .with_priority(4)
            .into_any_element(),
        )
    }
}
