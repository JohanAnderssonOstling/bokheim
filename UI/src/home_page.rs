use std::collections::HashMap;
use std::path::Path;

use epub_svelte_lib::home::{create_library, get_library_entries, remove_library, LibraryEntry};
use floem::action::open_file;
use floem::event::EventPropagation;
use floem::file::{FileDialogOptions, FileInfo};
use floem::peniko::Color;
use floem::prelude::{
    button, container, create_rw_signal, create_signal, dyn_view, h_stack, label, stack_from_iter,
    v_stack, Decorators, RwSignal, SignalGet, SignalUpdate,
};
use floem::reactive::{create_effect, WriteSignal};
use floem::style::{AlignItems, CursorStyle, JustifyContent};
use floem::views::ScrollExt;
use floem::{IntoView, View};
use crate::components::{add_library_button, create_top_row};
use crate::functions::add_library_on_click;
use crate::library_page::LibraryState;
use crate::page_navigation::{LibraryID, Signals};
#[derive(Clone, Eq, PartialEq)]

pub struct HomePage {
    pub active_library: Option<LibraryID>,
    pub library_states: HashMap<LibraryID, LibraryState>,
}

impl HomePage {
    pub fn home_view(&mut self, signals: Signals) -> impl View {
        let library_entries = get_library_entries();
        self.active_library = Some(library_entries.first().unwrap().library_id.clone());
        let library_entries_signal = create_rw_signal(library_entries);

        let show_switcher = create_rw_signal(false);

        let top_bar = create_top_bar(signals.clone(), show_switcher);
        let switcher_overlay = create_switcher_overlay(signals.clone(), show_switcher, library_entries_signal);

        let empty_state = dyn_view(move || {
            let entries = library_entries_signal.get();
            if entries.is_empty() {
                Box::new(
                    container(label(|| "No libraries yet. Add one to get started."))
                        .style(|s| s.padding(24).width_full()),
                ) as Box<dyn View>
            } else {
                let book_id = library_entries_signal.get();
                let library_state =
                    Box::new(label(|| "").style(|s| s.font_size(0).height(0))) as Box<dyn View>;
                library_state
            }
        });



        //let content = v_stack((empty_state, library_view(signals.clone())))
         //   .style(|s| s.width_full().height_full().flex_grow(1.0));

        v_stack((top_bar, switcher_overlay))
            .style(|s| s.width_full().height_full().flex_grow(1.0))
    }
}



fn create_top_bar(signals: Signals, show_switcher: RwSignal<bool>, ) -> impl View {
    let signals_for_back = signals.clone();
    let back_button = button(label(|| "Back"))
        .on_click(move |_| {
            signals_for_back.go_back();
            EventPropagation::Continue
        });

    let signals_for_label = signals.clone();
    let show_switcher_for_button = show_switcher.clone();

    let switcher_button = button(label(move || {
        "hello".to_string()
    }))
        .on_click(move |_| {
            show_switcher_for_button.update(|open| *open = !*open);
            EventPropagation::Continue
        });

    let left_group = h_stack((back_button, switcher_button))
        .style(|s| s.gap(12).align_items(Some(AlignItems::Center)));

    let top_row = create_top_row(left_group);


    top_row
}

fn create_switcher_item(library_entry: LibraryEntry, signals: Signals, show_switcher: RwSignal<bool>, library_entries_signal: RwSignal<Vec<LibraryEntry>>,
) -> impl IntoView {
    let name = library_entry.library_name;
    let entry_path = library_entries_signal.get_untracked();
    let select_button = button(label(move || name.clone()))
        .on_click(move |_| {
            signals.active_library_id.set(Some(library_entry.library_id.clone()));
            show_switcher.set(false);
            EventPropagation::Continue
        })
        .style(|s| {
            s.flex_grow(1.0)
                .padding(8)
                .border_radius(6)
                .justify_start()
                .cursor(CursorStyle::Pointer)
                .hover(|s| s.background(Color::rgb8(243, 244, 246)))
        });
    let delete_button = button(label(|| "Delete"))
        .on_click(move |_| {
            remove_library(&library_entry.library_path);
            library_entries_signal.set(get_library_entries());
            EventPropagation::Stop
        })
        .style(|s| {
            s.padding_left(8)
                .padding_right(8)
                .padding_top(6)
                .padding_bottom(6)
                .border_radius(6)
                .border(1)
                .border_color(Color::rgb8(220, 38, 38))
                .color(Color::rgb8(220, 38, 38))
                .hover(|s| s.background(Color::rgb8(254, 226, 226)))
        });
    h_stack((select_button, delete_button))
        .style(|s| s.gap(8).align_items(Some(AlignItems::Center)))
}

fn create_switcher_overlay(signals: Signals, show_switcher: RwSignal<bool>, library_entries_signal: RwSignal<Vec<LibraryEntry>>,
) -> impl View {
    dyn_view(move || {
        let library_entries = library_entries_signal.get();
        println!("Entries: {}", library_entries.len());
        let list = if library_entries.is_empty() {
            Box::new(label(|| "No libraries available").style(|s| {
                s.padding(16)
                    .border_radius(8)
                    .background(Color::rgb8(249, 250, 251))
                    .border(1)
                    .border_color(Color::rgb8(229, 231, 235))
            })) as Box<dyn View>
        } else {
            let library_list = stack_from_iter(library_entries.into_iter().map(|library_entry| {
                create_switcher_item(library_entry, signals.clone(), show_switcher.clone(), library_entries_signal.clone())
            }));
            Box::new(
                library_list
                .style(|s| s.flex_col().gap(8).width_full())
                .scroll()
                .style(|s| s.flex_grow(1.0).width_full()),
            ) as Box<dyn View>
        };

        let header_signals = signals.clone();
        let header_set_libraries = library_entries_signal.clone();
        let add_button = add_library_button()
            .on_click(add_library_on_click(library_entries_signal.write_only()));

        let header = h_stack((
            label(|| "Switch library").style(|s| s.font_size(20)),
            add_button,
        ))
        .style(|s| {
            s.justify_content(Some(JustifyContent::SpaceBetween))
                .width_full()
        });

        let panel = v_stack((header, list))
            .style(|s| {
                s.size_pct(50.0, 50.0)
                    .min_width(320)
                    .min_height(240)
                    .max_width_pct(90.0)
                    .max_height_pct(90.0)
                    .padding(24)
                    .gap(16)
                    .background(Color::WHITE)
                    .border(1)
                    .border_color(Color::rgb8(229, 231, 235))
                    .border_radius(16)
                    .box_shadow_color(Color::rgba8(0, 0, 0, 150))
                    .box_shadow_blur(28)
                    .box_shadow_h_offset(0)
                    .box_shadow_v_offset(16)
            })
            .on_click(|_| EventPropagation::Stop);

        Box::new(
            v_stack((panel,))
                .style(|s| s.size_full().items_center().justify_center())
                .on_click({
                    let show_switcher = show_switcher.clone();
                    move |_| {
                        show_switcher.set(false);
                        EventPropagation::Stop
                    }
                }),
        ) as Box<dyn View>
    })
    .style(move |s| {
        let s = s
            .absolute()
            .inset(0)
            .size_full()
            .background(Color::rgba8(15, 23, 42, 140))
            .z_index(100);
        if show_switcher.get() {
            s
        } else {
            s.hide()
        }
    })
}

