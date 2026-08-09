//! The table of contents, and which of its entries the reader is inside.
//!
//! TOC links are hrefs written by the publisher; the renderer resolves each to
//! a `(document index, anchor)` pair. Highlighting the current chapter means
//! matching the renderer's position back to a link, preferring an exact anchor
//! hit and falling back to any entry in the same document.

use std::collections::HashMap;
use std::sync::Arc;

use html_view_core::TocEntry;

/// Where a TOC link points, as `(document index, optional anchor)`.
pub(crate) type Resolution = (usize, Option<String>);

#[derive(Default)]
pub(crate) struct TocState {
    pub(crate) entries: Arc<Vec<TocEntry>>,
    resolutions: HashMap<String, Resolution>,
    /// The link the reader is currently inside, if any.
    pub(crate) active_link: Option<String>,
}

impl TocState {
    pub(crate) fn set(&mut self, entries: Vec<TocEntry>, resolutions: HashMap<String, Resolution>) {
        self.entries = Arc::new(entries);
        self.resolutions = resolutions;
    }

    /// Resolves every link in the tree, skipping any the renderer cannot place.
    pub(crate) fn resolve(entries: &[TocEntry], resolve: impl Fn(&str) -> Option<Resolution>) -> HashMap<String, Resolution> {
        fn visit(entries: &[TocEntry], resolve: &impl Fn(&str) -> Option<Resolution>, resolutions: &mut HashMap<String, Resolution>) {
            for entry in entries {
                if let Some(resolved) = resolve(&entry.link) {
                    resolutions.insert(entry.link.clone(), resolved);
                }
                visit(&entry.children, resolve, resolutions);
            }
        }

        let mut resolutions = HashMap::new();
        visit(entries, &resolve, &mut resolutions);
        resolutions
    }

    /// The link covering `(doc, anchor)`: an exact anchor match if there is
    /// one, otherwise any entry pointing into the same document.
    pub(crate) fn link_at(&self, doc: usize, anchor: &Option<String>) -> Option<String> {
        let exact = self.resolutions.iter().find(|(_, resolved)| resolved.0 == doc && resolved.1 == *anchor).map(|(link, _)| link.clone());
        exact.or_else(|| self.resolutions.iter().find(|(_, resolved)| resolved.0 == doc).map(|(link, _)| link.clone()))
    }

    /// The title of the active entry, searched depth-first through the tree.
    pub(crate) fn active_title(&self) -> Option<String> {
        fn find(entries: &[TocEntry], active: &str) -> Option<String> {
            for entry in entries {
                if entry.link == active {
                    return Some(entry.title.clone());
                }
                if let Some(found) = find(&entry.children, active) {
                    return Some(found);
                }
            }
            None
        }

        find(&self.entries, self.active_link.as_ref()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(link: &str, title: &str, children: Vec<TocEntry>) -> TocEntry {
        TocEntry { link: link.to_owned(), title: title.to_owned(), children }
    }

    fn sample() -> TocState {
        let entries = vec![entry("ch1.xhtml", "Beginnings", vec![entry("ch1.xhtml#part2", "The Second Part", Vec::new())]), entry("ch2.xhtml", "Endings", Vec::new())];
        let resolutions = TocState::resolve(&entries, |href| match href {
            "ch1.xhtml" => Some((0, None)),
            "ch1.xhtml#part2" => Some((0, Some("part2".to_owned()))),
            "ch2.xhtml" => Some((1, None)),
            _ => None,
        });
        let mut toc = TocState::default();
        toc.set(entries, resolutions);
        toc
    }

    #[test]
    fn resolution_covers_nested_entries() {
        let toc = sample();
        assert_eq!(toc.resolutions.len(), 3, "a child entry must be resolved alongside its parent");
    }

    #[test]
    fn unresolvable_links_are_skipped_rather_than_guessed() {
        let entries = vec![entry("missing.xhtml", "Gone", Vec::new())];
        let resolutions = TocState::resolve(&entries, |_| None);
        assert!(resolutions.is_empty());
    }

    #[test]
    fn an_exact_anchor_wins_over_the_enclosing_document() {
        let toc = sample();
        assert_eq!(toc.link_at(0, &Some("part2".to_owned())).as_deref(), Some("ch1.xhtml#part2"));
    }

    #[test]
    fn an_unknown_anchor_falls_back_to_an_entry_in_the_same_document() {
        let toc = sample();
        let link = toc.link_at(0, &Some("nowhere".to_owned())).expect("a document-level entry must be found");
        assert!(link.starts_with("ch1.xhtml"), "fallback must stay within the current document, got {link}");
    }

    #[test]
    fn a_document_with_no_entry_resolves_to_nothing() {
        let toc = sample();
        assert_eq!(toc.link_at(7, &None), None);
    }

    #[test]
    fn the_active_title_is_found_at_any_depth() {
        let mut toc = sample();
        toc.active_link = Some("ch1.xhtml#part2".to_owned());
        assert_eq!(toc.active_title().as_deref(), Some("The Second Part"));
    }

    #[test]
    fn no_active_link_yields_no_title() {
        let toc = sample();
        assert_eq!(toc.active_title(), None);
    }
}
