//! Search excerpts nested under the same contents branches as annotations.
use std::collections::HashMap;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity};
use ui_components as components;

use super::annotation_tree::{AnnotationBranch, AnnotationTree, AnnotationTreeState};

#[derive(Clone)]
pub(crate) struct SearchBranch {
    id: usize,
    title: String,
    depth: usize,
    expanded: bool,
}

#[derive(Default)]
pub(crate) struct SearchContents {
    pub(crate) tree: AnnotationTree,
    state: AnnotationTreeState,
    branches: HashMap<usize, SearchBranch>,
    depths: HashMap<usize, usize>,
}

impl SearchContents {
    pub(crate) fn set_query(&mut self, query: &str) {
        self.state.set_filter(query);
    }

    pub(crate) fn set_expanded(&mut self, id: usize, expanded: bool) {
        self.state.set_expanded(id, expanded);
    }

    pub(crate) fn reveal(&mut self, index: usize) {
        self.state.reveal_selected(&self.tree, Some(&index.to_string()), Some(index));
    }

    /// Branch IDs and result indices remain stable when other branches fold.
    pub(crate) fn rows(&mut self) -> Vec<(bool, usize)> {
        fn visit(branches: &[AnnotationBranch], state: &AnnotationTreeState, depth: usize, headings: &mut HashMap<usize, SearchBranch>, depths: &mut HashMap<usize, usize>, rows: &mut Vec<(bool, usize)>) {
            for branch in branches {
                let expanded = state.expanded(branch.id);
                headings.insert(branch.id, SearchBranch { id: branch.id, title: branch.title.clone(), depth, expanded });
                rows.push((true, branch.id));
                if expanded {
                    for &index in &branch.annotations {
                        depths.insert(index, depth + 1);
                        rows.push((false, index));
                    }
                    visit(&branch.children, state, depth + 1, headings, depths, rows);
                }
            }
        }
        self.branches.clear();
        self.depths.clear();
        let mut rows = Vec::new();
        visit(&self.tree.branches, &self.state, 0, &mut self.branches, &mut self.depths, &mut rows);
        rows.extend(self.tree.other.iter().map(|&index| (false, index)));
        rows
    }

    pub(crate) fn depth(&self, index: usize) -> usize {
        self.depths.get(&index).copied().unwrap_or(0)
    }

    pub(crate) fn branch(&self, id: usize) -> Option<&SearchBranch> {
        self.branches.get(&id)
    }
}

pub(crate) fn branch_row<C: 'static>(target: &Entity<C>, branch: &SearchBranch, theme: components::BrowserTheme, toggle: fn(&mut C, usize, bool, &mut Context<C>)) -> AnyElement {
    let SearchBranch { id, depth, expanded, .. } = *branch;
    let row = components::ContentsRow { depth, has_children: true, expanded, ..Default::default() };
    let click_target = target.downgrade();
    let key_target = target.downgrade();
    components::contents_row(("reader-search-branch", id), row, Some(components::contents_disclosure(("reader-search-disclosure", id), row, theme)), branch.title.clone(), None, None, theme)
        .flex_none()
        .focusable()
        .tab_stop(true)
        .on_click(move |_, _, cx| {
            let _ = click_target.update(cx, |this, cx| toggle(this, id, !expanded, cx));
        })
        .on_key_down(move |event, _, cx| {
            if event.keystroke.modifiers != gpui::Modifiers::default() { return; }
            let expanded = match event.keystroke.key.as_str() {
                "left" => false,
                "right" => true,
                "enter" | "space" => !expanded,
                _ => return,
            };
            let _ = key_target.update(cx, |this, cx| toggle(this, id, expanded, cx));
            cx.stop_propagation();
        })
        .into_any_element()
}
