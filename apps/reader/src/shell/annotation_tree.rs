//! A filtered contents hierarchy with annotation cards beneath each entry.

use std::collections::HashSet;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, div, px};
use ui_components as components;

use super::state_panel::{AnnotationSidebarTarget, reader_annotation_card};
use library_backend::Annotation;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AnnotationTree {
    pub(crate) branches: Vec<AnnotationBranch>,
    /// Annotation indices whose stored locations no longer match the TOC.
    pub(crate) other: Vec<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AnnotationBranch {
    /// Position in the full publisher tree, before pruning empty entries.
    pub(crate) id: usize,
    pub(crate) title: String,
    pub(crate) annotations: Vec<usize>,
    pub(crate) children: Vec<AnnotationBranch>,
}

impl AnnotationBranch {
    fn contains(&self, annotation: usize) -> bool {
        self.annotations.contains(&annotation) || self.children.iter().any(|child| child.contains(annotation))
    }
}

#[derive(Default)]
pub(crate) struct AnnotationTreeState {
    collapsed: HashSet<usize>,
    selected_id: Option<String>,
}

impl AnnotationTreeState {
    pub(crate) fn expanded(&self, id: usize) -> bool {
        !self.collapsed.contains(&id)
    }

    pub(crate) fn set_expanded(&mut self, id: usize, expanded: bool) {
        if expanded {
            self.collapsed.remove(&id);
        } else {
            self.collapsed.insert(id);
        }
    }

    /// Selecting a passage in the document reveals its card, even if its
    /// ancestors were folded. A subsequent manual collapse stays collapsed.
    pub(crate) fn reveal_selected(&mut self, tree: &AnnotationTree, selected_id: Option<&str>, selected_index: Option<usize>) {
        if self.selected_id.as_deref() == selected_id {
            return;
        }
        self.selected_id = selected_id.map(str::to_owned);
        let Some(index) = selected_index else { return };
        fn reveal(branches: &[AnnotationBranch], index: usize, collapsed: &mut HashSet<usize>) {
            for branch in branches {
                if branch.contains(index) {
                    collapsed.remove(&branch.id);
                    reveal(&branch.children, index, collapsed);
                }
            }
        }
        reveal(&tree.branches, index, &mut self.collapsed);
    }
}

pub(crate) trait AnnotationTreeTarget: AnnotationSidebarTarget {
    fn set_annotation_branch_expanded(&mut self, id: usize, expanded: bool, cx: &mut Context<Self>);
}

/// Variable-height cards stay in the sidebar's scroll surface, rather than a
/// uniform-height tree list that would clip their quotes and notes.
pub(crate) fn annotation_tree_rows<C: AnnotationTreeTarget>(
    target: &Entity<C>, tree: &AnnotationTree, state: &AnnotationTreeState, annotations: &[Annotation], selected_id: Option<&str>, no_annotations: &'static str, theme: components::BrowserTheme,
) -> Vec<AnyElement> {
    fn visit<C: AnnotationTreeTarget>(
        target: &Entity<C>, branches: &[AnnotationBranch], state: &AnnotationTreeState, annotations: &[Annotation], selected_id: Option<&str>, depth: usize, theme: components::BrowserTheme, rows: &mut Vec<AnyElement>,
    ) {
        for branch in branches {
            let expanded = state.expanded(branch.id);
            let row = components::ContentsRow { depth, has_children: true, expanded, ..Default::default() };
            let id = branch.id;
            let click_target = target.clone();
            let key_target = target.clone();
            rows.push(
                components::contents_row(("reader-annotation-branch", id), row, Some(components::contents_disclosure(("reader-annotation-disclosure", id), row, theme)), branch.title.clone(), None, theme)
                    .flex_none()
                    .debug_selector(move || format!("reader-annotation-branch-{id}"))
                    .focusable()
                    .tab_stop(true)
                    .on_click(move |_, _, cx| click_target.update(cx, |this, cx| this.set_annotation_branch_expanded(id, !expanded, cx)))
                    .on_key_down(move |event, _, cx| {
                        if event.keystroke.modifiers != gpui::Modifiers::default() {
                            return;
                        }
                        let expanded = match event.keystroke.key.as_str() {
                            "left" => false,
                            "right" => true,
                            "enter" | "space" => !expanded,
                            _ => return,
                        };
                        key_target.update(cx, |this, cx| this.set_annotation_branch_expanded(id, expanded, cx));
                        cx.stop_propagation();
                    })
                    .into_any_element(),
            );
            if expanded {
                for &index in &branch.annotations {
                    rows.push(card(target, annotations, selected_id, index, depth + 1, theme));
                }
                visit(target, &branch.children, state, annotations, selected_id, depth + 1, theme, rows);
            }
        }
    }

    let mut rows = Vec::new();
    if annotations.is_empty() {
        rows.push(components::reader_state_note(no_annotations, theme).p(px(12.0)).into_any_element());
        return rows;
    }
    visit(target, &tree.branches, state, annotations, selected_id, 0, theme, &mut rows);
    if !tree.other.is_empty() {
        rows.push(components::reader_annotation_group_heading("Other notes", theme).into_any_element());
        for &index in &tree.other {
            rows.push(card(target, annotations, selected_id, index, 0, theme));
        }
    }
    rows
}

fn card<C: AnnotationSidebarTarget>(target: &Entity<C>, annotations: &[Annotation], selected_id: Option<&str>, index: usize, depth: usize, theme: components::BrowserTheme) -> AnyElement {
    let annotation = &annotations[index];
    let selected = selected_id == Some(annotation.id.as_str());
    div().w_full().min_w_0().flex_none().pl(px(components::CONTENTS_ROW_INDENT * depth.min(3) as f32)).child(reader_annotation_card(target, index, annotation, selected, theme)).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{AnnotationAnchor, AnnotationStyle};
    use content_address::ContentHash;
    use gpui::{Render, TestAppContext, VisualTestContext, Window};

    struct TreeTest {
        tree: AnnotationTree,
        state: AnnotationTreeState,
        annotations: Vec<Annotation>,
        selected_id: Option<String>,
        navigated_id: Option<String>,
        deleted_id: Option<String>,
    }

    impl AnnotationSidebarTarget for TreeTest {
        fn select_sidebar_annotation(&mut self, id: String, _: &mut Window, cx: &mut Context<Self>) {
            self.selected_id = Some(id);
            cx.notify();
        }

        fn go_to_annotation(&mut self, annotation: &Annotation, cx: &mut Context<Self>) {
            self.navigated_id = Some(annotation.id.clone());
            cx.notify();
        }

        fn delete_sidebar_annotation(&mut self, id: String, cx: &mut Context<Self>) {
            self.deleted_id = Some(id);
            cx.notify();
        }
    }

    impl AnnotationTreeTarget for TreeTest {
        fn set_annotation_branch_expanded(&mut self, id: usize, expanded: bool, cx: &mut Context<Self>) {
            self.state.set_expanded(id, expanded);
            cx.notify();
        }
    }

    impl Render for TreeTest {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let rows = annotation_tree_rows(&cx.entity(), &self.tree, &self.state, &self.annotations, self.selected_id.as_deref(), "No notes yet. Select text, then choose Annotation to add a note.", components::browser_theme(cx));
            components::reader_state_panel(components::browser_theme(cx)).w(px(320.0)).children(rows)
        }
    }

    fn annotation(id: &str) -> Annotation {
        Annotation {
            id: id.into(),
            content_hash: ContentHash::from_bytes([0; 32]),
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4/2/1:0)".to_owned()),
            exact_text: "A passage with enough text to occupy several lines in the annotation sidebar.".into(),
            style: AnnotationStyle::Highlight,
            color: components::READER_ANNOTATION_DEFAULT_COLOR,
            note: "The reader's note".into(),
            created_at: 0,
            toc_ordinal: None,
            progress: None,
            modified_at: 0,
            deleted: false,
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn tree_expands_and_collapses_without_clipping_cards_and_cards_still_navigate(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (view, cx) = cx.add_window_view(|_, _| TreeTest {
            tree: AnnotationTree {
                branches: vec![AnnotationBranch { id: 0, title: "Part".into(), annotations: vec![0], children: vec![AnnotationBranch { id: 1, title: "Chapter".into(), annotations: vec![1], children: vec![] }] }],
                other: vec![],
            },
            state: AnnotationTreeState::default(),
            annotations: vec![annotation("part-note"), annotation("chapter-note")],
            selected_id: None,
            navigated_id: None,
            deleted_id: None,
        });
        draw(cx);
        let parent = cx.debug_bounds("reader-annotation-branch-0").unwrap();
        let child = cx.debug_bounds("reader-annotation-branch-1").unwrap();
        let parent_card = cx.debug_bounds("reader-annotation-card-0").unwrap();
        let child_card = cx.debug_bounds("reader-annotation-card-1").unwrap();
        assert!(parent_card.top() >= parent.bottom());
        assert!(child.top() >= parent_card.bottom());
        assert!(child_card.top() >= child.bottom());
        assert!(child_card.left() > parent_card.left());
        assert!(child_card.size.height > px(60.0));
        cx.simulate_click(parent.center(), Default::default());
        draw(cx);
        assert!(cx.debug_bounds("reader-annotation-branch-1").is_none());
        assert!(cx.debug_bounds("reader-annotation-card-0").is_none());
        assert!(cx.debug_bounds("reader-annotation-card-1").is_none());
        cx.simulate_keystrokes("right");
        draw(cx);
        let child_card = cx.debug_bounds("reader-annotation-card-1").unwrap();
        // The upper portion holds the quote; the actions live below it.
        cx.simulate_click(child_card.origin + gpui::point(px(30.0), px(20.0)), Default::default());
        view.read_with(cx, |view, _| {
            assert_eq!(view.selected_id.as_deref(), Some("chapter-note"));
            assert_eq!(view.navigated_id.as_deref(), Some("chapter-note"));
        });
        view.update(cx, |view, _| view.navigated_id = None);
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |view, _| assert_eq!(view.navigated_id.as_deref(), Some("chapter-note")));
        draw(cx);
        let delete = cx.debug_bounds("reader-annotation-delete-0").unwrap();
        cx.simulate_click(delete.center(), Default::default());
        view.read_with(cx, |view, _| {
            assert_eq!(view.deleted_id.as_deref(), Some("part-note"));
            assert_eq!(view.selected_id.as_deref(), Some("chapter-note"));
            assert_eq!(view.navigated_id.as_deref(), Some("chapter-note"));
        });
    }

    #[test]
    fn selecting_an_annotation_reveals_only_its_ancestors_and_preserves_manual_collapse() {
        let tree = AnnotationTree {
            branches: vec![
                AnnotationBranch { id: 0, title: "Part".into(), annotations: vec![], children: vec![AnnotationBranch { id: 1, title: "Chapter".into(), annotations: vec![0], children: vec![] }] },
                AnnotationBranch { id: 2, title: "Another part".into(), annotations: vec![1], children: vec![] },
            ],
            other: vec![],
        };
        let mut state = AnnotationTreeState::default();
        for id in 0..3 {
            state.set_expanded(id, false);
        }
        state.reveal_selected(&tree, Some("first"), Some(0));
        assert!(state.expanded(0));
        assert!(state.expanded(1));
        assert!(!state.expanded(2));
        state.set_expanded(0, false);
        state.reveal_selected(&tree, Some("first"), Some(0));
        assert!(!state.expanded(0));
        state.reveal_selected(&tree, None, None);
        state.reveal_selected(&tree, Some("first"), Some(0));
        assert!(state.expanded(0));
    }
}
