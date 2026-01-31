use crate::components::label_style;
use floem::event::EventPropagation;
use floem::prelude::{
    container, create_rw_signal, dyn_view, h_stack, label, v_stack, Decorators, SignalGet,
    SignalUpdate,
};
use floem::style::{AlignContent, AlignItems, JustifyContent, TextOverflow};
use floem::views::{stack_from_iter, ScrollExt};
use floem::{dyn_view, IntoView, View};
use std::rc::Rc;

#[derive(Clone)]
pub struct TocEntry {
    pub title: String,
    pub link: String, // When Some(link), clicking the title navigates.
    pub resolved: Option<(usize, Option<String>)>,
    pub children: Vec<TocEntry>,
}

pub fn toc_view(
    entries: Vec<TocEntry>,
    on_toc_click: Rc<dyn Fn(String)>,
    active_entry: floem::prelude::RwSignal<Option<(usize, Option<String>)>>,
    level: i32,
) -> impl View {
    stack_from_iter(
        entries
            .into_iter()
            .map(|entry| hierarchical_toc_entry(entry, on_toc_click.clone(), active_entry, level)),
    )
    .style(|s| s.flex_col().height_full().flex_grow(1.0))
}

pub fn hierarchical_toc_entry(
    entry: TocEntry,
    on_toc_click: Rc<dyn Fn(String)>,
    active_entry: floem::prelude::RwSignal<Option<(usize, Option<String>)>>,
    level: i32,
) -> impl View {
    let collapsed = create_rw_signal(true);

    // ── arrow ───────────────────────────────────────────────
    let arrow_button = if !entry.children.is_empty() {
        label(move || if collapsed.get() { "▶" } else { "▼" }.to_string())
            .on_click({
                move |_| {
                    collapsed.update(|c| *c = !*c);
                    EventPropagation::Stop
                }
            })
            .style(|s| {
                s.width(20)
                    .height(40)
                    .padding(0)
                    .align_content(AlignContent::Center)
                    .justify_center()
                    .items_center()
            })
    } else {
        label(move || "").style(|s| s.width(0).height(0).padding(0))
    };
    let arrow_button = label_style(arrow_button, 12);

    // ── title ───────────────────────────────────────────────
    let title_widget = {
        let entry = entry.clone();
        let on_toc_click = on_toc_click.clone();
        let active_entry = active_entry.clone();
        dyn_view(move || {
            let is_active = match (active_entry.get(), entry.resolved.as_ref()) {
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
                    println!("toc click: {}", link);
                    on_toc_click(link.clone());
                    EventPropagation::Stop
                }
            });

            let label_view = label_style(label_view, 16).style(move |s| {
                let s = s
                    .padding(10)
                    .width(260 - level * 30)
                    .min_width(0)
                    .text_overflow(TextOverflow::Wrap);
                if is_active {
                    s.background(floem::peniko::Color::rgb8(240, 244, 255))
                } else {
                    s
                }
            });

            label_view.into_view()
        })
    };

    // ── header row: arrow then title ────────────────────────
    let header = h_stack((arrow_button, title_widget)).style(|s| s.min_width(0));

    // ── children container (unchanged) ─────────────────────
    let children = dyn_view(move || {
        if collapsed.get() {
            container(label(|| "").style(|s| s.width(0).height(0)))
        } else {
            container(toc_view(
                entry.children.clone(),
                on_toc_click.clone(),
                active_entry,
                level + 1,
            ))
            .style(|s| s)
        }
    });

    v_stack((header, children)).style(|s| s.padding_left(30))
}
