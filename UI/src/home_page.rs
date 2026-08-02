use crate::auth_view::{account_button, auth_modal};
use crate::components::{
    create_switcher_select_button, dynamic_button, full_page_stack, label_card, padded_text, page_surface, secondary_button, switcher_delete_button, switcher_header, switcher_item_row, switcher_overlay_root, switcher_panel, switcher_rows,
    switcher_title, top_bar, top_bar_group,
};
use crate::library_page::LibraryState;
use crate::page_navigation::Signals;
use crate::style::theme_button_label;
use epub_svelte_lib::preferences::BrowsingPreferencePatch;
use floem::action::{exec_after, inspect, open_file};
use floem::event::{Event, EventPropagation};
use floem::file::FileDialogOptions;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::prelude::{create_rw_signal, dyn_view, Decorators, RwSignal, SignalGet, SignalUpdate, SignalWith};
use floem::views::stack_from_iter;
use floem::View;
use std::collections::HashMap;
use std::time::Duration;

pub fn home_view(signals: Signals) -> impl View {
    if let Some(first) = signals.libraries.get_untracked().first() {
        if signals.active_library_id.get_untracked().is_none() {
            signals.active_library_id.set(Some(first.meta.id.clone()));
        }
    }

    let show_switcher = create_rw_signal(false);
    let selected_switcher_index = create_rw_signal(active_library_index(&signals).unwrap_or(0));
    let show_auth_modal = create_rw_signal(false);

    let top_bar = {
        let signals_for_button = signals.clone();
        let signals_for_account = signals.clone();
        let active_name_signal = signals.clone();
        let switcher_button = secondary_button(active_name_signal.active_library_name()).on_click(move |_| {
            toggle_switcher(show_switcher, selected_switcher_index, signals_for_button.clone());
            EventPropagation::Continue
        });
        let theme_signals = signals.clone();
        let theme_button = secondary_button(theme_button_label()).on_click(move |_| {
            let next = theme_signals.application_theme.get_untracked().next();
            match theme_signals.app_core.update_browsing_preference(BrowsingPreferencePatch::Theme(next)) {
                Ok(_) => theme_signals.application_theme.set(next),
                Err(error) => eprintln!("failed to save browsing theme: {error}"),
            }
            EventPropagation::Continue
        });
        let interface_font_signals = signals.clone();
        let interface_font_label = signals.clone();
        let interface_font_button = dynamic_button(move || format!("Interface: {}", interface_font_label.interface_font.get().font_name())).on_click(move |_| {
            let next = interface_font_signals.interface_font.get_untracked().next();
            match interface_font_signals.app_core.update_browsing_preference(BrowsingPreferencePatch::InterfaceFont(next)) {
                Ok(_) => interface_font_signals.interface_font.set(next),
                Err(error) => eprintln!("failed to save interface font: {error}"),
            }
            EventPropagation::Continue
        });
        let heading_font_signals = signals.clone();
        let heading_font_label = signals.clone();
        let heading_font_button = dynamic_button(move || format!("Headings: {}", heading_font_label.heading_font.get().font_name())).on_click(move |_| {
            let next = heading_font_signals.heading_font.get_untracked().next();
            match heading_font_signals.app_core.update_browsing_preference(BrowsingPreferencePatch::HeadingFont(next)) {
                Ok(_) => heading_font_signals.heading_font.set(next),
                Err(error) => eprintln!("failed to save heading font: {error}"),
            }
            EventPropagation::Continue
        });
        let inspect_button = secondary_button("Inspect").on_click(move |_| {
            inspect();
            EventPropagation::Continue
        });
        let left_group = top_bar_group((switcher_button, theme_button, interface_font_button, heading_font_button, inspect_button));
        let account = account_button(signals_for_account, show_auth_modal);
        top_bar(left_group, account)
    };
    let switcher_overlay = {
        let signals_for_dyn = signals.clone();
        dyn_view(move || {
            let libraries = signals_for_dyn.libraries.get();
            let list = if libraries.is_empty() {
                Box::new(label_card("No libraries available")) as Box<dyn View>
            } else {
                let signals_for_iter = signals_for_dyn.clone();
                let rows = libraries.into_iter().enumerate().map(move |(idx, library)| {
                    let name = library.meta.name.clone();
                    let library_id = library.meta.id.clone();
                    let signals_for_select = signals_for_iter.clone();
                    let select_button = create_switcher_select_button(name, move || selected_switcher_index.get() == idx).on_click(move |_| {
                        switch_to_index(signals_for_select.clone(), idx, show_switcher);
                        EventPropagation::Continue
                    });
                    let delete_button = switcher_delete_button().on_click(on_delete_library(library_id, signals_for_iter.clone()));
                    switcher_item_row(select_button, delete_button)
                });
                Box::new(switcher_rows(stack_from_iter(rows))) as Box<dyn View>
            };

            let add_button = secondary_button("Add library").on_click(add_library_on_click(signals_for_dyn.clone()));
            let header = switcher_header(switcher_title("Switch library"), add_button);
            let panel = switcher_panel((header, list)).on_click(|_| EventPropagation::Stop);

            Box::new(switcher_overlay_root(panel, show_switcher).on_click(move |_| {
                show_switcher.set(false);
                EventPropagation::Stop
            })) as Box<dyn View>
        })
    };
    let auth_overlay = auth_modal(signals.clone(), show_auth_modal);

    let signals_for_content = signals.clone();
    let content_view = page_surface(dyn_view(move || match signals_for_content.active_library_id.get() {
        None => Box::new(padded_text("Add a library to get started")) as Box<dyn View>,
        Some(id) => match signals_for_content.library_by_id(&id) {
            Some(state) => Box::new(state.library_view(signals_for_content.clone())) as Box<dyn View>,
            None => Box::new(padded_text("Library not found")) as Box<dyn View>,
        },
    }));
    let signals_for_l = signals.clone();
    let signals_for_enter = signals.clone();
    let home_root = full_page_stack((top_bar, content_view, switcher_overlay, auth_overlay))
        .on_key_down(
            Key::Character("l".into()),
            |mods: Modifiers| mods.control(),
            move |_| {
                toggle_switcher(show_switcher, selected_switcher_index, signals_for_l.clone());
            },
        )
        .on_key_down(Key::Named(NamedKey::ArrowDown), move |_mods: Modifiers| show_switcher.get(), {
            let signals = signals.clone();
            move |_| move_selection(selected_switcher_index, &signals, 1)
        })
        .on_key_down(Key::Named(NamedKey::ArrowUp), move |_mods: Modifiers| show_switcher.get(), {
            let signals = signals.clone();
            move |_| move_selection(selected_switcher_index, &signals, -1)
        })
        .on_key_down(
            Key::Named(NamedKey::Enter),
            move |_mods: Modifiers| show_switcher.get(),
            move |_| {
                switch_to_index(signals_for_enter.clone(), selected_switcher_index.get(), show_switcher);
            },
        )
        .on_key_down(
            Key::Named(NamedKey::Escape),
            move |_mods: Modifiers| show_switcher.get(),
            move |_| {
                show_switcher.set(false);
            },
        )
        .on_key_down(
            Key::Character("i".into()),
            |mods: Modifiers| mods.control() && mods.shift(),
            move |_| {
                inspect();
            },
        );

    let home_root_id = home_root.id();
    home_root.on_click(move |_| {
        home_root_id.request_focus();
        EventPropagation::Continue
    })
}

fn add_library_on_click(signals: Signals) -> impl FnMut(&Event) -> EventPropagation {
    move |_| {
        let file_dialog_options = FileDialogOptions::new().select_directories().title("Select Folder");
        open_file(file_dialog_options, {
            let signals = signals.clone();
            move |file_info| {
                let Some(file_info) = file_info else { return };
                for new_library_path in file_info.path.iter() {
                    let name = new_library_path.file_name().and_then(|s| s.to_str()).unwrap_or("Library").to_string();
                    if let Err(err) = signals.app_core.create_library(name, Some(new_library_path.clone())) {
                        eprintln!("failed to create library at {}: {}", new_library_path.display(), err);
                    }
                }
                let entries = signals.app_core.libraries().unwrap_or_else(|error| {
                    eprintln!("failed to refresh library registry: {error}");
                    Vec::new()
                });
                let signals = signals.clone();
                exec_after(Duration::from_millis(0), move |_| {
                    signals.libraries.update(|libs| {
                        let mut existing_by_id: HashMap<String, LibraryState> = libs.iter().cloned().map(|s| (s.meta.id.clone(), s)).collect();
                        let next = entries
                            .iter()
                            .map(|entry| {
                                let library_id = entry.library_id().as_str().to_owned();
                                existing_by_id.remove(&library_id).unwrap_or_else(|| LibraryState::new(library_id, entry.library_name().to_owned(), entry.library_path().to_string_lossy().into_owned()))
                            })
                            .collect::<Vec<_>>();
                        *libs = next;
                    });
                    if signals.active_library_id.get_untracked().is_none() {
                        let first_id = signals.libraries.with_untracked(|v| v.first().map(|s| s.meta.id.clone()));
                        signals.active_library_id.set(first_id);
                    }
                });
            }
        });

        EventPropagation::Continue
    }
}

fn on_delete_library(library_id: String, signals: Signals) -> impl FnMut(&floem::event::Event) -> EventPropagation {
    move |_| {
        if let Err(error) = signals.app_core.remove_library(&library_id) {
            eprintln!("failed to remove library {library_id}: {error}");
            return EventPropagation::Stop;
        }
        signals.libraries.update(|v| v.retain(|s| s.meta.id != library_id));
        if signals.active_library_id.get_untracked().as_deref() == Some(library_id.as_str()) {
            let next_id = signals.libraries.with_untracked(|v| v.first().map(|s| s.meta.id.clone()));
            signals.active_library_id.set(next_id);
        }
        EventPropagation::Stop
    }
}

fn toggle_switcher(show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>, signals: Signals) {
    if show_switcher.get_untracked() {
        show_switcher.set(false);
    } else {
        selected_switcher_index.set(active_library_index(&signals).unwrap_or(0));
        show_switcher.set(true);
    }
}

fn move_selection(selected_switcher_index: RwSignal<usize>, signals: &Signals, delta: i32) {
    let len = signals.libraries.with_untracked(|v| v.len());
    if len == 0 {
        return;
    }
    selected_switcher_index.update(|idx| {
        let next = (*idx as i32 + delta).clamp(0, (len.saturating_sub(1)) as i32) as usize;
        *idx = next;
    });
}

fn switch_to_index(signals: Signals, index: usize, show_switcher: RwSignal<bool>) {
    let id = signals.libraries.with_untracked(|v| v.get(index).map(|s| s.meta.id.clone()));
    if let Some(id) = id {
        signals.active_library_id.set(Some(id));
    }
    show_switcher.set(false);
}

fn active_library_index(signals: &Signals) -> Option<usize> {
    let active_id = signals.active_library_id.get_untracked();
    signals.libraries.with_untracked(|v| active_id.as_deref().and_then(|active| v.iter().position(|s| s.meta.id == active)))
}
