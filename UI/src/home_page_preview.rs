use std::rc::Rc;
use std::sync::atomic::Ordering;

use crate::components::labels::{create_header_label, create_label_background, create_padded_label};
use crate::components::stacks::{create_button_group, create_centered_stack, create_full_stack, create_list, create_list_bodyr, create_list_header, create_top_row};
use crate::components::{add_library_button, create_danger_button, create_list_item_button};
use crate::functions::add_library_on_click;
use crate::page_navigation::Signals;
use crate::style::palette;
use epub_svelte_lib::home::{get_library_entries, remove_library, LibraryEntry};
use epub_svelte_lib::library::Library;
use floem::event::{Event, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::prelude::{button, create_rw_signal, dyn_view, label, Decorators, RwSignal, SignalGet, SignalUpdate};
use floem::reactive::create_effect;
use floem::{IntoView, View};

#[derive(Clone)]
pub struct HomePagePreview {
    signals: Signals,
    library_entries_signal: RwSignal<Vec<LibraryEntry>>,
    show_switcher: RwSignal<bool>,
    selected_switcher_index: RwSignal<usize>,
}

impl HomePagePreview {
    pub fn new(
    signals                          : Signals)
    -> Self
    {
        let library_entries_signal = create_rw_signal(get_library_entries());
        let show_switcher = create_rw_signal(false);
        let selected_switcher_index = create_rw_signal(0);

        let preview = Self {
            signals,
            library_entries_signal,
            show_switcher,
            selected_switcher_index,
        };

        preview.initialize_active_library();
        let selected_index = preview.active_library_index().unwrap_or(0);
        preview.selected_switcher_index.set(selected_index);

        preview
    }

    pub fn home_view(
    &self)
    -> impl View
    {
        let top_bar = self.create_top_bar();
        let content = self.create_home_content();
        let switcher_overlay = self.create_switcher_overlay();
        let home_root = create_full_stack((top_bar, content, switcher_overlay));
        let home_root = self.attach_home_keyboard_handlers(home_root);

        self.attach_home_focus_handlers(home_root)
    }

    fn create_home_content(
    &self)
    -> impl View
    {
        let this = self.clone();
        let content_view = dyn_view(move || {
            let entries = this.library_entries_signal.get();
            match entries.is_empty() {
                true => Box::new(create_padded_label("Add a library to get started".to_string())) as Box<dyn View>,
                false => this.create_active_library_content(),
            }
        });

        create_full_stack((content_view,))
    }

    fn create_active_library_content(
    &self)
    -> Box<dyn View>
    {
        self.execute_ensure_library_handles();

        let this = self.clone();
        let active_library_view = dyn_view(move || {
            match this.signals.active_library_id.get() {
                None => Box::new(label(|| "")) as Box<dyn View>,
                Some(id) => this.resolve_active_library_view(id),
            }
        })
        .style(|s| s.width_full().height_full().flex_grow(1.0));

        Box::new(active_library_view) as Box<dyn View>
    }

    fn resolve_active_library_view(
    &self,
    library_id                       : String)
    -> Box<dyn View>
    {
        match self.signals.library_state(&library_id) {
            Some(state) => Box::new(state.library_view(self.signals.clone())) as Box<dyn View>,
            None => Box::new(label(|| "Library not found".to_string())) as Box<dyn View>,
        }
    }

    fn execute_ensure_library_handles(
    &self)
    {
        let entries = self.library_entries_signal.get();
        let mut handles = self.signals.library_handles.get_untracked();
        let mut changed = false;

        for entry in &entries {
            if handles.contains_key(&entry.library_id) {
                continue;
            }

            let (library, events) = Library::new(&entry.library_path, &entry.library_id);
            let event_counter = self.signals.library_event_counter.clone();
            std::thread::spawn(move || {
                while let Ok(_event) = events.recv() {
                    event_counter.fetch_add(1, Ordering::Relaxed);
                }
            });

            handles.insert(entry.library_id.clone(), Rc::new(library));
            changed = true;
        }

        if changed {
            self.signals.library_handles.set(handles);
        }
    }

    fn create_top_bar(
    &self)
    -> impl View
    {
        let this = self.clone();
        let active_name_signal = self.signals.active_library_name;
        let switcher_button = button(label(move || active_name_signal.get())).on_click(move |_| {
            this.toggle_switcher();
            EventPropagation::Continue
        });

        create_top_row(create_button_group((switcher_button,)))
    }

    fn create_switcher_overlay(
    &self)
    -> impl View
    {
        let this = self.clone();
        dyn_view(move || {
            let library_entries = this.library_entries_signal.get();
            let list = this.create_switcher_list(library_entries);

            let add_button = add_library_button().on_click(add_library_on_click(this.library_entries_signal.write_only()));
            let header = create_list_header((create_header_label("Switch library".to_string()), add_button));
            let panel = create_list((header, list)).on_click(|_| EventPropagation::Stop);

            Box::new(create_switcher_overlay_container(panel, this.show_switcher).on_click({
                let this = this.clone();
                move |_| {
                    this.show_switcher.set(false);
                    EventPropagation::Stop
                }
            })) as Box<dyn View>
        })
        .style({
            let this = self.clone();
            move |s| match this.show_switcher.get() {
                true => s.absolute().inset(0).size_full().z_index(100),
                false => s.hide(),
            }
        })
    }

    fn create_switcher_list(
    &self,
    library_entries                  : Vec<LibraryEntry>)
    -> Box<dyn View>
    {
        match library_entries.is_empty() {
            true => Box::new(create_label_background("No libraries available".to_string())) as Box<dyn View>,
            false => {
                let this = self.clone();
                let iter = library_entries.into_iter().enumerate().map(move |(index, library_entry)| this.create_switcher_item(index, library_entry));
                Box::new(create_list_bodyr(iter)) as Box<dyn View>
            }
        }
    }

    fn create_switcher_item(
    &self,
    index                            : usize,
    library_entry                    : LibraryEntry)
    -> impl IntoView
    {
        let name = library_entry.library_name;
        let library_path = library_entry.library_path;

        let this_for_selected = self.clone();
        let this_for_switch = self.clone();
        let select_button = create_switcher_select_button(name, move || this_for_selected.selected_switcher_index.get() == index)
            .on_click(move |_| {
                this_for_switch.switch_to_index(index);
                EventPropagation::Continue
            });

        let entries_signal = self.library_entries_signal;
        let delete_button = create_danger_button("Delete").on_click(move |_| {
            remove_library(&library_path);
            entries_signal.set(get_library_entries());
            EventPropagation::Stop
        });

        create_button_group((select_button, delete_button))
    }

    fn attach_home_keyboard_handlers(
    &self,
    home_root                        : impl View + 'static)
    -> impl View
    {
        let this = self.clone();
        home_root.on_event(floem::event::EventListener::KeyDown, move |event| {
            let Event::KeyDown(key_event) = event else {
                return EventPropagation::Continue;
            };
            if key_event.modifiers.control() {
                match &key_event.key.logical_key {
                    Key::Character(ch) if ch.eq("l") => {
                        this.toggle_switcher();
                        EventPropagation::Stop
                    }
                    _ => EventPropagation::Continue,
                }
            } else if this.show_switcher.get() {
                match &key_event.key.logical_key {
                    Key::Named(NamedKey::ArrowDown) => {
                        this.move_selection(1);
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        this.move_selection(-1);
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::Enter) => {
                        this.confirm_selection();
                        EventPropagation::Stop
                    }
                    Key::Named(NamedKey::Escape) => {
                        this.show_switcher.set(false);
                        EventPropagation::Stop
                    }
                    _ => EventPropagation::Continue,
                }
            } else {
                EventPropagation::Continue
            }
        })
    }

    fn attach_home_focus_handlers(
    &self,
    home_root                        : impl View + 'static)
    -> impl View
    {
        let home_root_id = home_root.id();
        let home_root = home_root.on_click(move |_| {
            home_root_id.request_focus();
            EventPropagation::Continue
        });

        let home_root_id = home_root.id();
        let show_switcher = self.show_switcher;
        create_effect(move |_| {
            if !show_switcher.get() {
                home_root_id.request_focus();
            }
        });

        home_root
    }

    fn initialize_active_library(
    &self)
    {
        let first_entry = self.library_entries_signal.get_untracked().first().cloned();
        let Some(first_entry) = first_entry else { return; };

        if self.signals.active_library_name.get_untracked().is_empty() {
            self.signals.active_library_name.set(first_entry.library_name.clone());
        }
        if self.signals.active_library_id.get_untracked().is_none() {
            self.signals.active_library_id.set(Some(first_entry.library_id));
        }
    }

    fn toggle_switcher(
    &self)
    {
        match self.show_switcher.get_untracked() {
            true => self.show_switcher.set(false),
            false => {
                self.selected_switcher_index.set(self.active_library_index().unwrap_or(0));
                self.show_switcher.set(true);
            }
        }
    }

    fn move_selection(
    &self,
    delta                            : i32)
    {
        let len = self.library_entries_signal.get_untracked().len();
        if len == 0 { return; }

        self.selected_switcher_index.update(|idx| {
            *idx = clamp_index_with_delta(*idx, len, delta);
        });
    }

    fn confirm_selection(
    &self)
    {
        let index = self.selected_switcher_index.get();
        self.switch_to_index(index);
    }

    fn switch_to_index(
    &self,
    index                            : usize)
    {
        if let Some(entry) = self.library_entries_signal.get_untracked().get(index) {
            self.signals.active_library_id.set(Some(entry.library_id.clone()));
            self.signals.active_library_name.set(entry.library_name.clone());
        }

        self.show_switcher.set(false);
    }

    fn active_library_index(
    &self)
    -> Option<usize>
    {
        let active_id = self.signals.active_library_id.get_untracked();
        let entries = self.library_entries_signal.get_untracked();

        active_id.as_deref().and_then(|active| entries.iter().position(|entry| entry.library_id == active))
    }
}

fn clamp_index_with_delta(
current                          : usize,
len                              : usize,
delta                            : i32)
-> usize
{
    (current as i32 + delta).clamp(0, (len.saturating_sub(1)) as i32) as usize
}

fn create_switcher_select_button(
name                             : String,
is_selected                      : impl Fn() -> bool + 'static)
-> impl View
{
    create_list_item_button(name).style(move |s| match is_selected() {
        true => s.background(palette().active_highlight),
        false => s,
    })
}

fn create_switcher_overlay_container(
panel                            : impl IntoView + 'static,
show_switcher                    : RwSignal<bool>)
-> impl View
{
    create_centered_stack((panel,)).style(move |s| {
        let styled = s.absolute().inset(0).size_full().background(palette().overlay_scrim).z_index(100);
        match show_switcher.get() {
            true => styled,
            false => styled.hide(),
        }
    })
}
