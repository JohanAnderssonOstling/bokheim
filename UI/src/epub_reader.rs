use floem::action::exec_after;
use floem::event::EventPropagation;
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::peniko::Color;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use floem::prelude::{button, container, dyn_view, h_stack, label, text_input, v_stack, Decorators, RwSignal, SignalUpdate, View};
use floem::reactive::{create_effect, create_rw_signal, SignalGet};
use floem::style::{AlignItems, JustifyContent};
use floem::views::ScrollExt;
use html_renderer::HTMLRenderer;

use crate::page_navigation::{ReaderState, Signals};
use crate::style::{BORDER_COLOR, DISABLED_BORDER, DISABLED_TEXT, HOVER_BG, PANEL_BG, SUBTLE_TEXT};
use crate::toc::{toc_view, toc_view_flat, TocEntry};

pub fn create_epub_reader(reader_state: &ReaderState, signals: Signals) -> impl View {
    let book_title = reader_state.book_title.clone();
    let lib_handles = signals.library_handles.get_untracked();
    let library = lib_handles.get(&reader_state.library_id).unwrap();
    let book_bytes = library.read_book_bytes(&reader_state.book_id.to_string()).expect("failed to read book bytes");
    let book_position = library.get_book_reading_position(&reader_state.book_id.to_string());
    library.mark_book_opened(&reader_state.book_id.to_string());

    let nav_signals = signals.clone();
    let back_button = button(label(move || "Back"))
        .on_click(move |_| {
            &nav_signals.go_back();
            EventPropagation::Continue
        })
        .style(|s| s.padding(6).border_radius(6).border(1));

    let mut renderer = HTMLRenderer::from_epub_bytes_with_nav(book_bytes, book_position.as_deref());
    let book_toc_entries = renderer.toc().ok().flatten().unwrap_or_default();
    let book_toc_entries = Rc::new(map_toc_entries(&renderer, book_toc_entries));
    let document_toc_source_signal = renderer.document_toc_signal();
    let nav_anchor_signal = renderer.nav_anchor_signal();
    let header_toc_doc_index = create_rw_signal(nav_anchor_signal.get().0);
    let document_toc_entries = create_rw_signal(map_document_toc_entries(document_toc_source_signal.get(), header_toc_doc_index.get()));
    let toc_anchor_strings_by_doc = build_combined_toc_anchor_strings_by_doc(&[book_toc_entries.as_ref(), document_toc_entries.get().as_slice()]);
    renderer.set_toc_anchor_strings_by_doc(toc_anchor_strings_by_doc);
    let cfi_signal = renderer.cfi_position_signal();
    let nav_request_signal = renderer.nav_request_signal();
    let history_nav_signal = renderer.history_nav_signal();
    let can_history_back_signal = renderer.can_history_back_signal();
    let can_history_forward_signal = renderer.can_history_forward_signal();
    let doc_title_signal = renderer.document_title_signal();
    let search_active = renderer.search_active_signal();
    let match_info = renderer.match_info_signal();
    let search_nav = renderer.search_nav_signal();
    let search_query_signal = renderer.search_query_signal();
    let column_width_request_signal = renderer.column_width_request_signal();
    let font_size_request_signal = renderer.font_size_request_signal();
    let scale_request_signal = renderer.scale_request_signal();
    let line_numbers_request_signal = renderer.line_numbers_request_signal();
    let current_column_width_signal = renderer.current_column_width_signal();
    let current_font_size_signal = renderer.current_font_size_signal();
    let current_scale_signal = renderer.current_scale_signal();
    let current_line_numbers_signal = renderer.current_line_numbers_signal();
    let settings_column_width = create_rw_signal(renderer.current_column_width());
    let settings_font_size = create_rw_signal(renderer.current_font_size());
    let settings_scale = create_rw_signal(renderer.current_scale());
    let settings_line_numbers = create_rw_signal(renderer.current_line_numbers_enabled());
    let settings_menu_open = create_rw_signal(false);
    let last_position = create_rw_signal(None::<String>);
    let active_toc_entry = create_rw_signal(None::<(usize, Option<String>)>);

    let book_id = reader_state.book_id;
    let library_id = reader_state.library_id.clone();
    let signals_clone = signals.clone();
    create_effect(move |_| {
        let cfi = cfi_signal.get();
        let Some(cfi) = cfi else {
            return;
        };
        if last_position.get_untracked().as_deref() == Some(cfi.as_str()) {
            return;
        }
        last_position.set(Some(cfi.clone()));
        if let Some(library) = signals_clone.library_handles.get_untracked().get(&library_id) {
            library.update_reading_position(book_id, &cfi);
            library.update_last_read(&book_id.to_string());
        }
    });

    {
        let book_toc_entries = book_toc_entries.clone();
        let document_toc_entries = document_toc_entries;
        let active_toc_entry = active_toc_entry;
        create_effect(move |_| {
            let location = nav_anchor_signal.get();
            let prev = active_toc_entry.get_untracked();
            let mut toc_resolved_set = HashSet::new();
            let mut toc_first_by_doc = HashMap::new();
            build_toc_resolution_maps(book_toc_entries.as_ref(), &mut toc_resolved_set, &mut toc_first_by_doc);
            build_toc_resolution_maps(document_toc_entries.get().as_slice(), &mut toc_resolved_set, &mut toc_first_by_doc);
            let mut next = None;
            if toc_resolved_set.contains(&location) {
                next = Some(location.clone());
            } else if location.1.is_none() {
                next = toc_first_by_doc.get(&location.0).cloned();
            } else if prev.as_ref().map(|(doc, _)| *doc != location.0).unwrap_or(true) {
                next = toc_first_by_doc.get(&location.0).cloned();
            }
            if let Some(next) = next {
                if Some(next.clone()) != prev {
                    active_toc_entry.set(Some(next));
                }
            }
        });
    }

    create_effect(move |_| {
        let current_doc_index = nav_anchor_signal.get().0;
        if header_toc_doc_index.get_untracked() != current_doc_index {
            header_toc_doc_index.set(current_doc_index);
        }
    });

    create_effect(move |_| {
        let raw_entries = document_toc_source_signal.get();
        let current_doc_index = header_toc_doc_index.get();
        let mapped = map_document_toc_entries(raw_entries, current_doc_index);
        if document_toc_entries.get_untracked() != mapped {
            let doc_toc_signal = document_toc_entries;
            exec_after(Duration::from_millis(0), move |_| {
                doc_toc_signal.set(mapped);
            });
        }
    });

    let search_query_buf = create_rw_signal(String::new());

    create_effect(move |_| {
        let q = search_query_buf.get();
        search_query_signal.set(Some(q));
    });

    create_effect(move |_| { settings_column_width.set(current_column_width_signal.get()); });
    create_effect(move |_| { settings_font_size.set(current_font_size_signal.get()); });
    create_effect(move |_| { settings_scale.set(current_scale_signal.get()); });
    create_effect(move |_| { settings_line_numbers.set(current_line_numbers_signal.get()); });

    let search_input = text_input(search_query_buf);
    let input_id = search_input.id();

    create_effect(move |_| {
        if search_active.get() {
            input_id.request_focus();
        }
    });

    let search_bar = {
        let close_search = search_active;
        h_stack((
            search_input
                .on_key_down(Key::Named(NamedKey::Enter), |mods: Modifiers| !mods.shift(), move |_| {
                    search_nav.set(Some(1));
                })
                .on_key_down(Key::Named(NamedKey::Enter), |mods: Modifiers| mods.shift(), move |_| {
                    search_nav.set(Some(-1));
                })
                .style(move |s| {
                    let (_, total) = match_info.get();
                    let no_matches = !search_query_buf.get().is_empty() && total == 0;
                    let base = s.width(240).padding_horiz(6).padding_vert(4).border(1).border_radius(4);
                    if no_matches { base.background(Color::rgb8(255, 210, 210)) } else { base }
                }),
            label(move || {
                let (cur, total) = match_info.get();
                if total == 0 { String::new() } else { format!("{}/{}", cur, total) }
            })
            .style(|s| s.margin_left(8).min_width(40)),
            button(label(|| "<"))
                .on_click(move |_| { search_nav.set(Some(-1)); EventPropagation::Stop })
                .style(|s| s.margin_left(4).padding(4).border(1).border_radius(4)),
            button(label(|| ">"))
                .on_click(move |_| { search_nav.set(Some(1)); EventPropagation::Stop })
                .style(|s| s.margin_left(4).padding(4).border(1).border_radius(4)),
            button(label(|| "✕"))
                .on_click(move |_| {
                    close_search.set(false);
                    search_query_buf.set(String::new());
                    EventPropagation::Stop
                })
                .style(|s| s.margin_left(8).padding(4).border(1).border_radius(4)),
        ))
        .style(|s| s.padding(8).border_top(1).border_color(BORDER_COLOR).align_items(AlignItems::Center).background(PANEL_BG))
    };

    let body = v_stack((
        renderer.style(|s| s.flex_grow(1.0).width_full().min_width(0).min_height(0)),
        search_bar.style(move |s| if search_active.get() { s } else { s.hide() }),
    ))
    .style(|s| s.flex_grow(1.0).width_full().min_width(0).height_full());

    let top_bar = {
        let book_toc_entries = book_toc_entries.clone();
        let document_toc_entries = document_toc_entries;
        let active_toc_entry = active_toc_entry;
        let doc_title_signal = doc_title_signal;
        let settings_menu_open = settings_menu_open;
        h_stack((
            h_stack((
                back_button,
                history_nav_button("←", can_history_back_signal, history_nav_signal, -1)
                    .style(|s| s.margin_left(8)),
                history_nav_button("→", can_history_forward_signal, history_nav_signal, 1)
                    .style(|s| s.margin_left(6)),
                label(move || {
                    let active = active_toc_entry.get();
                    let doc_title = toc_title_for_resolved(book_toc_entries.as_ref(), active.clone())
                        .or_else(|| toc_title_for_resolved(document_toc_entries.get().as_slice(), active))
                        .or_else(|| doc_title_signal.get());
                    if let Some(doc_title) = doc_title {
                        format!("{} • {}", book_title, doc_title)
                    } else {
                        book_title.clone()
                    }
                })
                .style(|s| s.margin_left(10).min_width(0).flex_grow(1.0).text_ellipsis()),
            ))
            .style(|s| s.align_items(AlignItems::Center).min_width(0).flex_grow(1.0)),
            button(label(|| "☰"))
                .on_click(move |_| {
                    settings_menu_open.update(|open| *open = !*open);
                    EventPropagation::Stop
                })
                .style(|s| s.width(34).height(34).border(1).border_radius(6).background(Color::WHITE)),
        ))
        .style(|s| {
            s.width_pct(100.0)
                .min_width(0)
                .height(44)
                .padding_left(16)
                .padding_right(16)
                .border_bottom(1)
                .border_color(BORDER_COLOR)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::SpaceBetween)
        })
    };

    let settings_dropdown = v_stack((
        stepper_row(
            "Column width",
            move || format!("{:.0}px", settings_column_width.get()),
            move || {
                let next = (settings_column_width.get() - 40.0).clamp(200.0, 1200.0);
                settings_column_width.set(next);
                column_width_request_signal.set(Some(next));
            },
            move || {
                let next = (settings_column_width.get() + 40.0).clamp(200.0, 1200.0);
                settings_column_width.set(next);
                column_width_request_signal.set(Some(next));
            },
        ),
        stepper_row(
            "Font size",
            move || format!("{:.0}px", settings_font_size.get()),
            move || {
                let next = (settings_font_size.get() - 1.0).clamp(8.0, 64.0);
                settings_font_size.set(next);
                font_size_request_signal.set(Some(next));
            },
            move || {
                let next = (settings_font_size.get() + 1.0).clamp(8.0, 64.0);
                settings_font_size.set(next);
                font_size_request_signal.set(Some(next));
            },
        ),
        stepper_row(
            "Zoom",
            move || format!("{:.1}x", settings_scale.get()),
            move || {
                let next = (settings_scale.get() - 0.1).clamp(0.5, 3.0);
                settings_scale.set(next);
                scale_request_signal.set(Some(next));
            },
            move || {
                let next = (settings_scale.get() + 0.1).clamp(0.5, 3.0);
                settings_scale.set(next);
                scale_request_signal.set(Some(next));
            },
        ),
        h_stack((
            label(|| "Line numbers").style(|s| s.min_width(120)),
            button(label(move || if settings_line_numbers.get() { "On".to_string() } else { "Off".to_string() }))
                .on_click(move |_| {
                    let next = !settings_line_numbers.get();
                    settings_line_numbers.set(next);
                    line_numbers_request_signal.set(Some(next));
                    EventPropagation::Stop
                })
                .style(|s| s.padding_horiz(10).height(28).border(1).border_radius(4)),
        ))
        .style(|s| s.align_items(AlignItems::Center).gap(8.0)),
        v_stack((
            label(|| "Keyboard shortcuts").style(|s| s.margin_top(6).font_size(13)),
            label(|| "Arrow Up / Down: Previous / Next line").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Mouse wheel: Previous / Next line").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Alt + Arrow Left / Right: Link history back / forward").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "L: Toggle line numbers").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Arrow Left / Right: Previous / Next page").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Ctrl + Arrow Left / Right: Previous / Next chapter").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Ctrl + F: Toggle search").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Enter / Shift+Enter (in search): Next / Previous match").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Esc: Close search").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Ctrl + C: Copy current selection").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Ctrl + +/-: Zoom in / out").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Ctrl + Alt + +/-: Column width +/-").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
            label(|| "Alt + Arrow Up / Down: Font size +/-").style(|s| s.font_size(12).color(SUBTLE_TEXT)),
        ))
        .style(|s| s.gap(4.0)),
    ))
    .style(move |s| if settings_menu_open.get() { s.padding(12).gap(10.0).border_bottom(1).border_color(BORDER_COLOR).background(PANEL_BG) } else { s.hide() });

    let on_toc_click = {
        let nav_request = nav_request_signal;
        Rc::new(move |href: String| {
            nav_request.set(Some(href));
        })
    };

    let left_toc_panel = {
        let book_toc_entries = book_toc_entries.clone();
        let on_toc_click = on_toc_click.clone();
        let toc_content = dyn_view(move || {
            if book_toc_entries.is_empty() {
                Box::new(label(|| "No table of contents").style(|s| s.padding(10))) as Box<dyn View>
            } else {
                Box::new(v_stack((
                    label(|| "Book table of contents").style(|s| s.padding_horiz(10).padding_top(10).font_size(13)),
                    toc_view(book_toc_entries.as_ref().clone(), on_toc_click.clone(), active_toc_entry, 0),
                ))) as Box<dyn View>
            }
        });
        container(toc_content)
            .scroll()
            .style(|s| s.width(280).height_full().border_right(1).flex_shrink(0.0))
    };

    let right_header_panel = {
        let document_toc_entries = document_toc_entries;
        let on_toc_click = on_toc_click.clone();
        let toc_content = dyn_view(move || {
            let entries = document_toc_entries.get();
            if entries.is_empty() {
                Box::new(label(|| "No document headers").style(|s| s.padding(10))) as Box<dyn View>
            } else {
                Box::new(v_stack((
                    label(|| "Document headers").style(|s| s.padding_horiz(8).padding_top(8).font_size(12)),
                    toc_view_flat(entries, on_toc_click.clone(), active_toc_entry, 0),
                ))) as Box<dyn View>
            }
        });
        container(toc_content)
            .scroll()
            .style(|s| s.width(160).height_full().border_left(1).flex_shrink(0.0))
    };

    let content = h_stack((left_toc_panel, body, right_header_panel))
        .style(|s| s.flex_grow(1.0).width_full().min_width(0).height_full().flex_row());

    v_stack((top_bar, settings_dropdown, content))



















































        .style(|s| s.width_full().height_full().flex_grow(1.0).align_items(AlignItems::Stretch))
}

fn history_nav_button(glyph: &'static str, can_nav: RwSignal<bool>, nav_signal: RwSignal<Option<i8>>, step: i8) -> impl View {
    button(label(move || glyph))
        .on_click(move |_| {
            if !can_nav.get() {
                return EventPropagation::Stop;
            }
            nav_signal.set(Some(step));
            EventPropagation::Stop
        })
        .style(move |s| {
            let base = s.width(30).height(30).border(1).border_radius(6);
            if can_nav.get() { base } else { base.color(DISABLED_TEXT).border_color(DISABLED_BORDER).background(HOVER_BG) }
        })
}

fn stepper_row(
    title: &'static str,
    value_label: impl Fn() -> String + 'static,
    on_decrement: impl Fn() + 'static,
    on_increment: impl Fn() + 'static,
) -> impl View {
    h_stack((
        label(move || title).style(|s| s.min_width(120)),
        button(label(|| "−"))
            .on_click(move |_| { on_decrement(); EventPropagation::Stop })
            .style(|s| s.width(28).height(28).border(1).border_radius(4)),
        label(value_label).style(|s| s.min_width(72).justify_center()),
        button(label(|| "+"))
            .on_click(move |_| { on_increment(); EventPropagation::Stop })
            .style(|s| s.width(28).height(28).border(1).border_radius(4)),
    ))
    .style(|s| s.align_items(AlignItems::Center).gap(8.0))
}

fn map_toc_entries_with<F>(entries: Vec<html_renderer::TocEntry>, resolve: &F) -> Vec<TocEntry>
where
    F: Fn(&html_renderer::TocEntry) -> Option<(usize, Option<String>)>,
{
    entries
        .into_iter()
        .map(|entry| {
            let resolved = resolve(&entry);
            TocEntry {
                title: entry.title,
                link: entry.link,
                resolved,
                children: map_toc_entries_with(entry.children, resolve),
            }
        })
        .collect()
}

fn map_toc_entries(renderer: &HTMLRenderer, entries: Vec<html_renderer::TocEntry>) -> Vec<TocEntry> {
    map_toc_entries_with(entries, &|entry| renderer.resolve_href(entry.link.as_str()))
}

fn map_document_toc_entries(entries: Vec<html_renderer::TocEntry>, doc_index: usize) -> Vec<TocEntry> {
    map_toc_entries_with(entries, &|entry| {
        entry.link.strip_prefix('#')
            .map(|fragment| (doc_index, Some(fragment.to_string())))
            .or(Some((doc_index, None)))
    })
}

fn build_toc_resolution_maps(entries: &[TocEntry], resolved_set: &mut HashSet<(usize, Option<String>)>, first_by_doc: &mut HashMap<usize, (usize, Option<String>)>) {
    for entry in entries {
        if let Some(resolved) = entry.resolved.as_ref() {
            resolved_set.insert(resolved.clone());
            first_by_doc.entry(resolved.0).or_insert_with(|| resolved.clone());
        }
        if !entry.children.is_empty() {
            build_toc_resolution_maps(&entry.children, resolved_set, first_by_doc);
        }
    }
}

fn collect_toc_anchors_by_doc(entries: &[TocEntry], map: &mut HashMap<usize, Vec<String>>) {
    for entry in entries {
        if let Some((doc_index, Some(fragment))) = entry.resolved.as_ref() {
            map.entry(*doc_index).or_default().push(fragment.clone());
        }
        collect_toc_anchors_by_doc(&entry.children, map);
    }
}

fn build_toc_anchor_strings_by_doc(entries: &[TocEntry]) -> Vec<Vec<String>> {
    let mut map: HashMap<usize, Vec<String>> = HashMap::new();
    collect_toc_anchors_by_doc(entries, &mut map);
    let max_doc = map.keys().copied().max().unwrap_or(0);
    let mut out = vec![Vec::new(); max_doc + 1];
    for (doc, anchors) in map {
        out[doc] = anchors;
    }
    out
}

fn build_combined_toc_anchor_strings_by_doc(toc_groups: &[&[TocEntry]]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for group in toc_groups {
        let group_anchors = build_toc_anchor_strings_by_doc(group);
        if out.len() < group_anchors.len() {
            out.resize_with(group_anchors.len(), Vec::new);
        }
        for (doc_index, anchors) in group_anchors.into_iter().enumerate() {
            if let Some(bucket) = out.get_mut(doc_index) {
                bucket.extend(anchors);
            }
        }
    }
    out
}

fn toc_title_for_resolved(entries: &[TocEntry], resolved: Option<(usize, Option<String>)>) -> Option<String> {
    let resolved = resolved?;
    toc_title_for_resolved_inner(entries, &resolved)
}

fn toc_title_for_resolved_inner(entries: &[TocEntry], resolved: &(usize, Option<String>)) -> Option<String> {
    for entry in entries {
        if let Some(entry_resolved) = entry.resolved.as_ref() {
            if entry_resolved.0 == resolved.0 && entry_resolved.1 == resolved.1 {
                return Some(entry.title.clone());
            }
        }
        if let Some(found) = toc_title_for_resolved_inner(&entry.children, resolved) {
            return Some(found);
        }
    }
    None
}
