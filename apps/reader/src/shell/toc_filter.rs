//! Filter chapter titles without changing their destinations or document order.

use ui_text::text;

use std::{collections::HashMap, rc::Rc};

use book_model::normalize_search_text;
use gpui::prelude::*;
use gpui::{Context, Entity, Render, SharedString, Window, div, px};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::tree::{TreeItem, TreeState};
use ui_components as components;

struct TocEntryMetadata {
    search_title: String,
}

#[derive(Default)]
pub(crate) struct TocIndex {
    entries: HashMap<SharedString, TocEntryMetadata>,
    pub(crate) nested: bool,
}

impl TocIndex {
    fn new(items: &[TreeItem]) -> Self {
        fn visit(items: &[TreeItem], index: &mut TocIndex) {
            for item in items {
                index.entries.insert(item.id.clone(), TocEntryMetadata { search_title: normalize_search_text(&item.label) });
                index.nested |= !item.children.is_empty();
                visit(&item.children, index);
            }
        }
        let mut index = Self::default();
        visit(items, &mut index);
        index
    }

    fn matches(&self, id: &str, query: &str) -> bool {
        self.entries.get(id).is_some_and(|entry| entry.search_title.contains(query))
    }
}

pub(crate) struct TocFilter {
    input: Entity<InputState>,
    reset: Option<components::ContentsReadReset>,
    tree: Entity<TreeState>,
    roots: Vec<TreeItem>,
    query: String,
    index: Rc<TocIndex>,
    unfiltered_selection: Option<TreeItem>,
}

impl TocFilter {
    pub(crate) fn new(tree: Entity<TreeState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = components::contents_filter_input(window, cx);
        cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => this.set_query(&input.read(cx).value(), cx),
            InputEvent::PressEnter { .. } => this.tree.update(cx, |tree, cx| tree.focus(window, cx)),
            _ => {}
        })
        .detach();
        Self { input, reset: None, tree, roots: Vec::new(), query: String::new(), index: Rc::default(), unfiltered_selection: None }
    }

    pub(crate) fn on_reset(mut self, reset: components::ContentsReadReset) -> Self {
        self.reset = Some(reset);
        self
    }

    pub(crate) fn index(&self) -> Rc<TocIndex> {
        self.index.clone()
    }

    pub(crate) fn set_items(&mut self, items: Vec<TreeItem>, cx: &mut Context<Self>) {
        self.index = Rc::new(TocIndex::new(&items));
        self.roots = items;
        let selected = self.tree.read(cx).selected_item().cloned();
        self.rebuild(selected, cx);
    }

    fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = normalize_search_text(query);
        if self.query == query {
            return;
        }
        let selected = self.tree.read(cx).selected_item().cloned();
        if self.query.is_empty() {
            self.unfiltered_selection = selected.clone();
        }
        self.query = query;
        let selected = if self.query.is_empty() { self.unfiltered_selection.take() } else { selected };
        self.rebuild(selected, cx);
    }

    fn rebuild(&mut self, selected: Option<TreeItem>, cx: &mut Context<Self>) {
        let items = if self.query.is_empty() { self.roots.clone() } else { filtered_items(&self.roots, &self.query, &self.index) };
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            tree.set_selected_item(selected.as_ref(), cx);
            if tree.selected_index().is_none() && !self.query.is_empty() {
                let first_match = tree.entries().iter().position(|entry| !entry.is_disabled() && self.index.matches(&entry.item().id, &self.query));
                tree.set_selected_index(first_match, cx);
            }
        });
        cx.notify();
    }
}

/// Fresh item states expand matching descendants without unfolding the saved
/// unfiltered tree. IDs, disabled headings, and hierarchy remain intact.
fn filtered_items(items: &[TreeItem], query: &str, index: &TocIndex) -> Vec<TreeItem> {
    items
        .iter()
        .filter_map(|item| {
            let children = filtered_items(&item.children, query, index);
            if children.is_empty() && !index.matches(&item.id, query) {
                return None;
            }
            Some(TreeItem::new(item.id.clone(), item.label.clone()).disabled(item.is_disabled()).expanded(true).children(children))
        })
        .collect()
}

impl Render for TocFilter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = components::browser_theme(cx);
        let reset = self.reset.clone().map(|reset| components::contents_read_reset_button("reset-chapter-read-status").on_click(move |event, window, cx| reset(event, window, cx)));
        let filter = components::contents_filter_bar(&self.input, reset, theme);
        div().flex_none().w_full().child(filter).when(!self.query.is_empty() && self.tree.read(cx).entries().is_empty(), |panel| {
            panel.child(components::reader_state_note(text!(reader::no_chapters_match_your_filter), theme).debug_selector(|| "reader-toc-filter-empty".into()).p(px(12.0)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::state_panel::reader_toc_tree;
    use gpui::{TestAppContext, VisualTestContext};

    fn chapters() -> Vec<TreeItem> {
        vec![
            TreeItem::new("part", "Part one").disabled(true).children([TreeItem::new("chapter-one.xhtml", "A distant beginning"), TreeItem::new("subpart", "Subpart").child(TreeItem::new("chapter-two.xhtml#cafe", "Café society"))]),
            TreeItem::new("appendix.xhtml", "Appendix"),
        ]
    }

    #[test]
    fn outline_index_stores_normalized_titles_and_filtering_keeps_display_text() {
        let items = chapters();
        let index = TocIndex::new(&items);
        assert!(index.matches("chapter-two.xhtml#cafe", "cafe society"));
        assert!(!index.matches("chapter-one.xhtml", "cafe"));
        let filtered = filtered_items(&items, "cafe", &index);
        let chapter = &filtered[0].children[0].children[0];
        assert_eq!(chapter.id, "chapter-two.xhtml#cafe");
        assert_eq!(chapter.label, "Café society");
    }

    struct FilterTest {
        tree: Entity<TreeState>,
        filter: Entity<TocFilter>,
        navigated: Option<String>,
    }

    impl Render for FilterTest {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = components::browser_theme(cx);
            let tree = reader_toc_tree(
                &self.tree,
                Some(self.filter.read(cx).index()),
                &cx.entity(),
                |_, _| Default::default(),
                None,
                true,
                theme,
                cx,
                |id, _, _, this: &mut Self, cx| {
                    this.navigated = Some(id.to_owned());
                    cx.notify();
                },
            );
            components::reader_toc(theme).w(px(300.0)).h(px(420.0)).child(self.filter.clone()).child(tree)
        }
    }

    fn setup(cx: &mut TestAppContext) -> (Entity<FilterTest>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        let (root, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let tree = cx.new(|cx| TreeState::new(cx));
                let filter = cx.new(|cx| TocFilter::new(tree.clone(), window, cx));
                filter.update(cx, |filter, cx| filter.set_items(chapters(), cx));
                tree.update(cx, |tree, cx| tree.set_selected_index(Some(1), cx));
                FilterTest { tree, filter, navigated: None }
            });
            gpui_component::Root::new(view, window, cx)
        });
        let view = visual.update(|_, cx| root.read(cx).view().clone().downcast::<FilterTest>().ok().unwrap());
        (view, visual)
    }

    fn set_query(view: &Entity<FilterTest>, value: &str, cx: &mut VisualTestContext) {
        view.update_in(cx, |view, window, cx| {
            let input = view.filter.read(cx).input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        cx.simulate_keystrokes("secondary-a backspace");
        if !value.is_empty() {
            cx.simulate_input(value);
        }
        cx.run_until_parked();
    }

    #[gpui::test]
    fn filtering_reveals_matching_descendants_and_restores_folds_and_selection(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        set_query(&view, " CAFÉ ", cx);
        view.read_with(cx, |view, cx| {
            let tree = view.tree.read(cx);
            let ids = tree.entries().iter().map(|entry| entry.item().id.as_ref()).collect::<Vec<_>>();
            assert_eq!(ids, ["part", "subpart", "chapter-two.xhtml#cafe"]);
            assert_eq!(tree.selected_item().unwrap().id, "chapter-two.xhtml#cafe");
            assert!(tree.entries()[0].is_disabled());
            assert!(!view.filter.read(cx).roots[0].is_expanded());
        });
        set_query(&view, "not a chapter", cx);
        view.read_with(cx, |view, cx| assert!(view.tree.read(cx).entries().is_empty()));
        assert!(cx.debug_bounds("reader-toc-filter-empty").is_some());
        set_query(&view, "", cx);
        view.read_with(cx, |view, cx| {
            let tree = view.tree.read(cx);
            assert_eq!(tree.entries().len(), 2);
            assert!(!tree.entries()[0].is_expanded());
            assert_eq!(tree.selected_item().unwrap().id, "appendix.xhtml");
        });
        assert!(cx.debug_bounds("reader-toc-filter-empty").is_none());
    }

    #[gpui::test]
    fn typing_then_enter_keeps_original_chapter_navigation(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        view.update_in(cx, |view, window, cx| {
            let input = view.filter.read(cx).input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        cx.simulate_input("cafe");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert_eq!(view.navigated.as_deref(), Some("chapter-two.xhtml#cafe")));
    }

    #[gpui::test]
    fn positional_ids_keep_duplicate_chapters_distinct_when_filtered(cx: &mut TestAppContext) {
        use crate::epub::toc::TocState;
        use book_content::BookToc;
        use book_model::BookTocEntry;

        let mut toc = TocState::default();
        let chapter = || BookTocEntry { title: "Chapter".into(), target: "shared.xhtml".into(), children: vec![] };
        let entries = vec![BookTocEntry { title: "Part".into(), target: "shared.xhtml".into(), children: vec![chapter(), chapter()] }, BookTocEntry { title: "Appendix".into(), target: "appendix.xhtml".into(), children: vec![] }];
        let resolutions = BookToc::resolve(&entries, |_| Some((0, None)));
        toc.set(entries, resolutions);
        let (view, cx) = setup(cx);
        view.update(cx, |view, cx| view.filter.update(cx, |filter, cx| filter.set_items(toc.tree_items(), cx)));
        set_query(&view, "chapter", cx);
        view.read_with(cx, |view, cx| {
            let tree = view.tree.read(cx);
            let ids = tree.entries().iter().map(|entry| entry.item().id.as_ref()).collect::<Vec<_>>();
            assert_eq!(ids, ["epub-toc-0", "epub-toc-1", "epub-toc-2"]);
        });
        cx.simulate_keystrokes("enter down enter");
        view.read_with(cx, |view, _| {
            assert_eq!(view.navigated.as_deref(), Some("epub-toc-2"));
            assert_eq!(toc.link_for_id(view.navigated.as_deref().unwrap()), Some("shared.xhtml"));
        });
        view.update(cx, |view, _| view.navigated = None);
        let second = cx.debug_bounds("reader-toc-title-2").unwrap();
        cx.simulate_click(second.center(), Default::default());
        view.read_with(cx, |view, _| assert_eq!(view.navigated.as_deref(), Some("epub-toc-2")));
        set_query(&view, "", cx);
        view.read_with(cx, |view, cx| {
            let tree = view.tree.read(cx);
            assert_eq!(tree.entries().len(), 2);
            assert!(!tree.entries()[0].is_expanded());
        });
    }

    #[gpui::test]
    fn replacing_outline_reapplies_the_existing_filter(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        set_query(&view, "cafe", cx);
        view.update(cx, |view, cx| {
            view.filter.update(cx, |filter, cx| {
                filter.set_items(vec![TreeItem::new("chapter-two.xhtml#cafe", "Renamed chapter"), TreeItem::new("updated.pdf#page=12", "Café after revision"), TreeItem::new("other", "Other chapter")], cx);
            });
        });
        view.read_with(cx, |view, cx| {
            let tree = view.tree.read(cx);
            assert_eq!(tree.entries().len(), 1);
            assert_eq!(tree.selected_item().unwrap().id, "updated.pdf#page=12");
        });
    }
}
