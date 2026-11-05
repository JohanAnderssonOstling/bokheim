//! The publication tree and its resolved source locations, shared by readers
//! and annotation exports. UI rows and format-specific parsers adapt this index.

use book_model::BookTocEntry;
use std::collections::{HashMap, HashSet};

/// A destination's entry is its preorder position in the full publisher tree.
/// Its position in the resolutions vector is the persisted resolved ordinal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTocEntry {
    pub entry: usize,
    pub document: usize,
    pub anchor: Option<String>,
}

pub type TocResolution = (usize, Option<String>);
pub type TocSourcePositions = HashMap<usize, Vec<(Vec<usize>, usize)>>;

/// The publisher's tree and the indexes used by contents, bookmarks, and
/// exports. Resolved ordinals retain their mapping to full-tree entry order.
#[derive(Default)]
pub struct BookToc {
    pub entries: Vec<BookTocEntry>,
    pub resolutions: Vec<ResolvedTocEntry>,
    pub source_positions: TocSourcePositions,
    pub document_count: usize,
    pub(super) packages: HashMap<Vec<usize>, usize>,
}

impl BookToc {
    pub fn new(entries: Vec<BookTocEntry>, resolutions: Vec<ResolvedTocEntry>) -> Self {
        let document_count = resolutions.iter().map(|entry| entry.document + 1).max().unwrap_or(0);
        Self { entries, resolutions, document_count, ..Self::default() }
    }

    /// Resolutions retain publisher order, so both iterators advance together.
    pub fn resolved_entries(&self) -> impl Iterator<Item = &BookTocEntry> {
        let mut entries = toc_entries(&self.entries).map(|(_, entry)| entry).enumerate();
        self.resolutions.iter().filter_map(move |resolved| entries.find(|(order, _)| *order == resolved.entry).map(|(_, entry)| entry))
    }

    /// Nearest preceding source anchor, then a document entry, then the first
    /// same-document link, then the final entry in a preceding document.
    pub fn ordinal_for_position(&self, document: usize, path: &[usize]) -> Option<usize> {
        if document >= self.document_count {
            return None;
        }
        self.source_positions
            .get(&document)
            .and_then(|positions| positions.iter().filter(|(anchor, _)| anchor.as_slice() <= path).max_by(|(left, _), (right, _)| left.cmp(right)).map(|(_, ordinal)| *ordinal))
            .or_else(|| self.resolutions.iter().position(|entry| entry.document == document && entry.anchor.is_none()))
            .or_else(|| self.resolutions.iter().position(|entry| entry.document == document))
            .or_else(|| self.resolutions.iter().enumerate().filter(|(_, entry)| entry.document < document).max_by_key(|(ordinal, entry)| (entry.document, *ordinal)).map(|(ordinal, _)| ordinal))
    }

    /// Navigable roots are chapters; unresolved headings delegate to children.
    pub fn chapter_ordinals(&self) -> Vec<usize> {
        fn visit(toc: &BookToc, entries: &[BookTocEntry], result: &mut Vec<usize>) {
            for entry in entries {
                if let Some(ordinal) = toc.resolved_entries().position(|resolved| resolved.target == entry.target) {
                    if !result.contains(&ordinal) {
                        result.push(ordinal);
                    }
                } else {
                    visit(toc, &entry.children, result);
                }
            }
        }
        let mut result = Vec::new();
        visit(self, &self.entries, &mut result);
        result
    }

    pub fn resolve(entries: &[BookTocEntry], resolve: impl Fn(&str) -> Option<TocResolution>) -> Vec<ResolvedTocEntry> {
        toc_entries(entries)
            .enumerate()
            .filter_map(|(entry, (_, node))| {
                let (document, anchor) = resolve(&node.target)?;
                Some(ResolvedTocEntry { entry, document, anchor })
            })
            .collect()
    }

    /// Hrefs resolve from the publication root. Empty headings and ambiguous
    /// document names must not acquire a destination.
    pub fn resolve_publication_href(href: &str, documents: &[String], resolve: impl Fn(&str) -> String) -> Option<TocResolution> {
        let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
        if path.is_empty() {
            return None;
        }
        let uri = resolve(path);
        let mut matches = documents.iter().enumerate().filter(|(_, document)| resolve(document) == uri).map(|(index, _)| index);
        let document = matches.next()?;
        matches.next().is_none().then(|| (document, (!fragment.is_empty()).then(|| fragment.to_owned())))
    }

    /// Read each relevant fragment document once. Exports may restrict this
    /// further to the documents addressed by their annotation CFIs.
    pub fn collect_source_positions(resolutions: &[ResolvedTocEntry], documents: &[String], needed: Option<&HashSet<usize>>, mut read: impl FnMut(usize, &str) -> Option<Vec<(Vec<usize>, usize)>>) -> TocSourcePositions {
        let mut positions = HashMap::new();
        for (index, uri) in documents.iter().enumerate() {
            if needed.is_some_and(|needed| !needed.contains(&index)) || !resolutions.iter().any(|entry| entry.document == index && entry.anchor.is_some()) {
                continue;
            }
            if let Some(paths) = read(index, uri) {
                positions.insert(index, paths);
            }
        }
        positions
    }

    /// Parsers supply source paths, while TOC order determines their ordinals.
    pub fn positions_for_anchors(resolutions: &[ResolvedTocEntry], document: usize, path: impl Fn(&str) -> Option<Vec<usize>>) -> Vec<(Vec<usize>, usize)> {
        resolutions.iter().enumerate().filter(|(_, entry)| entry.document == document).filter_map(|(ordinal, entry)| Some((path(entry.anchor.as_deref()?)?, ordinal))).collect()
    }
}

/// Visit every publisher entry in preorder as `(depth, entry)`, including
/// unresolved headings. Traversal borrows the tree without a persistent index.
pub fn toc_entries(entries: &[BookTocEntry]) -> impl Iterator<Item = (usize, &BookTocEntry)> {
    let mut pending: Vec<_> = entries.iter().rev().map(|entry| (0, entry)).collect();
    std::iter::from_fn(move || {
        let (depth, entry) = pending.pop()?;
        pending.extend(entry.children.iter().rev().map(|entry| (depth + 1, entry)));
        Some((depth, entry))
    })
}
