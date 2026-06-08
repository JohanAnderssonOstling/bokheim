use crate::auth_view::{account_button, auth_modal};
use crate::components::labels::{create_label_background, create_padded_label};
use crate::components::page_views::{
    create_overlay_scrim, create_switcher_select_button, dialog_action_button, full_page_container, full_page_stack, switcher_header,
    switcher_item_row, switcher_list, switcher_overlay_content, switcher_panel, switcher_rows, switcher_title, top_bar, top_bar_group,
};
use crate::components::{add_library_button, create_danger_button, create_dialog_button};
use crate::functions::add_library_on_click;
use crate::library_page::LibraryState;
use crate::page_navigation::Signals;
use crate::style::{theme_button_label, toggle_theme};
use epub_svelte_lib::home::remove_library;
use floem::action::inspect;
use floem::event::EventPropagation;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::prelude::{create_rw_signal, dyn_view, label, Decorators, RwSignal, SignalGet, SignalUpdate, SignalWith};
use floem::views::stack_from_iter;
use floem::View;

pub fn home_view(signals: Signals) -> impl View {
    if let Some(first) = signals.libraries.get_untracked().first() {
        if signals.active_library_id.get_untracked().is_none() {
            signals.active_library_id.set(Some(first.meta.id.clone()));
        }
    }

    let show_switcher = create_rw_signal(false);
    let selected_switcher_index = create_rw_signal(active_library_index(&signals).unwrap_or(0));
    let show_auth_modal = create_rw_signal(false);

    let top_bar = create_top_bar(signals.clone(), show_switcher, selected_switcher_index, show_auth_modal);
    let switcher_overlay = create_switcher_overlay(signals.clone(), show_switcher, selected_switcher_index);
    let auth_overlay = auth_modal(signals.clone(), show_auth_modal);

    let signals_for_content = signals.clone();
    let content_view = full_page_container(dyn_view(move || match signals_for_content.active_library_id.get() {
        None => Box::new(create_padded_label("Add a library to get started".to_string())) as Box<dyn View>,
        Some(id) => match signals_for_content.library_by_id(&id) {
            Some(state) => Box::new(state.library_view(signals_for_content.clone())) as Box<dyn View>,
            None => Box::new(create_padded_label("Library not found".to_string())) as Box<dyn View>,
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
        .on_key_down(
            Key::Named(NamedKey::ArrowDown),
            move |_mods: Modifiers| show_switcher.get(),
            {
                let signals = signals.clone();
                move |_| move_selection(selected_switcher_index, &signals, 1)
            },
        )
        .on_key_down(
            Key::Named(NamedKey::ArrowUp),
            move |_mods: Modifiers| show_switcher.get(),
            {
                let signals = signals.clone();
                move |_| move_selection(selected_switcher_index, &signals, -1)
            },
        )
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
    let home_root = home_root.on_click(move |_| {
        home_root_id.request_focus();
        EventPropagation::Continue
    });

    home_root
}

fn create_top_bar(signals: Signals, show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>, show_auth_modal: RwSignal<bool>) -> impl View {
    let signals_for_button = signals.clone();
    let signals_for_account = signals.clone();
    let active_name_signal = signals.clone();
    let switcher_button = dialog_action_button(label(move || active_name_signal.active_library_name()))
        .on_click(move |_| {
            toggle_switcher(show_switcher, selected_switcher_index, signals_for_button.clone());
            EventPropagation::Continue
        });
    let theme_button = dialog_action_button(label(theme_button_label)).on_click(move |_| {
        toggle_theme();
        EventPropagation::Continue
    });
    let inspect_button = create_dialog_button("Inspect").on_click(move |_| {
        inspect();
        EventPropagation::Continue
    });
    let left_group = top_bar_group((switcher_button, theme_button, inspect_button));
    let account = account_button(signals_for_account, show_auth_modal);
    top_bar(left_group, account)
}

fn create_switcher_item(index: usize, library: LibraryState, signals: Signals, show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>) -> impl View {
    let name = library.meta.name.clone();
    let library_id = library.meta.id.clone();
    let signals_for_select = signals.clone();
    let select_button = create_switcher_select_button(name, move || selected_switcher_index.get() == index).on_click(move |_| {
        switch_to_index(signals_for_select.clone(), index, show_switcher);
        EventPropagation::Continue
    });
    let delete_button = create_danger_button("Delete")
        .style(|s| s.width(88).height(36).flex_shrink(0.0))
        .on_click(on_delete_library(library.meta.path.clone(), library_id, signals));
    switcher_item_row(select_button.style(|s| s.flex_grow(1.0).height(36).justify_start().padding_horiz(12)), delete_button)
}

fn create_switcher_overlay(signals: Signals, show_switcher: RwSignal<bool>, selected_switcher_index: RwSignal<usize>) -> impl View {
    let signals_for_dyn = signals.clone();
    dyn_view(move || {
        let libraries = signals_for_dyn.libraries.get();
        let list = if libraries.is_empty() {
            Box::new(create_label_background("No libraries available".to_string())) as Box<dyn View>
        } else {
            let signals_for_iter = signals_for_dyn.clone();
            let rows = libraries
                .into_iter()
                .enumerate()
                .map(move |(idx, library)| create_switcher_item(idx, library, signals_for_iter.clone(), show_switcher, selected_switcher_index));
            Box::new(switcher_rows(stack_from_iter(rows))) as Box<dyn View>
        };

        let add_button = add_library_button().style(|s| s.height(36)).on_click(add_library_on_click(signals_for_dyn.clone()));
        let header = switcher_header(switcher_title("Switch library"), add_button);
        let list = switcher_list(list);
        let panel = switcher_panel((header, list)).on_click(|_| EventPropagation::Stop);

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

fn on_delete_library(library_path: String, library_id: String, signals: Signals) -> impl FnMut(&floem::event::Event) -> EventPropagation {
    move |_| {
        remove_library(&library_path);
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

fn create_switcher_overlay_container(panel: impl View + 'static, show_switcher: RwSignal<bool>) -> impl View {
    create_overlay_scrim(show_switcher, switcher_overlay_content(panel))
}
