//! Chapter descriptions retained independently of their visible row elements.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound::{Excluded, Unbounded};
use std::time::Duration;

use gpui::{Font, ListAlignment, ListState, Pixels, SharedString, Window, px};
use library_model::BookTocEntry;
use ui_components::ContentsTiming;

pub(super) struct Chapter {
    pub title: SharedString,
    pub target: SharedString,
    pub depth: usize,
    pub end: usize,
    pub timing: Option<ContentsTiming>,
    parent: Option<usize>,
    search: String,
}

pub(super) struct ContentsList {
    pub chapters: Vec<Chapter>,
    pub visible: Vec<usize>,
    pub targets: HashMap<String, Vec<usize>>,
    pub starts: BTreeMap<u64, usize>,
    // Page and sheet can be visible at different widths in the same frame.
    states: [ListState; 2],
    pub nested: bool,
    typography: Option<(Pixels, Font, u32)>,
}

impl Default for ContentsList {
    fn default() -> Self {
        Self { chapters: Vec::new(), visible: Vec::new(), targets: HashMap::new(), starts: BTreeMap::new(), states: std::array::from_fn(|_| ListState::new(0, ListAlignment::Top, px(100.0))), typography: None, nested: false }
    }
}

impl ContentsList {
    pub fn state(&self, sheet: bool) -> &ListState {
        &self.states[usize::from(sheet)]
    }

    pub fn set_toc(&mut self, entries: &[BookTocEntry], duration: Option<u64>) {
        fn visit(entries: &[BookTocEntry], parent: Option<usize>, depth: usize, chapters: &mut Vec<Chapter>) {
            for entry in entries {
                let index = chapters.len();
                chapters.push(Chapter { title: entry.title.clone().into(), target: entry.target.clone().into(), depth, end: 0, timing: None, parent, search: book_model::normalize_search_text(&entry.title) });
                visit(&entry.children, Some(index), depth + 1, chapters);
                chapters[index].end = chapters.len();
            }
        }
        self.chapters.clear();
        self.targets.clear();
        self.starts.clear();
        visit(entries, None, 0, &mut self.chapters);
        self.nested = self.chapters.iter().any(|chapter| chapter.depth > 0);
        for (index, chapter) in self.chapters.iter().enumerate() {
            self.targets.entry(chapter.target.to_string()).or_default().push(index);
            if let Some(start) = book_model::audiobook_toc_position(&chapter.target) {
                self.starts.insert(start, index);
            }
        }
        for chapter in &mut self.chapters {
            chapter.timing = book_model::audiobook_toc_position(&chapter.target).map(|start| {
                let end = self.starts.range((Excluded(start), Unbounded)).next().map(|(&start, _)| start).or(duration);
                ContentsTiming { start: Duration::from_millis(start), duration: end.and_then(|end| end.checked_sub(start)).map(Duration::from_millis) }
            });
        }
        for state in &self.states {
            state.remeasure();
            state.clone().with_uniform_item_height(px(80.0));
        }
    }

    pub fn filter(&mut self, expanded: &HashSet<String>, query: &str) {
        let mut visible = Vec::new();
        if query.is_empty() {
            let mut index = 0;
            while let Some(chapter) = self.chapters.get(index) {
                visible.push(index);
                index = if expanded.contains(chapter.target.as_ref()) { index + 1 } else { chapter.end };
            }
        } else {
            let mut matches = vec![false; self.chapters.len()];
            for (index, chapter) in self.chapters.iter().enumerate().rev() {
                matches[index] |= chapter.search.contains(query);
                if matches[index]
                    && let Some(parent) = chapter.parent
                {
                    matches[parent] = true;
                }
            }
            visible.extend(matches.iter().enumerate().filter_map(|(index, &matched)| matched.then_some(index)));
        }
        let prefix = self.visible.iter().zip(&visible).take_while(|(old, new)| old == new).count();
        let suffix = self.visible[prefix..].iter().rev().zip(visible[prefix..].iter().rev()).take_while(|(old, new)| old == new).count();
        if prefix + suffix != self.visible.len() || self.visible.len() != visible.len() {
            for state in &self.states {
                state.splice_with_uniform_height(prefix..self.visible.len() - suffix, visible.len() - prefix - suffix, px(80.0));
            }
            self.visible = visible;
        }
    }

    pub fn sync_typography(&mut self, window: &Window) {
        let typography = (window.rem_size(), window.text_style().font(), window.scale_factor().to_bits());
        if self.typography.as_ref().is_some_and(|previous| *previous != typography) {
            for state in &self.states {
                state.remeasure();
                state.clone().with_uniform_item_height(px(80.0));
            }
        }
        self.typography = Some(typography);
    }
}
