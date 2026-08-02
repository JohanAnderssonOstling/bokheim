use floem::event::EventPropagation;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use floem::prelude::{dyn_view, Decorators, SignalUpdate, View};
use floem::reactive::{create_effect, create_rw_signal, SignalGet};
use html_renderer::{HTMLRenderer, RendererCommand, RendererEvent};

use crate::components::reader_views::*;
use crate::components::{inline_message, padded_text, secondary_button, square_button, toc_view};
use crate::page_navigation::{ReaderState, Signals};
use crate::toc_model::{build_toc_anchor_strings_by_doc, build_toc_resolution_maps, map_toc_entries, toc_title_for_resolved};
use epub_svelte_lib::preferences::ReaderPreferencePatch;

pub fn create_epub_reader(reader_state: &ReaderState, signals: Signals) -> Box<dyn View> {
    let book_title = reader_state.book_title.clone();
    let book_bytes = match signals.app_core.read_book_bytes_in(&reader_state.library_id, reader_state.book_id) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("failed to read book bytes for {}: {}", reader_state.book_id, err);
            return Box::new(padded_text("Book not found"));
        }
    };
    let book_position = signals.app_core.reading_position_in(&reader_state.library_id, reader_state.book_id).unwrap_or_else(|error| {
        eprintln!("failed to load reading position for {}: {error}", reader_state.book_id,);
        None
    });
    if let Err(error) = signals.app_core.mark_book_opened_in(&reader_state.library_id, reader_state.book_id) {
        eprintln!("failed to mark book {} as opened: {error}", reader_state.book_id,);
    }

    let nav_signals = signals.clone();
    let back_button = secondary_button("Back").on_click(move |_| {
        nav_signals.go_back();
        EventPropagation::Continue
    });

    let mut renderer = HTMLRenderer::from_epub_bytes_with_nav(book_bytes, book_position.as_deref());
    let book_toc_entries = renderer.toc().ok().flatten().unwrap_or_default();
    let book_toc_entries = Rc::new(map_toc_entries(&renderer, book_toc_entries));
    renderer.set_toc_anchor_strings_by_doc(build_toc_anchor_strings_by_doc(book_toc_entries.as_ref()));
    let restored_preferences = signals.reader_preferences.get_untracked();
    let restored_ui = signals.reader_ui.get_untracked().sanitized();
    renderer.apply(RendererCommand::SetColumnWidth(restored_preferences.max_line_width_px()));
    renderer.apply(RendererCommand::SetFontSize(restored_preferences.font_size_px()));
    renderer.apply(RendererCommand::SetScale(restored_preferences.zoom()));
    renderer.apply(RendererCommand::SetLineNumbers(restored_preferences.line_numbers()));

    let command_handle = renderer.command_handle();
    let nav_anchor_signal = create_rw_signal((0usize, None::<String>));
    let cfi_signal = create_rw_signal(None::<String>);
    let can_history_back_signal = create_rw_signal(false);
    let can_history_forward_signal = create_rw_signal(false);
    let doc_title_signal = create_rw_signal(None::<String>);
    let search_active = create_rw_signal(false);
    let match_info = create_rw_signal((0usize, 0usize));
    let settings_column_width = create_rw_signal(restored_preferences.max_line_width_px());
    let settings_font_size = create_rw_signal(restored_preferences.font_size_px());
    let settings_scale = create_rw_signal(restored_preferences.zoom());
    let settings_line_numbers = create_rw_signal(restored_preferences.line_numbers());

    renderer.set_event_handler(move |event| match event {
        RendererEvent::CfiChanged(cfi) => cfi_signal.set(cfi),
        RendererEvent::NavAnchorChanged { doc, anchor } => nav_anchor_signal.set((doc, anchor)),
        RendererEvent::TitleChanged(title) => doc_title_signal.set(title),
        RendererEvent::MatchInfo { current, total } => match_info.set((current, total)),
        RendererEvent::HistoryAvailability { back, forward } => {
            can_history_back_signal.set(back);
            can_history_forward_signal.set(forward);
        }
        RendererEvent::SearchActiveChanged(active) => search_active.set(active),
        RendererEvent::ColumnWidthChanged(width) => settings_column_width.set(width),
        RendererEvent::FontSizeChanged(size) => settings_font_size.set(size),
        RendererEvent::ScaleChanged(scale) => settings_scale.set(scale),
        RendererEvent::LineNumbersChanged(enabled) => settings_line_numbers.set(enabled),
        RendererEvent::OperationFailed { operation, message } => eprintln!("renderer failed to {operation}: {message}"),
        RendererEvent::PositionChanged { .. } | RendererEvent::TocChanged(_) => {}
    });
    let settings_menu_open = create_rw_signal(false);
    let toc_open = create_rw_signal(restored_ui.toc_open);
    let left_sidebar_width = create_rw_signal(restored_ui.left_panel_width);
    let last_position = create_rw_signal(None::<String>);
    let active_toc_entry = create_rw_signal(None::<(usize, Option<String>)>);

    create_effect({
        let signals = signals.clone();
        let command_handle = command_handle;
        move |_| {
            let prev = signals.reader_preferences.get_untracked();
            let mut next = prev;
            if let Err(error) = next.set_max_line_width_px(settings_column_width.get()) {
                eprintln!("renderer produced an invalid line width: {error}");
            }
            if let Err(error) = next.set_font_size_px(settings_font_size.get()) {
                eprintln!("renderer produced an invalid font size: {error}");
            }
            if let Err(error) = next.set_zoom(settings_scale.get()) {
                eprintln!("renderer produced an invalid zoom: {error}");
            }
            next.set_line_numbers(settings_line_numbers.get());
            let mut patches = Vec::new();
            if (prev.max_line_width_px() - next.max_line_width_px()).abs() >= f64::EPSILON {
                patches.push(ReaderPreferencePatch::MaxLineWidthPx(next.max_line_width_px()));
            }
            if (prev.font_size_px() - next.font_size_px()).abs() >= f32::EPSILON {
                patches.push(ReaderPreferencePatch::FontSizePx(next.font_size_px()));
            }
            if (prev.zoom() - next.zoom()).abs() >= f64::EPSILON {
                patches.push(ReaderPreferencePatch::Zoom(next.zoom()));
            }
            if prev.line_numbers() != next.line_numbers() {
                patches.push(ReaderPreferencePatch::LineNumbers(next.line_numbers()));
            }
            let mut canonical = prev;
            for patch in patches {
                match signals.app_core.update_reader_preference(patch) {
                    Ok(updated) => canonical = updated,
                    Err(error) => eprintln!("failed to save reader preference: {error}"),
                }
            }
            if canonical != prev {
                signals.reader_preferences.set(canonical);
            }
            let next_ui = crate::session::ReaderUiSession { left_panel_width: left_sidebar_width.get(), toc_open: toc_open.get() }.sanitized();
            if signals.reader_ui.get_untracked().sanitized().left_panel_width != next_ui.left_panel_width || signals.reader_ui.get_untracked().toc_open != next_ui.toc_open {
                signals.reader_ui.set(next_ui);
            }
            command_handle.apply(RendererCommand::SetColumnWidth(canonical.max_line_width_px()));
            command_handle.apply(RendererCommand::SetFontSize(canonical.font_size_px()));
            command_handle.apply(RendererCommand::SetScale(canonical.zoom()));
            command_handle.apply(RendererCommand::SetLineNumbers(canonical.line_numbers()));
        }
    });

    let book_id = reader_state.book_id;
    let library_id = reader_state.library_id.clone();
    let app_core = signals.app_core.clone();
    create_effect(move |_| {
        let cfi = cfi_signal.get();
        let Some(cfi) = cfi else {
            return;
        };
        if last_position.get_untracked().as_deref() == Some(cfi.as_str()) {
            return;
        }
        last_position.set(Some(cfi.clone()));
        if let Err(error) = app_core.update_reading_position_in(&library_id, book_id, &cfi) {
            eprintln!("failed to save reading position for {book_id}: {error}");
        }
        if let Err(error) = app_core.update_last_read_in(&library_id, book_id) {
            eprintln!("failed to update last-read time for {book_id}: {error}");
        }
    });

    {
        let book_toc_entries = book_toc_entries.clone();
        create_effect(move |_| {
            let location = nav_anchor_signal.get();
            let prev = active_toc_entry.get_untracked();
            let mut toc_resolved_set = HashSet::new();
            let mut toc_first_by_doc = HashMap::new();
            build_toc_resolution_maps(book_toc_entries.as_ref(), &mut toc_resolved_set, &mut toc_first_by_doc);
            let mut next = None;
            if toc_resolved_set.contains(&location) {
                next = Some(location.clone());
            } else if location.1.is_none() || prev.as_ref().map(|(doc, _)| *doc != location.0).unwrap_or(true) {
                next = toc_first_by_doc.get(&location.0).cloned();
            }
            if let Some(next) = next {
                if Some(next.clone()) != prev {
                    active_toc_entry.set(Some(next));
                }
            }
        });
    }

    let search_query_buf = create_rw_signal(String::new());

    create_effect(move |_| {
        let q = search_query_buf.get();
        command_handle.apply(RendererCommand::SetSearchQuery(q));
    });

    let search_bar = {
        let search_input = reader_search_input(search_query_buf, match_info);
        let search_input_id = search_input.id();
        create_effect(move |_| {
            if search_active.get() {
                search_input_id.request_focus();
            }
        });
        let search_input = search_input
            .on_key_down(
                floem::keyboard::Key::Named(floem::keyboard::NamedKey::Enter),
                |mods: floem::keyboard::Modifiers| !mods.shift(),
                move |_| {
                    command_handle.apply(RendererCommand::NavigateSearch(1));
                },
            )
            .on_key_down(
                floem::keyboard::Key::Named(floem::keyboard::NamedKey::Enter),
                |mods: floem::keyboard::Modifiers| mods.shift(),
                move |_| {
                    command_handle.apply(RendererCommand::NavigateSearch(-1));
                },
            );
        reader_search_bar((
            search_input,
            reader_search_count(match_info),
            square_button("<").on_click(move |_| {
                command_handle.apply(RendererCommand::NavigateSearch(-1));
                EventPropagation::Stop
            }),
            square_button(">").on_click(move |_| {
                command_handle.apply(RendererCommand::NavigateSearch(1));
                EventPropagation::Stop
            }),
            square_button("x").on_click(move |_| {
                command_handle.apply(RendererCommand::SetSearchActive(false));
                search_query_buf.set(String::new());
                EventPropagation::Stop
            }),
        ))
    };

    let body = reader_body(renderer, search_bar, move || search_active.get());

    let top_bar = {
        let book_toc_entries = book_toc_entries.clone();
        reader_top_bar(
            reader_top_bar_left((
                back_button,
                history_nav_button("←", can_history_back_signal).on_click(move |_| {
                    if can_history_back_signal.get() {
                        command_handle.apply(RendererCommand::NavigateHistory(-1));
                    }
                    EventPropagation::Stop
                }),
                history_nav_button("→", can_history_forward_signal).on_click(move |_| {
                    if can_history_forward_signal.get() {
                        command_handle.apply(RendererCommand::NavigateHistory(1));
                    }
                    EventPropagation::Stop
                }),
                square_button("☷").on_click(move |_| {
                    toc_open.update(|open| *open = !*open);
                    EventPropagation::Stop
                }),
                reader_title_label(move || {
                    let active = active_toc_entry.get();
                    let doc_title = toc_title_for_resolved(book_toc_entries.as_ref(), active).or_else(|| doc_title_signal.get());
                    if let Some(doc_title) = doc_title {
                        format!("{} • {}", book_title, doc_title)
                    } else {
                        book_title.clone()
                    }
                }),
            )),
            square_button("☰").on_click(move |_| {
                settings_menu_open.update(|open| *open = !*open);
                EventPropagation::Stop
            }),
        )
    };

    let settings_dropdown = reader_settings_dropdown(
        (
            stepper_row(
                "Column width",
                move || format!("{:.0}px", settings_column_width.get()),
                move || {
                    let next = (settings_column_width.get() - 40.0).clamp(200.0, 1200.0);
                    settings_column_width.set(next);
                },
                move || {
                    let next = (settings_column_width.get() + 40.0).clamp(200.0, 1200.0);
                    settings_column_width.set(next);
                },
            ),
            stepper_row(
                "Font size",
                move || format!("{:.0}px", settings_font_size.get()),
                move || {
                    let next = (settings_font_size.get() - 1.0).clamp(8.0, 64.0);
                    settings_font_size.set(next);
                },
                move || {
                    let next = (settings_font_size.get() + 1.0).clamp(8.0, 64.0);
                    settings_font_size.set(next);
                },
            ),
            stepper_row(
                "Zoom",
                move || format!("{:.1}x", settings_scale.get()),
                move || {
                    let next = (settings_scale.get() - 0.1).clamp(0.5, 3.0);
                    settings_scale.set(next);
                },
                move || {
                    let next = (settings_scale.get() + 0.1).clamp(0.5, 3.0);
                    settings_scale.set(next);
                },
            ),
            reader_line_numbers_row((
                reader_line_numbers_label(),
                reader_line_numbers_toggle(settings_line_numbers).on_click(move |_| {
                    let next = !settings_line_numbers.get();
                    settings_line_numbers.set(next);
                    EventPropagation::Stop
                }),
            )),
            reader_shortcuts_stack((
                reader_keyboard_shortcuts_title(),
                shortcut_label("Arrow Up / Down: Previous / Next line"),
                shortcut_label("Mouse wheel: Previous / Next line"),
                shortcut_label("Alt + Arrow Left / Right: Link history back / forward"),
                shortcut_label("L: Toggle line numbers"),
                shortcut_label("Arrow Left / Right: Previous / Next page"),
                shortcut_label("Ctrl + Arrow Left / Right: Previous / Next chapter"),
                shortcut_label("Ctrl + F: Toggle search"),
                shortcut_label("Enter / Shift+Enter (in search): Next / Previous match"),
                shortcut_label("Esc: Close search"),
                shortcut_label("Ctrl + C: Copy current selection"),
                shortcut_label("Ctrl + +/-: Zoom in / out"),
                shortcut_label("Ctrl + Alt + +/-: Column width +/-"),
                shortcut_label("Alt + Arrow Up / Down: Font size +/-"),
            )),
        ),
        move || settings_menu_open.get(),
    );

    let on_toc_click = {
        Rc::new(move |href: String| {
            command_handle.apply(RendererCommand::NavigateToHref(href));
        })
    };

    let left_toc_panel = {
        let book_toc_entries = book_toc_entries.clone();
        let on_toc_click = on_toc_click.clone();
        let toc_content = dyn_view(move || {
            if book_toc_entries.is_empty() {
                Box::new(inline_message("No table of contents")) as Box<dyn View>
            } else {
                Box::new(toc_view(book_toc_entries.as_ref().clone(), on_toc_click.clone(), active_toc_entry, 0)) as Box<dyn View>
            }
        });
        reader_toc_panel(toc_content)
    };

    let content = reader_split_content(left_toc_panel, body, left_sidebar_width, toc_open);

    Box::new(reader_root((top_bar, settings_dropdown, content)))
}
