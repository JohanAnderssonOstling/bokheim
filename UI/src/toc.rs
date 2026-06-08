use crate::style::{label_style, palette};
use floem::event::EventPropagation;
use floem::prelude::{container, create_rw_signal, dyn_view, h_stack, label, v_stack, Decorators, RwSignal, SignalGet, SignalUpdate};
use floem::style::AlignContent;
use floem::text::Weight;
use floem::views::stack_from_iter;
use floem::View;
use std::collections::HashMap;
use std::rc::Rc;

pub type CollapseState = RwSignal<HashMap<String, bool>>;

#[derive(Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub title: String,
    pub link: String, // When Some(link), clicking the title navigates.
    pub resolved: Option<(usize, Option<String>)>,
    pub children: Vec<TocEntry>,
}

#[derive(Clone)]
struct FlatTocEntry {
    title: String,
    link: String,
    resolved: Option<(usize, Option<String>)>,
    level: i32,
}

fn toc_indent_px(level: i32) -> f32 {
    ((level.max(0) as f32) * 20.0).min(140.0)
}

fn flatten_entries(entries: &[TocEntry], level: i32, out: &mut Vec<FlatTocEntry>) {
    for entry in entries {
        out.push(FlatTocEntry { title: entry.title.clone(), link: entry.link.clone(), resolved: entry.resolved.clone(), level });
        if !entry.children.is_empty() {
            flatten_entries(&entry.children, level + 1, out);
        }
    }
}

pub fn toc_view(entries: Vec<TocEntry>, on_toc_click: Rc<dyn Fn(String)>, active_entry: floem::prelude::RwSignal<Option<(usize, Option<String>)>>, level: i32) -> impl View {
    stack_from_iter(entries.into_iter().map(|entry| hierarchical_toc_entry(entry, on_toc_click.clone(), active_entry, level))).style(|s| s.flex_col().min_width(0))
}

pub fn toc_view_flat(entries: Vec<TocEntry>, on_toc_click: Rc<dyn Fn(String)>, active_entry: floem::prelude::RwSignal<Option<(usize, Option<String>)>>, level: i32) -> impl View {
    let mut flat_entries = Vec::new();
    flatten_entries(&entries, level, &mut flat_entries);
    stack_from_iter(flat_entries.into_iter().map(move |entry| {
        let on_toc_click = on_toc_click.clone();
        let active_entry = active_entry;
        let is_active = move || match (active_entry.try_get().unwrap_or(None), entry.resolved.as_ref()) {
            (Some((active_doc, active_fragment)), Some((doc_index, fragment))) => active_doc == *doc_index && active_fragment.as_ref() == fragment.as_ref(),
            _ => false,
        };
        let link = entry.link.clone();
            label_style(
                label(move || entry.title.clone()).on_click(move |_| {
                    on_toc_click(link.clone());
                    EventPropagation::Stop
            }),
            16,
        )
        .style(move |s| {
            let s = s
                .padding(10)
                .padding_left(10.0 + toc_indent_px(entry.level))
                .width_full()
                .max_width_full()
                .min_width(0)
                .text_ellipsis();
            if is_active() {
                s.background(palette().active_highlight)
            } else {
                s
            }
        })
    }))
    .style(|s| s.flex_col().min_width(0))
}

pub fn hierarchical_toc_entry(entry: TocEntry, on_toc_click: Rc<dyn Fn(String)>, active_entry: floem::prelude::RwSignal<Option<(usize, Option<String>)>>, level: i32) -> impl View {
    let collapsed = create_rw_signal(true);

    let arrow_button = if !entry.children.is_empty() {
        label(move || if collapsed.try_get().unwrap_or(true) { "▶" } else { "▼" }.to_string())
            .on_click({
                move |_| {
                    collapsed.update(|c| *c = !*c);
                    EventPropagation::Stop
                }
            })
            .style(|s| s.width(20).height(40).padding(0).align_content(AlignContent::Center).justify_center().items_center())
    } else {
        label(move || "").style(|s| s.width(0).height(0).padding(0))
    };
    let arrow_button = label_style(arrow_button, 12);

    let title_widget = {
        let entry = entry.clone();
        let on_toc_click = on_toc_click.clone();
        dyn_view(move || {
            let is_active = match (active_entry.try_get().unwrap_or(None), entry.resolved.as_ref()) {
                (Some((active_doc, active_fragment)), Some((doc_index, fragment))) => active_doc == *doc_index && active_fragment.as_ref() == fragment.as_ref(),
                _ => false,
            };
            let title = entry.title.clone();
            let link = entry.link.clone();
            let label_view = label(move || title.clone()).on_click({
                let on_toc_click = on_toc_click.clone();
                move |_| {
                    on_toc_click(link.clone());
                    EventPropagation::Stop
                }
            });

            label_style(label_view, 17).style(move |s| {
                let s = s
                    .padding(10)
                    .padding_left(toc_indent_px(level))
                    .font_weight(Weight::SEMIBOLD)
                    .flex_grow(1.0)
                    .max_width_full()
                    .min_width(0)
                    .text_ellipsis();
                if is_active {
                    s.background(palette().active_highlight)
                } else {
                    s
                }
            })
        })
    };

    let header = h_stack((arrow_button, title_widget)).style(|s| s.width_full().min_width(0));

    let children = dyn_view(move || if collapsed.try_get().unwrap_or(true) { container(label(|| "").style(|s| s.width(0).height(0))) } else { container(toc_view(entry.children.clone(), on_toc_click.clone(), active_entry, level + 1)) });

    v_stack((header, children)).style(|s| s.width_full().min_width(0))
}

/// Like `toc_view` but collapse state is looked up from `collapse_state` by entry link,
/// so it survives tree rebuilds (e.g. when a folder is added to the library).
pub fn toc_view_with_collapse_state(
    entries: Vec<TocEntry>,
    on_toc_click: Rc<dyn Fn(String)>,
    active_entry: RwSignal<Option<(usize, Option<String>)>>,
    level: i32,
    collapse_state: CollapseState,
) -> impl View {
    stack_from_iter(entries.into_iter().map(|entry| {
        hierarchical_toc_entry_with_state(entry, on_toc_click.clone(), active_entry, level, collapse_state.clone())
    }))
    .style(|s| s.flex_col().min_width(0))
}

pub fn toc_view_with_collapse_state_active_link(
    entries: Vec<TocEntry>,
    on_toc_click: Rc<dyn Fn(String)>,
    active_link: RwSignal<Option<String>>,
    level: i32,
    collapse_state: CollapseState,
) -> impl View {
    stack_from_iter(entries.into_iter().map(|entry| {
        hierarchical_toc_entry_with_state_active_link(entry, on_toc_click.clone(), active_link, level, collapse_state.clone())
    }))
    .style(|s| s.flex_col().min_width(0))
}

fn hierarchical_toc_entry_with_state(
    entry: TocEntry,
    on_toc_click: Rc<dyn Fn(String)>,
    active_entry: RwSignal<Option<(usize, Option<String>)>>,
    level: i32,
    collapse_state: CollapseState,
) -> impl View {
    let entry_link = entry.link.clone();
    let initial_collapsed = collapse_state
        .get_untracked()
        .get(&entry_link)
        .copied()
        .unwrap_or(true);
    let collapsed = create_rw_signal(initial_collapsed);

    let arrow_button = if !entry.children.is_empty() {
        label(move || if collapsed.get() { "▶" } else { "▼" }.to_string())
            .on_click(move |_| {
                let next = !collapsed.get_untracked();
                collapsed.set(next);
                collapse_state.update(|state| {
                    state.insert(entry_link.clone(), next);
                });
                EventPropagation::Stop
            })
            .style(|s| s.width(20).height(40).padding(0).align_content(AlignContent::Center).justify_center().items_center())
    } else {
        label(move || "").style(|s| s.width(0).height(0).padding(0))
    };
    let arrow_button = label_style(arrow_button, 12);

    let title_widget = {
        let entry = entry.clone();
        let on_toc_click = on_toc_click.clone();
        dyn_view(move || {
            let is_active = match (active_entry.try_get().unwrap_or(None), entry.resolved.as_ref()) {
                (Some((active_doc, active_fragment)), Some((doc_index, fragment))) => {
                    active_doc == *doc_index && active_fragment.as_ref() == fragment.as_ref()
                }
                _ => false,
            };
            let title = entry.title.clone();
            let link = entry.link.clone();
            let label_view = label(move || title.clone()).on_click({
                let on_toc_click = on_toc_click.clone();
                move |_| {
                    on_toc_click(link.clone());
                    EventPropagation::Stop
                }
            });
            label_style(label_view, 17).style(move |s| {
                let s = s.padding(10).padding_left(toc_indent_px(level)).font_weight(Weight::SEMIBOLD).flex_grow(1.0).max_width_full().min_width(0).text_ellipsis();
                if is_active { s.background(palette().active_highlight) } else { s }
            })
        })
    };

    let header = h_stack((arrow_button, title_widget)).style(|s| s.width_full().min_width(0));

    let children = dyn_view(move || {
        if collapsed.get() {
            container(label(|| "").style(|s| s.width(0).height(0)))
        } else {
            container(toc_view_with_collapse_state(
                entry.children.clone(),
                on_toc_click.clone(),
                active_entry,
                level + 1,
                collapse_state.clone(),
            ))
        }
    });

    v_stack((header, children)).style(|s| s.width_full().min_width(0))
}

fn hierarchical_toc_entry_with_state_active_link(
    entry: TocEntry,
    on_toc_click: Rc<dyn Fn(String)>,
    active_link: RwSignal<Option<String>>,
    level: i32,
    collapse_state: CollapseState,
) -> impl View {
    let entry_link = entry.link.clone();
    let initial_collapsed = collapse_state
        .get_untracked()
        .get(&entry_link)
        .copied()
        .unwrap_or(true);
    let collapsed = create_rw_signal(initial_collapsed);

    let arrow_button = if !entry.children.is_empty() {
        label(move || if collapsed.get() { "▶" } else { "▼" }.to_string())
            .on_click(move |_| {
                let next = !collapsed.get_untracked();
                collapsed.set(next);
                collapse_state.update(|state| {
                    state.insert(entry_link.clone(), next);
                });
                EventPropagation::Stop
            })
            .style(|s| s.width(20).height(40).padding(0).align_content(AlignContent::Center).justify_center().items_center())
    } else {
        label(move || "").style(|s| s.width(0).height(0).padding(0))
    };
    let arrow_button = label_style(arrow_button, 12);

    let title_widget = {
        let entry = entry.clone();
        let on_toc_click = on_toc_click.clone();
        dyn_view(move || {
            let is_active = active_link.get().as_deref() == Some(entry.link.as_str());
            let title = entry.title.clone();
            let link = entry.link.clone();
            let label_view = label(move || title.clone()).on_click({
                let on_toc_click = on_toc_click.clone();
                move |_| {
                    on_toc_click(link.clone());
                    EventPropagation::Stop
                }
            });
            label_style(label_view, 17).style(move |s| {
                let s = s.padding(10).padding_left(toc_indent_px(level)).font_weight(Weight::SEMIBOLD).flex_grow(1.0).max_width_full().min_width(0).text_ellipsis();
                if is_active { s.background(palette().active_highlight) } else { s }
            })
        })
    };

    let header = h_stack((arrow_button, title_widget)).style(|s| s.width_full().min_width(0));

    let children = dyn_view(move || {
        if collapsed.get() {
            container(label(|| "").style(|s| s.width(0).height(0)))
        } else {
            container(toc_view_with_collapse_state_active_link(
                entry.children.clone(),
                on_toc_click.clone(),
                active_link,
                level + 1,
                collapse_state.clone(),
            ))
        }
    });

    v_stack((header, children)).style(|s| s.width_full().min_width(0))
}
