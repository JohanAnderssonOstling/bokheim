//! Chapter membership changes with bookmark data; filters and folds only
//! change which rows of that model are visible.

use ui_text::text;

use gpui::{AnyElement, Entity, SharedString};
use library_backend::Annotation;
use ui_components as components;

use crate::shell::annotation_tree::{AnnotationBranch, AnnotationTree, AnnotationTreeRow, AnnotationTreeState, AnnotationTreeTarget};
use crate::shell::sidebar_list::SidebarList;
use crate::shell::state_panel::AnnotationRow;

struct BookmarkEntry {
    id: SharedString,
    modified_at: u64,
    search_name: String,
}

impl BookmarkEntry {
    fn row(&self, index: usize, depth: usize) -> AnnotationRow {
        AnnotationRow::Card { index, depth, id: self.id.clone(), modified_at: self.modified_at }
    }
}

#[derive(Default)]
struct BookmarkFilter {
    entries: Vec<BookmarkEntry>,
    query: String,
    matches: Vec<bool>,
}

impl BookmarkFilter {
    fn replace(&mut self, bookmarks: &[Annotation], query: &str) {
        self.entries = bookmarks.iter().map(|bookmark| BookmarkEntry { id: bookmark.id.clone().into(), modified_at: bookmark.modified_at, search_name: bookmark.note.to_lowercase() }).collect();
        self.query = query.trim().to_lowercase();
        self.match_names();
    }

    fn set_query(&mut self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if self.query == query {
            return false;
        }
        self.query = query;
        self.match_names();
        true
    }

    fn match_names(&mut self) {
        self.matches = self.entries.iter().map(|entry| entry.search_name.contains(&self.query)).collect();
    }

    fn empty_message(&self) -> &'static str {
        if self.query.is_empty() { text!(reader::no_bookmarks_yet) } else { text!(reader::no_bookmarks_match_your_filter) }
    }
}

#[derive(Default)]
pub(crate) struct BookmarkTree {
    hierarchy: AnnotationTree,
    filter: BookmarkFilter,
    pub(crate) state: AnnotationTreeState,
}

impl BookmarkTree {
    pub(crate) fn replace(&mut self, hierarchy: AnnotationTree, bookmarks: &[Annotation], query: &str, selected: Option<&str>) {
        self.hierarchy = hierarchy;
        self.filter.replace(bookmarks, query);
        self.state.set_filter(&self.filter.query);
        self.reveal_ancestors(selected);
        self.prepare(selected);
    }

    pub(crate) fn filter(&mut self, query: &str, selected: Option<&str>) {
        if self.filter.set_query(query) {
            self.state.set_filter(&self.filter.query);
            self.prepare(selected);
        }
    }

    pub(crate) fn set_expanded(&mut self, id: usize, expanded: bool, selected: Option<&str>) {
        if self.state.expanded(id) != expanded {
            self.state.set_expanded(id, expanded);
            self.prepare(selected);
        }
    }

    pub(crate) fn select(&mut self, selected: Option<&str>) {
        if self.reveal_ancestors(selected) {
            self.prepare(selected);
        } else {
            self.reveal_row(selected);
        }
    }

    fn reveal_ancestors(&mut self, selected: Option<&str>) -> bool {
        let index = selected.and_then(|id| self.filter.entries.iter().position(|bookmark| bookmark.id.as_ref() == id)).filter(|&index| self.filter.matches[index]);
        self.state.reveal_selected(&self.hierarchy, selected, index)
    }

    fn reveal_row(&mut self, selected: Option<&str>) {
        let row = selected.and_then(|id| self.state.list.rows.iter().position(|row| matches!(row, AnnotationTreeRow::Annotation(row) if row.annotation_id() == Some(id))));
        self.state.list.reveal_selected(selected, row);
    }

    fn prepare(&mut self, selected: Option<&str>) {
        // Visit each bookmarked branch once, including folded descendants
        // when deciding whether a filtered heading has any matches.
        fn visit(branches: &[AnnotationBranch], state: &AnnotationTreeState, filter: &BookmarkFilter, depth: usize, draw: bool, rows: &mut Vec<AnnotationTreeRow>) -> bool {
            // The stored hierarchy contains only chapters with bookmarks.
            if !draw && filter.query.is_empty() {
                return !branches.is_empty();
            }
            let matches = &filter.matches;
            let mut any_match = false;
            for branch in branches {
                let start = rows.len();
                let expanded = state.expanded(branch.id);
                if draw {
                    rows.push(AnnotationTreeRow::Branch { id: branch.id, title: String::new(), depth, expanded });
                    if expanded {
                        rows.extend(branch.annotations.iter().filter(|&&index| matches[index]).map(|&index| AnnotationTreeRow::Annotation(filter.entries[index].row(index, depth + 1))));
                    }
                }
                let children_match = visit(&branch.children, state, filter, depth + 1, draw && expanded, rows);
                let own_match = branch.annotations.iter().any(|&index| matches[index]);
                if own_match || children_match {
                    if draw {
                        if let AnnotationTreeRow::Branch { title, .. } = &mut rows[start] {
                            title.clone_from(&branch.title);
                        }
                    }
                    any_match = true;
                } else {
                    rows.truncate(start);
                }
            }
            any_match
        }

        let mut rows = Vec::new();
        visit(&self.hierarchy.branches, &self.state, &self.filter, 0, true, &mut rows);
        if self.hierarchy.other.iter().any(|&index| self.filter.matches[index]) {
            rows.push(AnnotationTreeRow::Annotation(AnnotationRow::Heading(text!(reader::other_bookmarks).into())));
            rows.extend(self.hierarchy.other.iter().filter(|&&index| self.filter.matches[index]).map(|&index| AnnotationTreeRow::Annotation(self.filter.entries[index].row(index, 0))));
        }
        if rows.is_empty() {
            rows.push(AnnotationTreeRow::Annotation(AnnotationRow::Empty(self.filter.empty_message())));
        }
        self.state.list.set_rows(rows);
        self.reveal_row(selected);
    }

    pub(crate) fn render_row<C: AnnotationTreeTarget>(&self, target: &Entity<C>, index: usize, bookmarks: &[Annotation], selected: Option<&str>, theme: components::BrowserTheme) -> AnyElement {
        self.state.render_row(target, index, bookmarks, selected, theme)
    }
}

#[derive(Default)]
pub(crate) struct BookmarkList {
    groups: Vec<AnnotationRow>,
    filter: BookmarkFilter,
    pub(crate) list: SidebarList<AnnotationRow>,
}

impl BookmarkList {
    pub(crate) fn replace(&mut self, groups: Vec<AnnotationRow>, bookmarks: &[Annotation], query: &str, selected: Option<&str>) {
        self.groups = groups;
        self.filter.replace(bookmarks, query);
        self.prepare(selected);
    }

    pub(crate) fn filter(&mut self, query: &str, selected: Option<&str>) {
        if self.filter.set_query(query) {
            self.prepare(selected);
        }
    }

    pub(crate) fn select(&mut self, selected: Option<&str>) {
        let row = selected.and_then(|id| self.list.rows.iter().position(|row| row.annotation_id() == Some(id)));
        self.list.reveal_selected(selected, row);
    }

    fn prepare(&mut self, selected: Option<&str>) {
        let mut rows = Vec::new();
        let mut heading = None;
        for row in &self.groups {
            match row {
                AnnotationRow::Heading(_) => heading = Some(row),
                AnnotationRow::Card { index, .. } if self.filter.matches[*index] => {
                    if let Some(heading) = heading.take() {
                        rows.push(heading.clone());
                    }
                    rows.push(row.clone());
                }
                _ => {}
            }
        }
        if rows.is_empty() {
            rows.push(AnnotationRow::Empty(self.filter.empty_message()));
        }
        self.list.set_rows(rows);
        self.select(selected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::AnnotationAnchor;
    use content_address::BookId;

    use crate::shell::bookmarks::bookmark;

    fn bookmarks() -> Vec<Annotation> {
        ["First place", "Second place"]
            .into_iter()
            .enumerate()
            .map(|(ordinal, name)| {
                let mut bookmark = bookmark(BookId::from_bytes([1; 32]), AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"), ordinal, 0.0);
                bookmark.note = name.into();
                bookmark
            })
            .collect()
    }

    fn hierarchy() -> AnnotationTree {
        AnnotationTree {
            branches: vec![AnnotationBranch {
                id: 0,
                title: "Part".into(),
                annotations: vec![],
                children: vec![AnnotationBranch { id: 1, title: "One".into(), annotations: vec![0], children: vec![] }, AnnotationBranch { id: 2, title: "Two".into(), annotations: vec![1], children: vec![] }],
            }],
            other: vec![],
        }
    }

    fn cards(tree: &BookmarkTree) -> Vec<&str> {
        tree.state
            .list
            .rows
            .iter()
            .filter_map(|row| match row {
                AnnotationTreeRow::Annotation(row) => row.annotation_id(),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tree_filter_preserves_indices_and_folds_and_selection_reveals_ancestors() {
        let bookmarks = bookmarks();
        let first = bookmarks[0].id.as_str();
        let second = bookmarks[1].id.as_str();
        let mut tree = BookmarkTree::default();
        tree.replace(hierarchy(), &bookmarks, "", None);
        tree.set_expanded(1, false, None);
        assert_eq!(cards(&tree), vec![second]);
        tree.filter(" FIRST ", None);
        assert_eq!(cards(&tree), vec![first]);
        assert!(!tree.state.list.rows.iter().any(|row| matches!(row, AnnotationTreeRow::Branch { id: 2, .. })));
        tree.set_expanded(1, false, None);
        assert!(cards(&tree).is_empty());
        assert!(!tree.state.list.rows.iter().any(|row| matches!(row, AnnotationTreeRow::Annotation(AnnotationRow::Empty(_)))));
        tree.filter("", None);
        assert_eq!(cards(&tree), vec![second]);
        tree.select(Some(first));
        assert_eq!(cards(&tree), vec![first, second]);
        tree.set_expanded(1, false, Some(first));
        tree.select(Some(first));
        assert_eq!(cards(&tree), vec![second], "reselecting must respect a manual fold");
        tree.filter("absent", None);
        assert_eq!(tree.state.list.rows, vec![AnnotationTreeRow::Annotation(AnnotationRow::Empty("No bookmarks match your filter."))]);
    }

    #[test]
    fn tree_uses_one_snapshot_until_a_deletion_updates_the_hierarchy() {
        let mut bookmarks = bookmarks();
        let surviving_id = bookmarks[1].id.clone();
        let mut tree = BookmarkTree::default();
        tree.replace(hierarchy(), &bookmarks, "", None);
        // The live model changes immediately; the sidebar observer hasn't
        // replaced its hierarchy yet. Filters and folds use its own entries.
        bookmarks.remove(0);
        tree.filter("second", None);
        assert_eq!(cards(&tree), vec![surviving_id.as_str()]);
        tree.set_expanded(2, false, None);
        tree.select(Some(&surviving_id));
        assert_eq!(cards(&tree), vec![surviving_id.as_str()]);

        let hierarchy = AnnotationTree { branches: vec![AnnotationBranch { id: 2, title: "Two".into(), annotations: vec![0], children: vec![] }], other: vec![] };
        tree.replace(hierarchy, &bookmarks, "second", Some(&surviving_id));
        assert_eq!(cards(&tree), vec![surviving_id.as_str()]);
        assert!(tree.state.list.rows.iter().any(|row| matches!(row, AnnotationTreeRow::Annotation(AnnotationRow::Card { index: 0, .. }))));
        bookmarks.clear();
        tree.filter("", None);
        tree.set_expanded(2, false, None);
        tree.replace(AnnotationTree::default(), &bookmarks, "", None);
        assert_eq!(tree.state.list.rows, vec![AnnotationTreeRow::Annotation(AnnotationRow::Empty("No bookmarks yet."))]);
    }

    #[test]
    fn flat_filter_hides_empty_groups_and_selection_preserves_filtered_rows() {
        let bookmarks = bookmarks();
        let groups = vec![AnnotationRow::Heading("One".into()), AnnotationRow::card(0, 0, &bookmarks[0]), AnnotationRow::Heading("Two".into()), AnnotationRow::card(1, 0, &bookmarks[1])];
        let mut list = BookmarkList::default();
        list.replace(groups.clone(), &bookmarks, "", None);
        list.filter("SECOND", None);
        let filtered = vec![AnnotationRow::Heading("Two".into()), AnnotationRow::card(1, 0, &bookmarks[1])];
        assert_eq!(list.list.rows, filtered);
        list.select(Some(&bookmarks[1].id));
        assert_eq!(list.list.rows, filtered);
        list.filter("missing", None);
        assert_eq!(list.list.rows, vec![AnnotationRow::Empty("No bookmarks match your filter.")]);
        list.filter("", None);
        assert_eq!(list.list.rows, groups);
    }
}
