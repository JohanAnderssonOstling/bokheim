use crate::components::empty_view;
use crate::components::text::ui_label;
use crate::style::palette;
use floem::event::EventPropagation;
use floem::prelude::{container, create_rw_signal, dyn_view, h_stack, label, v_stack, Decorators, Label, RwSignal, SignalGet, SignalUpdate};
use floem::style::AlignContent;
use floem::text::Weight;
use floem::views::stack_from_iter;
use floem::View;
use std::collections::HashMap;
use std::rc::Rc;

/// Collapse state keyed by entry link, shared so it survives tree rebuilds
/// (e.g. when a folder is added to the library).
pub type CollapseState = RwSignal<HashMap<String, bool>>;

#[derive(Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub title: String,
    pub link: String, // Clicking the title navigates to this link.
    pub resolved: Option<(usize, Option<String>)>,
    pub children: Vec<TocEntry>,
}

/// Per-tree configuration: what happens on click, how an entry knows it is
/// active, and (optionally) where collapse state is persisted.
#[derive(Clone)]
struct TocTreeCtx {
    on_click: Rc<dyn Fn(String)>,
    is_active: Rc<dyn Fn(&TocEntry) -> bool>,
    collapse_state: Option<CollapseState>,
}

fn resolved_is_active(active_entry: RwSignal<Option<(usize, Option<String>)>>, resolved: Option<&(usize, Option<String>)>) -> bool {
    match (active_entry.try_get().unwrap_or(None), resolved) {
        (Some((active_doc, active_fragment)), Some((doc_index, fragment))) => active_doc == *doc_index && active_fragment.as_ref() == fragment.as_ref(),
        _ => false,
    }
}

fn resolved_matcher(active_entry: RwSignal<Option<(usize, Option<String>)>>) -> Rc<dyn Fn(&TocEntry) -> bool> {
    Rc::new(move |entry| resolved_is_active(active_entry, entry.resolved.as_ref()))
}

fn link_matcher(active_link: RwSignal<Option<String>>) -> Rc<dyn Fn(&TocEntry) -> bool> {
    Rc::new(move |entry| active_link.get().as_deref() == Some(entry.link.as_str()))
}

/// Hierarchical TOC with per-entry collapse state local to the view.
pub fn toc_view(entries: Vec<TocEntry>, on_toc_click: Rc<dyn Fn(String)>, active_entry: RwSignal<Option<(usize, Option<String>)>>, level: i32) -> impl View {
    toc_tree(entries, TocTreeCtx { on_click: on_toc_click, is_active: resolved_matcher(active_entry), collapse_state: None }, level)
}

/// Like `toc_view` but collapse state is persisted in `collapse_state` (keyed by
/// entry link) and the active entry is matched by link string.
pub fn toc_view_with_collapse_state_active_link(entries: Vec<TocEntry>, on_toc_click: Rc<dyn Fn(String)>, active_link: RwSignal<Option<String>>, level: i32, collapse_state: CollapseState) -> impl View {
    toc_tree(entries, TocTreeCtx { on_click: on_toc_click, is_active: link_matcher(active_link), collapse_state: Some(collapse_state) }, level)
}

/// Shared chrome for clickable TOC text: hover highlight + ellipsis on the base label.
fn toc_hover_label(toc_text: Label, font_size: i32) -> Label {
    toc_text.style(move |s| s.hover(|s| s.background(palette().hover_bg)).text_ellipsis().font_size(font_size))
}

/// A TOC row title: hover chrome + row padding + active highlight.
fn toc_title_label(toc_text: Label, font_size: i32, is_active: impl Fn() -> bool + 'static) -> Label {
    toc_hover_label(toc_text, font_size).style(move |s| {
        let s = s.padding(10).flex_grow(1.0).max_width_full().min_width(0).text_ellipsis();
        if is_active() {
            s.background(palette().active_highlight)
        } else {
            s
        }
    })
}

/// Zero-height spacer that indents a TOC row.
fn indent_spacer(width: f32) -> Label {
    label(|| "").style(move |s| s.width(width).height(0).padding(0))
}

fn toc_indent_px(level: i32) -> f32 {
    ((level.max(0) as f32) * 20.0).min(140.0)
}

fn toc_tree(entries: Vec<TocEntry>, ctx: TocTreeCtx, level: i32) -> impl View {
    stack_from_iter(entries.into_iter().map(move |entry| toc_tree_entry(entry, ctx.clone(), level))).style(|s| s.width_full().flex_col().min_width(0))
}

fn toc_tree_entry(entry: TocEntry, ctx: TocTreeCtx, level: i32) -> impl View {
    let initial_collapsed = ctx.collapse_state.as_ref().map(|state| state.get_untracked().get(&entry.link).copied().unwrap_or(true)).unwrap_or(true);
    let collapsed = create_rw_signal(initial_collapsed);
    let has_children = !entry.children.is_empty();
    let children_entries = entry.children.clone();

    let set_collapsed = {
        let collapse_state = ctx.collapse_state;
        let entry_link = entry.link.clone();
        Rc::new(move |value: bool| {
            collapsed.set(value);
            if let Some(state) = collapse_state.as_ref() {
                state.update(|map| {
                    map.insert(entry_link.clone(), value);
                });
            }
        })
    };

    let arrow_button = if has_children {
        let set_collapsed = set_collapsed.clone();
        let arrow = ui_label(move || if collapsed.try_get().unwrap_or(true) { "▶" } else { "▼" }.to_string())
            .on_click(move |_| {
                set_collapsed(!collapsed.get_untracked());
                EventPropagation::Stop
            })
            .style(|s| s.width(20).height(40).padding(0).align_content(AlignContent::Center).justify_center().items_center());
        toc_hover_label(arrow, 12)
    } else {
        empty_view()
    };

    let title_widget = {
        let ctx = ctx.clone();
        dyn_view(move || {
            let is_active = (ctx.is_active)(&entry);
            let title = entry.title.clone();
            let link = entry.link.clone();
            let spacer = indent_spacer(toc_indent_px(level));
            let label_view = ui_label(move || title.clone()).on_click({
                let on_click = ctx.on_click.clone();
                let set_collapsed = set_collapsed.clone();
                move |_| {
                    if has_children {
                        set_collapsed(false);
                    }
                    on_click(link.clone());
                    EventPropagation::Stop
                }
            });
            let title = toc_title_label(label_view, 17, move || is_active).style(|s| s.font_weight(Weight::SEMIBOLD));
            h_stack((spacer, title)).style(|s| s.flex_grow(1.0).min_width(0))
        })
    };

    let header = h_stack((arrow_button, title_widget)).style(|s| s.width_full().min_width(0));

    let children = dyn_view(move || if collapsed.try_get().unwrap_or(true) { container(empty_view()) } else { container(toc_tree(children_entries.clone(), ctx.clone(), level + 1)) });

    v_stack((header, children)).style(|s| s.width_full().min_width(0))
}
