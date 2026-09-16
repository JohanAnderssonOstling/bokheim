//! The PDF outline, and which of its entries the current page falls under.
//!
//! A PDF outline is a flat list of entries carrying a depth, which has to be
//! nested into a tree before it can be shown. Deciding which entry the reader
//! is inside is then a "last entry at or before this page" search, with depth
//! and document order breaking ties so a page that several entries could claim
//! resolves to the innermost, latest one.

use gpui_component::tree::TreeItem;
use pdf_reader_core::PdfOutlineEntry;

/// One outline entry, flattened to what the reader needs from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PdfTocTarget {
    id: String,
    title: String,
    page_index: usize,
    depth: usize,
}

#[derive(Default)]
pub(crate) struct PdfTocState {
    targets: Vec<PdfTocTarget>,
    /// The entry the current page sits under, if any.
    active_id: Option<String>,
    /// Whether the outline has been read off the document yet. The PDF reports
    /// its outline only once loading finishes, so this distinguishes "no
    /// outline" from "not asked yet".
    loaded: bool,
}

impl PdfTocState {
    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub(crate) fn replace_targets(&mut self, targets: Vec<PdfTocTarget>) {
        self.targets = targets;
        self.loaded = true;
    }

    /// Changes the active outline entry and reports whether it moved.
    pub(crate) fn set_active(&mut self, active_id: Option<String>) -> bool {
        if self.active_id == active_id {
            return false;
        }
        self.active_id = active_id;
        true
    }

    /// The entry covering `page_index`: the last one at or before that page.
    ///
    /// Ties break towards the deepest entry, then the latest in document
    /// order, so a chapter and the section opening it both starting on one
    /// page resolve to the section.
    pub(crate) fn entry_at(&self, page_index: usize) -> Option<String> {
        self.targets.iter().enumerate().filter(|(_, target)| target.page_index <= page_index).max_by_key(|(order, target)| (target.page_index, target.depth, *order)).map(|(_, target)| target.id.clone())
    }

    pub(crate) fn page_of(&self, id: &str) -> Option<usize> {
        self.targets.iter().find(|target| target.id == id).map(|target| target.page_index)
    }

    /// The title of the active entry, shown in the reader toolbar.
    pub(crate) fn active_title(&self) -> Option<&str> {
        let id = self.active_id.as_ref()?;
        self.targets.iter().find(|target| target.id == *id).map(|target| target.title.as_str())
    }
}

/// A nested outline entry, ready to become a tree item.
#[derive(Clone, Debug)]
pub(crate) struct PdfTocNode {
    id: String,
    title: String,
    page_index: usize,
    depth: usize,
    children: Vec<PdfTocNode>,
}

#[derive(Clone, Debug)]
struct FlatPdfTocItem {
    id: String,
    title: String,
    page_index: usize,
    depth: usize,
}

#[derive(Clone, Debug)]
struct PdfTocArenaNode {
    item: FlatPdfTocItem,
    children: Vec<usize>,
}

/// Flattens a PDF's outline entries and nests them by their reported depth.
pub(crate) fn nest_pdf_outline(outline: &[PdfOutlineEntry]) -> Vec<PdfTocNode> {
    let items = outline.iter().enumerate().map(|(index, entry)| FlatPdfTocItem { id: format!("pdf-toc-{index}"), title: entry.title().to_owned(), page_index: entry.page_index(), depth: entry.depth() }).collect();
    nest_pdf_toc_items(items)
}

fn nest_pdf_toc_items(items: Vec<FlatPdfTocItem>) -> Vec<PdfTocNode> {
    let mut arena = Vec::<PdfTocArenaNode>::with_capacity(items.len());
    let mut roots = Vec::<usize>::new();
    let mut ancestors = Vec::<(usize, usize)>::new();

    for item in items {
        while ancestors.last().is_some_and(|(depth, _)| *depth >= item.depth) {
            ancestors.pop();
        }
        let index = arena.len();
        arena.push(PdfTocArenaNode { item, children: Vec::new() });
        if let Some((_, parent)) = ancestors.last().copied() {
            arena[parent].children.push(index);
        } else {
            roots.push(index);
        }
        let depth = arena[index].item.depth;
        ancestors.push((depth, index));
    }

    fn materialize(index: usize, arena: &[PdfTocArenaNode]) -> PdfTocNode {
        let node = &arena[index];
        PdfTocNode { id: node.item.id.clone(), title: node.item.title.clone(), page_index: node.item.page_index, depth: node.item.depth, children: node.children.iter().map(|child| materialize(*child, arena)).collect() }
    }

    roots.into_iter().map(|root| materialize(root, &arena)).collect()
}

/// Builds the sidebar tree, collecting a flat target list as it goes so page
/// lookups do not have to walk the tree again.
pub(crate) fn pdf_toc_tree_items(nodes: &[PdfTocNode], targets: &mut Vec<PdfTocTarget>) -> Vec<TreeItem> {
    nodes
        .iter()
        .map(|node| {
            targets.push(PdfTocTarget { id: node.id.clone(), title: node.title.clone(), page_index: node.page_index, depth: node.depth });
            TreeItem::new(node.id.clone(), node.title.clone()).children(pdf_toc_tree_items(&node.children, targets))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, title: &str, page_index: usize, depth: usize) -> PdfTocTarget {
        PdfTocTarget { id: id.to_owned(), title: title.to_owned(), page_index, depth }
    }

    fn sample() -> PdfTocState {
        PdfTocState { targets: vec![target("intro", "Introduction", 0, 0), target("method", "Method", 12, 0), target("method-a", "Sampling", 12, 1), target("results", "Results", 40, 0)], active_id: None, loaded: true }
    }

    #[test]
    fn pdf_outline_depths_become_the_same_nested_tree_used_by_epub() {
        let item = |id: &str, title: &str, page_index, depth| FlatPdfTocItem { id: id.to_owned(), title: title.to_owned(), page_index, depth };
        let tree = nest_pdf_toc_items(vec![item("one", "Chapter one", 0, 0), item("one-a", "Section one A", 1, 1), item("one-a-i", "Detail", 2, 2), item("two", "Chapter two", 8, 0), item("two-a", "Section two A", 9, 1)]);

        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].title, "Chapter one");
        assert_eq!(tree[0].children[0].title, "Section one A");
        assert_eq!(tree[0].children[0].children[0].page_index, 2);
        assert_eq!(tree[1].title, "Chapter two");
        assert_eq!(tree[1].children[0].title, "Section two A");
    }

    #[test]
    fn a_page_resolves_to_the_last_entry_at_or_before_it() {
        let toc = sample();
        assert_eq!(toc.entry_at(0).as_deref(), Some("intro"));
        assert_eq!(toc.entry_at(11).as_deref(), Some("intro"), "a page before the next entry stays in the previous section");
        assert_eq!(toc.entry_at(41).as_deref(), Some("results"));
    }

    #[test]
    fn entries_sharing_a_page_resolve_to_the_innermost() {
        let toc = sample();
        assert_eq!(toc.entry_at(12).as_deref(), Some("method-a"), "a section opening its chapter's page wins over the chapter");
    }

    #[test]
    fn a_page_before_the_first_entry_belongs_to_nothing() {
        let toc = PdfTocState { targets: vec![target("later", "Later", 5, 0)], ..PdfTocState::default() };
        assert_eq!(toc.entry_at(0), None);
    }

    #[test]
    fn an_outline_free_pdf_resolves_nothing() {
        let toc = PdfTocState::default();
        assert_eq!(toc.entry_at(3), None);
        assert_eq!(toc.active_title(), None);
    }

    #[test]
    fn the_active_entry_exposes_its_title() {
        let mut toc = sample();
        toc.active_id = Some("method-a".to_owned());
        assert_eq!(toc.active_title(), Some("Sampling"));
    }

    #[test]
    fn a_navigation_target_maps_back_to_its_page() {
        let toc = sample();
        assert_eq!(toc.page_of("results"), Some(40));
        assert_eq!(toc.page_of("missing"), None);
    }
}
