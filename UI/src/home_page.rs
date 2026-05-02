use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::components::labels::{create_header_label, create_label_background, create_padded_label};
use crate::components::stacks::{create_button_group, create_centered_stack, create_full_stack, create_list, create_list_bodyr, create_list_header, create_top_row};
use crate::components::{add_library_button, create_danger_button, create_list_item_button};
use crate::functions::add_library_on_click;
use crate::library_page::LibraryState;
use crate::page_navigation::{LibraryID, Signals};
use epub_svelte_lib::home::{get_library_entries, remove_library, LibraryEntry};
use epub_svelte_lib::library::Library;
use floem::event::EventPropagation;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::peniko::Color;
use floem::prelude::{button, create_rw_signal, dyn_view, label, stack_from_iter, Decorators, RwSignal, SignalGet, SignalUpdate};
use floem::reactive::create_effect;
use floem::{IntoView, View};

#[derive(Clone, Eq, PartialEq)]
pub struct HomePage {
    pub active_library: Option<LibraryID>,
    pub library_states: HashMap<LibraryID, LibraryState>,
}

impl HomePage {
    pub fn home_view(&mut self, signals: Signals) -> impl View {
        let initial_entries = get_library_entries();
        self.active_library = initial_entries.first().map(|library| library.library_id.clone());
        let library_entries_signal = create_rw_signal(initial_entries);

        if let Some(first) = library_entries_signal.get_untracked().first() {
            if signals.active_library_name.get_untracked().is_empty() {
                signals.active_library_name.set(first.library_name.clone());
            }
            if signals.active_library_id.get_untracked().is_none() {
                signals.active_library_id.set(Some(first.library_id.clone()));
            }
        }


        let show_switcher = create_rw_signal(false);
        let selected_switcher_index = create_rw_signal(active_library_index(&signals, library_entries_signal).unwrap_or(0));

        let top_bar = create_top_bar(signals.clone(), show_switcher, selected_switcher_index, library_entries_signal);
        let switcher_overlay = create_switcher_overlay(signals.clone(), show_switcher, selected_switcher_index, library_entries_signal);

        let signals_for_content = signals.clone();
        let entries_for_content = library_entries_signal;
        let content_view = dyn_view(move || {
            let entries = entries_for_content.get();
            match entries.is_empty() {
                true => Box::new(create_padded_label("Add a library to get started".to_string())) as Box<dyn View>,
                false => create_content_view(signals_for_content.clone(), entries_for_content),
            }
        });

        let content = create_full_stack((content_view,));
        let signals_for_enter = signals.clone();
        let entries_for_down = library_entries_signal;
        let entries_for_up = library_entries_signal;
        let entries_for_enter = library_entries_signal;
        let selected_for_down = selected_switcher_index;
        let selected_for_up = selected_switcher_index;
        let selected_for_enter = selected_switcher_index;
        let home_root = create_full_stack((top_bar, content, switcher_overlay))
            .on_key_down(
                Key::Character("l".into()),
                |mods: Modifiers| mods.control(),
                move |_| {
                    toggle_switcher(show_switcher, selected_switcher_index, signals.clone(), library_entries_signal);
                },
            )
            .on_key_down(
                Key::Named(NamedKey::ArrowDown),
                move |_mods: Modifiers| show_switcher.get(),
                move |_| {
                    move_selection(selected_for_down, entries_for_down, 1);
                },
            )
            .on_key_down(
                Key::Named(NamedKey::ArrowUp),
                move |_mods: Modifiers| show_switcher.get(),
                move |_| {
                    move_selection(selected_for_up, entries_for_up, -1);
                },
            )
            .on_key_down(
                Key::Named(NamedKey::Enter),
                move |_mods: Modifiers| show_switcher.get(),
                move |_| {
                    switch_to_index(signals_for_enter.clone(), entries_for_enter, selected_for_enter.get(), show_switcher);
                },
            )
            .on_key_down(
                Key::Named(NamedKey::Escape),
                move |_mods: Modifiers| show_switcher.get(),
                move |_| {
                    show_switcher.set(false);
                },
            );

        let home_root_id = home_root.id();
        let home_root = home_root.on_click(move |_| {
            home_root_id.request_focus();
            EventPropagation::Continue
        });
        let home_root_id = home_root.id();
        create_effect(move |_| {
            if !show_switcher.get() {
                home_root_id.request_focus();
            }
        });

        home_root
    }
}

fn create_content_view(signals: Signals, library_entries_signal: RwSignal<Vec<LibraryEntry>>) -> Box<dyn View> {
    let entries = library_entries_signal.get();

    let mut handles = signals.library_handles.get_untracked();
    let mut changed = false;
    for entry in &entries {
        if !handles.contains_key(&entry.library_id) {
            let (library, events) = Library::new(&entry.library_path, &entry.library_id);
            let event_counter = signals.library_event_counter.clone();
            std::thread::spawn(move || {
                while let Ok(_event) = events.recv() {
                    event_counter.fetch_add(1, Ordering::Relaxed);
                }
            });
            handles.insert(entry.library_id.clone(), Rc::new(library));
            changed = true;
        }
    }
    if changed {
        signals.library_handles.set(handles);
    }

    let active_signal = signals.active_library_id;
    let views = stack_from_iter(entries.into_iter().map(|entry| {
        let lid = entry.library_id.clone();
        let library_view = LibraryState { library_id: lid.clone() }.library_view(signals.clone());
        create_active_library_view(library_view, move || {
            let active = active_signal.get();
            active.as_deref() == Some(lid.as_str())
        })
    }))
    .style(|s| s.width_full().height_full().flex_grow(1.0));

    Box::new(views) as Box<dyn View>
}

fn create_top_bar(signals: Signals, show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>, library_entries_signal: RwSignal<Vec<LibraryEntry>>) -> impl View {
    let switcher_button = button(label(move || signals.active_library_name.get())).on_click(move |_| {
        toggle_switcher(show_switcher, selected_switcher_index, signals.clone(), library_entries_signal);
        EventPropagation::Continue
    });
    let left_group = create_button_group((switcher_button,));
    create_top_row(left_group)
}

fn create_switcher_item(index: usize, library_entry: LibraryEntry, signals: Signals, show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>, library_entries_signal: RwSignal<Vec<LibraryEntry>>) -> impl IntoView {
    let name = library_entry.library_name;
    let library_path = library_entry.library_path;
    let select_button = create_switcher_select_button(name, move || selected_switcher_index.get() == index).on_click(move |_| {
        switch_to_index(signals.clone(), library_entries_signal, index, show_switcher);
        EventPropagation::Continue
    });
    let delete_button = create_danger_button("Delete").on_click(on_delete_library(library_path, library_entries_signal));
    create_button_group((select_button, delete_button))
}

fn create_switcher_overlay(signals: Signals, show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>, library_entries_signal: RwSignal<Vec<LibraryEntry>>) -> impl View {
    dyn_view(move || {
        let library_entries = library_entries_signal.get();
        let list = if library_entries.is_empty() {
            Box::new(create_label_background("No libraries available".to_string())) as Box<dyn View>
        } else {
            let iter = library_entries.into_iter().enumerate().map(|(idx, library_entry)| create_switcher_item(idx, library_entry, signals.clone(), show_switcher, selected_switcher_index, library_entries_signal));
            Box::new(create_list_bodyr(iter)) as Box<dyn View>
        };

        let add_button = add_library_button().on_click(add_library_on_click(library_entries_signal.write_only()));
        let header = create_list_header((create_header_label("Switch library".to_string()), add_button));
        let panel = create_list((header, list)).on_click(|_| EventPropagation::Stop);

        Box::new(create_switcher_overlay_container(panel, show_switcher).on_click(move |_| {
            show_switcher.set(false);
            EventPropagation::Stop
        })) as Box<dyn View>
    })
    .style(move |s| {
        if show_switcher.get() {
            s.absolute().inset(0).size_full().z_index(100)
        } else {
            s.hide()
        }
    })
}

fn on_delete_library(library_path: String, library_entries_signal: RwSignal<Vec<LibraryEntry>>) -> impl FnMut(&floem::event::Event) -> EventPropagation {
    move |_| {
        remove_library(&library_path);
        library_entries_signal.set(get_library_entries());
        EventPropagation::Stop
    }
}

fn toggle_switcher(show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>, signals: Signals, library_entries_signal: RwSignal<Vec<LibraryEntry>>) {
    if show_switcher.get_untracked() {
        show_switcher.set(false);
    } else {
        selected_switcher_index.set(active_library_index(&signals, library_entries_signal).unwrap_or(0));
        show_switcher.set(true);
    }
}

fn move_selection(selected_switcher_index: RwSignal<usize>, library_entries_signal: RwSignal<Vec<LibraryEntry>>, delta: i32) {
    let len = library_entries_signal.get_untracked().len();
    if len == 0 {
        return;
    }
    selected_switcher_index.update(|idx| {
        let next = (*idx as i32 + delta).clamp(0, (len.saturating_sub(1)) as i32) as usize;
        *idx = next;
    });
}

fn switch_to_index(signals: Signals, library_entries_signal: RwSignal<Vec<LibraryEntry>>, index: usize, show_switcher: RwSignal<bool>) {
    if let Some(entry) = library_entries_signal.get_untracked().get(index) {
        signals.active_library_id.set(Some(entry.library_id.clone()));
        signals.active_library_name.set(entry.library_name.clone());
    }
    show_switcher.set(false);
}

fn active_library_index(signals: &Signals, library_entries_signal: RwSignal<Vec<LibraryEntry>>) -> Option<usize> {
    let active_id = signals.active_library_id.get_untracked();
    let entries = library_entries_signal.get_untracked();
    active_id.as_deref().and_then(|active| entries.iter().position(|e| e.library_id == active))
}

fn create_active_library_view(view: impl View + 'static, is_active: impl Fn() -> bool + 'static) -> impl View {
    view.style(move |s| if is_active() { s.width_full().height_full().flex_grow(1.0) } else { s.hide() })
}

fn create_switcher_select_button(name: String, is_selected: impl Fn() -> bool + 'static) -> impl View {
    create_list_item_button(name).style(move |s| if is_selected() { s.background(Color::rgb8(219, 234, 254)) } else { s })
}

fn create_switcher_overlay_container(panel: impl IntoView + 'static, show_switcher: RwSignal<bool>) -> impl View {
    create_centered_stack((panel,)).style(move |s| {
        let s = s.absolute().inset(0).size_full().background(Color::rgba8(15, 23, 42, 140)).z_index(100);
        if show_switcher.get() {
            s
        } else {
            s.hide()
        }
    })
}
