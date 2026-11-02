//! The PDF outline, and which of its entries the current page falls under.
//!
//! The outline arrives as the tree the publisher authored, in the navigation
//! form every format shares. Chapter lookups are indexed when it changes.
//! An entry is named by its position in document order, so the sidebar, the
//! page lookup and the active-entry search all agree on identity without
//! duplicating navigation identities.

use ui_text::text;

use std::collections::HashMap;

use book_content::{BookToc, toc_entries};
use book_model::{BookTocEntry, ChapterReadStatus, ReadingPosition, pdf_toc_page};
use gpui_component::tree::TreeItem;

const TOC_ID_PREFIX: &str = "pdf-toc-";

fn toc_id(order: usize) -> String {
    format!("{TOC_ID_PREFIX}{order}")
}

fn toc_order(id: &str) -> Option<usize> {
    id.strip_prefix(TOC_ID_PREFIX)?.parse().ok()
}

#[derive(Default)]
pub(crate) struct PdfTocState {
    toc: BookToc,
    chapter_pages: Vec<usize>,
    chapter_titles: HashMap<usize, String>,
    pub(crate) read_status: ChapterReadStatus,
    /// The entry the current page sits under, if any.
    active_id: Option<String>,
    loaded: bool,
}

impl PdfTocState {
    pub(crate) fn reset_chapter_read_status(&mut self, id: &str) -> Option<String> {
        let target = self.entry(id)?.target.clone();
        self.read_status.reset_chapter(&self.toc.entries, &target);
        Some(target)
    }
    pub(crate) fn update_read_status(&mut self, page: usize, at_end: bool) {
        self.read_status.update_position(&self.toc.entries, &ReadingPosition::pdf_page(page as u32), None, at_end);
    }

    pub(crate) fn is_read(&self, id: &str) -> bool {
        self.entry(id).is_some_and(|entry| self.read_status.completed.contains(&entry.target))
    }
    pub(crate) fn has_entries(&self) -> bool {
        !self.toc.entries.is_empty()
    }

    pub(crate) fn replace_entries(&mut self, entries: Vec<BookTocEntry>) {
        self.loaded = true;
        self.toc = BookToc::new(entries, Vec::new());
        fn visit<'a>(entries: &'a [BookTocEntry], depth: usize, in_chapter: bool, next: &mut usize, pages: &mut Vec<usize>, titles: &mut HashMap<usize, (usize, usize, &'a str)>) {
            for entry in entries {
                let order = *next;
                *next += 1;
                let page = pdf_toc_page(&entry.target);
                if let Some(page) = page {
                    if !in_chapter {
                        pages.push(page);
                    }
                    if titles.get(&page).is_none_or(|(best_depth, best_order, _)| (depth, order) > (*best_depth, *best_order)) {
                        titles.insert(page, (depth, order, entry.title.as_str()));
                    }
                }
                visit(&entry.children, depth + 1, in_chapter || page.is_some(), next, pages, titles);
            }
        }
        self.chapter_pages.clear();
        let mut titles = HashMap::new();
        visit(&self.toc.entries, 0, false, &mut 0, &mut self.chapter_pages, &mut titles);
        self.chapter_pages.sort_unstable();
        self.chapter_pages.dedup();
        // The single walk resolves equal-page titles by depth and document
        // order. Page zero also covers locations before the first chapter.
        self.chapter_titles = self.chapter_pages.iter().copied().chain([0]).filter_map(|page| titles.get(&page).map(|(_, _, title)| (page, (*title).to_owned()))).collect();
    }

    #[cfg(all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo")))]
    pub(crate) fn narration_chapter_label(&self, page: usize) -> String {
        let start = self.chapter_at(page);
        let number = self.chapter_pages.binary_search(&start).unwrap_or(0) + 1;
        fn chapter_title(entries: &[BookTocEntry], start: usize) -> Option<&str> {
            for entry in entries {
                if let Some(page) = pdf_toc_page(&entry.target) {
                    if page == start {
                        return Some(entry.title.as_str());
                    }
                } else if let Some(title) = chapter_title(&entry.children, start) {
                    return Some(title);
                }
            }
            None
        }
        let title = chapter_title(&self.toc.entries, start).filter(|title| !title.trim().is_empty());
        title.map_or_else(|| format!(text!(reader::chapter), number = number), |title| format!(text!(reader::chapter_with_title), number = number, title = title))
    }

    pub(crate) fn chapter_at(&self, page: usize) -> usize {
        self.chapter_pages.partition_point(|start| *start <= page).checked_sub(1).map_or(0, |index| self.chapter_pages[index])
    }

    pub(crate) fn chapter_title_at(&self, page: usize) -> Option<&str> {
        self.chapter_titles.get(&self.chapter_at(page)).map(String::as_str)
    }

    #[cfg(any(test, all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo"))))]
    pub(crate) fn narration_start_page(&self, page: usize, page_count: usize) -> Option<usize> {
        (page..page_count).find(|page| {
            let entry = self.ordinal_at(*page).and_then(|ordinal| toc_entries(&self.toc.entries).nth(ordinal));
            entry.is_none_or(|(_, entry)| !html::layout::is_navigation_section_title(&entry.title))
        })
    }

    /// Changes the active outline entry and reports whether it moved.
    pub(crate) fn set_active(&mut self, active_id: Option<String>) -> bool {
        if self.active_id == active_id {
            return false;
        }
        self.active_id = active_id;
        true
    }

    /// The entry covering `page_index`: the one opening at the highest page at
    /// or before it.
    ///
    /// Ties break towards the deepest entry, then the latest in document
    /// order, so a chapter and the section opening it both starting on one
    /// page resolve to the section.
    pub(crate) fn entry_at(&self, page_index: usize) -> Option<String> {
        self.ordinal_at(page_index).map(toc_id)
    }

    /// Depth-first ordinal of the entry covering `page_index`, for reading
    /// position. Matches the sidebar identity, so stored ordinals agree with
    /// what the contents panel shows.
    pub(crate) fn ordinal_at(&self, page_index: usize) -> Option<usize> {
        toc_entries(&self.toc.entries)
            .enumerate()
            .filter_map(|(order, (depth, entry))| {
                let page = pdf_toc_page(&entry.target).filter(|page| *page <= page_index)?;
                Some((page, depth, order))
            })
            .max()
            .map(|(_, _, order)| order)
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Before the outline starts, use its earliest destination. Zero denotes
    /// the implicit whole-book entry when the publication has no outline.
    pub(crate) fn bookmark_ordinal_at(&self, page: usize) -> usize {
        self.ordinal_at(page).or_else(|| toc_entries(&self.toc.entries).enumerate().filter_map(|(ordinal, (_, entry))| pdf_toc_page(&entry.target).map(|page| (page, ordinal))).min().map(|(_, ordinal)| ordinal)).unwrap_or(0)
    }

    pub(crate) fn chapter_for_ordinal(&self, ordinal: usize) -> Option<(usize, &str)> {
        let (_, entry) = toc_entries(&self.toc.entries).nth(ordinal)?;
        let page = pdf_toc_page(&entry.target)?;
        Some((self.chapter_at(page), self.chapter_title_at(page)?))
    }

    pub(crate) fn page_of(&self, id: &str) -> Option<usize> {
        pdf_toc_page(&self.entry(id)?.target)
    }

    /// The title of the active entry, shown in the reader toolbar.
    pub(crate) fn active_title(&self) -> Option<&str> {
        Some(self.entry(self.active_id.as_ref()?)?.title.as_str())
    }

    pub(crate) fn active_id(&self) -> Option<&str> {
        self.active_id.as_deref()
    }

    fn entry(&self, id: &str) -> Option<&BookTocEntry> {
        toc_entries(&self.toc.entries).nth(toc_order(id)?).map(|(_, entry)| entry)
    }
}

/// Builds the sidebar tree, naming each entry by its position in document
/// order so [`PdfTocState`] can find it again.
pub(crate) fn pdf_toc_tree_items(entries: &[BookTocEntry]) -> Vec<TreeItem> {
    tree_items(entries, &mut 0)
}

fn tree_items(entries: &[BookTocEntry], next: &mut usize) -> Vec<TreeItem> {
    entries
        .iter()
        .map(|entry| {
            let id = toc_id(*next);
            *next += 1;
            TreeItem::new(id, entry.title.clone()).children(tree_items(&entry.children, next))
        })
        .collect()
}

#[cfg(test)]
mod tts_tests {
    use super::*;

    #[test]
    fn narration_skips_contents_pages_and_resumes_at_the_next_section() {
        let mut toc = PdfTocState::default();
        toc.replace_entries(vec![
            BookTocEntry { title: "Contents".into(), target: "pdf-page:1".into(), children: vec![] },
            BookTocEntry { title: "List of Figures".into(), target: "pdf-page:3".into(), children: vec![] },
            BookTocEntry { title: "Chapter One".into(), target: "pdf-page:4".into(), children: vec![] },
        ]);
        assert_eq!(toc.narration_start_page(0, 8), Some(0));
        assert_eq!(toc.narration_start_page(1, 8), Some(4));
        assert_eq!(toc.narration_start_page(2, 8), Some(4));
        assert_eq!(toc.narration_start_page(4, 8), Some(4));
        assert_eq!(toc.narration_start_page(1, 4), None);
    }

    #[test]
    fn bookmark_ordinals_group_stored_sections_and_support_books_without_an_outline() {
        let mut toc = PdfTocState::default();
        assert!(!toc.is_loaded());
        toc.replace_entries(vec![
            BookTocEntry { title: "One".into(), target: "pdf-page:2".into(), children: vec![BookTocEntry { title: "Section".into(), target: "pdf-page:5".into(), children: vec![] }] },
            BookTocEntry { title: "Two".into(), target: "pdf-page:10".into(), children: vec![] },
        ]);
        assert!(toc.is_loaded());
        assert_eq!(toc.bookmark_ordinal_at(0), 0);
        assert_eq!(toc.bookmark_ordinal_at(8), 1);
        assert_eq!(toc.chapter_for_ordinal(1), Some((2, "One")));
        assert_eq!(toc.chapter_for_ordinal(2), Some((10, "Two")));
        toc.replace_entries(vec![]);
        assert!(toc.is_loaded());
        assert_eq!(toc.bookmark_ordinal_at(8), 0);
        assert_eq!(toc.chapter_for_ordinal(0), None);
    }

    #[test]
    fn sleep_chapters_group_outline_subsections() {
        let mut toc = PdfTocState::default();
        toc.replace_entries(vec![
            BookTocEntry { title: "One".into(), target: "pdf-page:0".into(), children: vec![BookTocEntry { title: "Section".into(), target: "pdf-page:3".into(), children: vec![] }] },
            BookTocEntry { title: "Two".into(), target: "pdf-page:10".into(), children: vec![] },
        ]);
        assert_eq!(toc.chapter_at(8), 0);
        assert_eq!(toc.chapter_at(12), 10);
        assert_eq!(PdfTocState::default().chapter_at(3), 0);
    }

    #[test]
    fn chapter_index_preserves_outline_ties_and_refreshes_on_replacement() {
        let mut toc = PdfTocState::default();
        toc.replace_entries(vec![
            BookTocEntry { title: "Later".into(), target: "pdf-page:10".into(), children: vec![] },
            BookTocEntry {
                title: "One".into(),
                target: "pdf-page:2".into(),
                children: vec![
                    BookTocEntry { title: "Opening".into(), target: "pdf-page:2".into(), children: vec![] },
                    BookTocEntry { title: "Opening again".into(), target: "pdf-page:2".into(), children: vec![] },
                    BookTocEntry { title: "Section".into(), target: "pdf-page:5".into(), children: vec![] },
                ],
            },
        ]);
        assert_eq!(toc.chapter_at(1), 0);
        assert_eq!(toc.chapter_title_at(1), None);
        assert_eq!(toc.chapter_at(8), 2);
        assert_eq!(toc.chapter_title_at(8), Some("Opening again"));
        assert_eq!(toc.chapter_title_at(10), Some("Later"));

        toc.replace_entries(vec![BookTocEntry { title: "Replacement".into(), target: "pdf-page:4".into(), children: vec![] }]);
        assert_eq!(toc.chapter_at(8), 4);
        assert_eq!(toc.chapter_title_at(8), Some("Replacement"));
        assert_eq!(toc.chapter_title_at(2), None);
        toc.replace_entries(vec![]);
        assert_eq!(toc.chapter_at(8), 0);
        assert_eq!(toc.chapter_title_at(8), None);
    }

    #[test]
    fn chapter_walk_delegates_non_navigation_groups_and_reads_current_status() {
        let mut toc = PdfTocState::default();
        toc.replace_entries(vec![BookTocEntry {
            title: "Part".into(),
            target: String::new(),
            children: vec![
                BookTocEntry { title: "One".into(), target: "pdf-page:0".into(), children: vec![BookTocEntry { title: "Section".into(), target: "pdf-page:3".into(), children: vec![] }] },
                BookTocEntry { title: "Two".into(), target: "pdf-page:10".into(), children: vec![] },
            ],
        }]);
        assert_eq!(toc.chapter_at(8), 0);
        assert_eq!(toc.chapter_title_at(8), Some("One"));
        assert_eq!(toc.chapter_at(12), 10);
        assert_eq!(toc.chapter_title_at(12), Some("Two"));
        assert!(!toc.is_read("pdf-toc-3"));
        toc.read_status.completed.insert("pdf-page:10".into());
        assert!(toc.is_read("pdf-toc-3"));
        assert!(!toc.is_read("pdf-toc-2"));
        toc.read_status.reset();
        assert!(!toc.is_read("pdf-toc-3"));
    }
}
