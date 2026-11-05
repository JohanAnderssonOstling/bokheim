//! The table of contents, and which of its entries the reader is inside.
//!
//! TOC links are hrefs written by the publisher; the renderer resolves each to
//! a source position. Highlighting the current chapter means matching the
//! settled page's source address back to a link. Resolutions stay in TOC
//! order so duplicate targets and document-level fallbacks are deterministic.

use book_content::is_navigation_section_title;
use std::collections::HashMap;

use book_content::{BookToc, toc_entries};
pub(crate) use book_content::{ResolvedTocEntry, TocSourcePositions as SourcePositions};

use book_model::{BookTocEntry, ChapterReadStatus};
use gpui_component::tree::TreeItem;
use html_pipeline::MarkupSyntax;
use html_dom::NodeRef;
use html_pipeline::decode_html_bytes;
use html_pipeline::parse_document;
use html::resources::ResourceProvider;
use skrift_column_paginator::SourcePosition;
use skrift_column_paginator::cfi::PublicationCfi;
#[cfg(test)]
use skrift_column_paginator::cfi::parse_source_position;

use crate::shell::annotation_tree::{AnnotationBranch, AnnotationTree};

const TOC_ID_PREFIX: &str = "epub-toc-";

fn toc_id(order: usize) -> String {
    format!("{TOC_ID_PREFIX}{order}")
}

#[derive(Default)]
pub(crate) struct TocState {
    pub(crate) cfi: PublicationCfi,
    toc: BookToc,
    pub(crate) read_status: ChapterReadStatus,
    /// The link the reader is currently inside, if any.
    active_link: Option<String>,
    active_sidebar_id: Option<String>,
    position: Option<SourcePosition>,
    loaded: bool,
}

impl TocState {
    /// Sidebar identity uses the full publisher tree's preorder position,
    /// including unresolved headings and repeated destinations.
    pub(crate) fn tree_items(&self) -> Vec<TreeItem> {
        fn visit(entries: &[BookTocEntry], next: &mut usize) -> Vec<TreeItem> {
            entries
                .iter()
                .map(|entry| {
                    let id = toc_id(*next);
                    *next += 1;
                    TreeItem::new(id, entry.title.clone()).children(visit(&entry.children, next))
                })
                .collect()
        }
        visit(&self.toc.entries, &mut 0)
    }

    /// Hrefs stay navigation targets rather than sidebar row identities.
    pub(crate) fn link_for_id(&self, id: &str) -> Option<&str> {
        let order = id.strip_prefix(TOC_ID_PREFIX)?.parse::<usize>().ok()?;
        toc_entries(&self.toc.entries).nth(order).map(|(_, entry)| entry.target.as_str())
    }

    pub(crate) fn active_id(&self) -> Option<&str> {
        self.active_sidebar_id.as_deref()
    }

    pub(crate) fn is_read(&self, id: &str) -> bool {
        self.link_for_id(id).is_some_and(|link| self.read_status.completed.contains(link))
    }

    pub(crate) fn set(&mut self, entries: Vec<BookTocEntry>, resolutions: Vec<ResolvedTocEntry>) {
        self.loaded = false;
        self.toc = BookToc::new(entries, resolutions);
        self.refresh_position();
        self.update_active_sidebar_id();
    }

    /// Retain early renderer positions even when the TOC is still loading.
    pub(crate) fn update_position(&mut self, position: &SourcePosition) -> bool {
        self.position = Some(position.clone());
        self.refresh_position()
    }

    pub(crate) fn update_read_status(&mut self, at_end: bool) {
        self.read_status.update(&self.toc.entries, self.active_link.as_deref(), at_end);
    }

    pub(crate) fn reset_chapter_read_status(&mut self, target: &str) {
        self.read_status.reset_chapter(&self.toc.entries, target);
    }

    fn refresh_position(&mut self) -> bool {
        let link = self.position.as_ref().and_then(|position| self.ordinal_for_position(position)).and_then(|ordinal| self.toc.resolved_entries().nth(ordinal)).map(|entry| entry.target.clone());
        self.set_active_link(link)
    }

    /// Index TOC anchors in source order for reading position and annotations.
    pub(crate) fn source_positions(resolutions: &[ResolvedTocEntry], documents: &[String], provider: &dyn ResourceProvider) -> SourcePositions {
        BookToc::collect_source_positions(resolutions, documents, None, |index, uri| {
            let bytes = provider.read_bytes(uri).ok()?;
            let metadata = provider.metadata(uri).ok();
            let source = decode_html_bytes(&bytes, metadata.as_ref().and_then(|metadata| metadata.charset.as_deref()));
            let syntax = if metadata.as_ref().and_then(|metadata| metadata.media_type.as_deref()) == Some("application/xhtml+xml") { MarkupSyntax::Xml } else { MarkupSyntax::from_uri(uri) };
            anchor_source_positions(resolutions, index, &source, syntax)
        })
    }

    pub(crate) fn install_source_positions(&mut self, positions: SourcePositions, document_count: usize) {
        self.loaded = true;
        self.toc.source_positions = positions;
        self.toc.document_count = document_count;
        self.refresh_position();
    }

    pub(crate) fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Before the first chapter, use the first entry. With no TOC, ordinal
    /// zero represents the whole book and remains in the ungrouped sidebar.
    pub(crate) fn bookmark_ordinal_for_cfi(&self, cfi: &str) -> usize {
        self.ordinal_for_cfi(cfi).unwrap_or(0)
    }

    pub(crate) fn ordinal_for_cfi(&self, cfi: &str) -> Option<usize> {
        self.ordinal_for_position(&self.cfi.parse_source_position(cfi)?)
    }

    /// Resolve a document without inventing a CFI location inside it.
    pub(crate) fn ordinal_for_document(&self, document: usize) -> Option<usize> {
        self.ordinal_for_position(&SourcePosition { document, path: Vec::new(), utf16_offset: None })
    }

    pub(crate) fn ordinal_for_position(&self, position: &SourcePosition) -> Option<usize> {
        self.toc.ordinal_for_position(position.document, &position.path)
    }

    /// Navigable roots are chapters; unresolved grouping nodes delegate to
    /// their children. Nested sections remain within their owning chapter.
    pub(in crate::epub) fn chapter_ordinals(&self) -> Vec<usize> {
        self.toc.chapter_ordinals()
    }

    pub(crate) fn chapter_for_cfi(&self, cfi: &str) -> Option<usize> {
        let ordinal = self.ordinal_for_cfi(cfi)?;
        self.chapter_ordinals().into_iter().rev().find(|chapter| *chapter <= ordinal)
    }

    #[cfg(any(test, all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo"))))]
    pub(crate) fn chapter_for_document(&self, document: usize) -> Option<usize> {
        let ordinal = self.ordinal_for_document(document)?;
        self.chapter_ordinals().into_iter().rev().find(|chapter| *chapter <= ordinal)
    }

    #[cfg(any(test, all(any(feature = "native-tts", target_arch = "wasm32"), not(feature = "kobo"))))]
    pub(crate) fn narration_excludes_cfi(&self, cfi: &str) -> bool {
        let Some(position) = self.cfi.parse_source_position(cfi) else { return false };
        let Some(ordinal) = self.ordinal_for_position(&position) else { return false };
        let entry = &self.toc.resolutions[ordinal];
        // The sidebar falls back to an earlier entry across documents and
        // before the first anchor. Those fallbacks must not suppress prose.
        if entry.document != position.document {
            return false;
        }
        if entry.anchor.is_some() && !self.toc.source_positions.get(&position.document).is_some_and(|anchors| anchors.iter().any(|(path, index)| *index == ordinal && path <= &position.path)) {
            return false;
        }
        self.title_for_ordinal(ordinal).is_some_and(|title| is_navigation_section_title(&title))
    }

    /// The publisher's own href for the entry the reader is inside — the
    /// identity a stored position is recorded against, since a title can be
    /// re-worded by a re-parsed navigation document.
    pub(crate) fn active_link(&self) -> Option<String> {
        self.active_link.clone()
    }

    pub(crate) fn set_active_link(&mut self, active_link: Option<String>) -> bool {
        if self.active_link == active_link {
            return false;
        }
        self.active_link = active_link;
        self.update_active_sidebar_id();
        true
    }

    fn update_active_sidebar_id(&mut self) {
        self.active_sidebar_id = self.active_link.as_ref().and_then(|active| toc_entries(&self.toc.entries).position(|(_, entry)| &entry.target == active)).map(toc_id);
    }

    /// Depth-first ordinal of the entry the reader is inside, for reading
    /// position. Resolutions are stored in TOC order, so the index is the
    /// ordinal; duplicate links resolve to their first entry, matching
    /// `active_title`.
    pub(crate) fn ordinal_of_active(&self) -> Option<usize> {
        let active = self.active_link.as_ref()?;
        self.toc.resolved_entries().position(|entry| &entry.target == active)
    }

    /// Attaches annotation indices to the full publisher hierarchy, keeping
    /// only entries with annotations in their own section or a descendant.
    /// Stored ordinals use resolved TOC order, which can skip unresolved
    /// entries; those entries still survive when they parent an annotated one.
    pub(crate) fn annotation_tree(&self, ordinals: impl IntoIterator<Item = Option<usize>>) -> AnnotationTree {
        fn visit(entries: &[BookTocEntry], groups: &mut HashMap<&str, Vec<usize>>, next: &mut usize) -> Vec<AnnotationBranch> {
            let mut branches = Vec::new();
            for entry in entries {
                let id = *next;
                *next += 1;
                // Removing the group also ensures duplicate TOC links cannot
                // show the same annotation more than once.
                let annotations = groups.remove(entry.target.as_str()).unwrap_or_default();
                let children = visit(&entry.children, groups, next);
                if !annotations.is_empty() || !children.is_empty() {
                    branches.push(AnnotationBranch { id, title: entry.title.clone(), annotations, children });
                }
            }
            branches
        }

        let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut other = Vec::new();
        let resolved = self.toc.resolved_entries().collect::<Vec<_>>();
        for (index, ordinal) in ordinals.into_iter().enumerate() {
            if let Some(resolved) = ordinal.and_then(|ordinal| resolved.get(ordinal)) {
                groups.entry(resolved.target.as_str()).or_default().push(index);
            } else {
                other.push(index);
            }
        }
        let branches = visit(&self.toc.entries, &mut groups, &mut 0);
        other.extend(groups.into_values().flatten());
        other.sort_unstable();
        AnnotationTree { branches, other }
    }

    /// Publisher title for a resolved TOC ordinal, including chapter groups.
    pub(crate) fn title_for_ordinal(&self, ordinal: usize) -> Option<String> {
        self.title_for_link(&self.toc.resolved_entries().nth(ordinal)?.target)
    }

    /// The title of the active entry, searched depth-first through the tree.
    pub(crate) fn active_title(&self) -> Option<String> {
        self.title_for_link(self.active_link.as_ref()?)
    }

    fn title_for_link(&self, active: &str) -> Option<String> {
        toc_entries(&self.toc.entries).find(|(_, entry)| entry.target == active).map(|(_, entry)| entry.title.clone())
    }
}

fn anchor_source_positions(resolutions: &[ResolvedTocEntry], document_index: usize, source: &str, syntax: MarkupSyntax) -> Option<Vec<(Vec<usize>, usize)>> {
    let document = parse_document(source, syntax).ok()?.into_dom_checked().ok()?;
    let ids: HashMap<_, _> = document.nodes().filter_map(|(_, node)| match node {
        NodeRef::Element(element) => element.id().map(|id| (id, element.node_id())),
        NodeRef::Text(_) => None,
    }).collect();
    Some(BookToc::positions_for_anchors(resolutions, document_index, |anchor| {
        let node = *ids.get(anchor)?;
        document.element_source_path(node).map(|path| path.into_iter().map(|step| step.step()).collect())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn entry(title: &str, children: Vec<BookTocEntry>) -> BookTocEntry {
        BookTocEntry { title: title.into(), target: format!("{title}.xhtml"), children }
    }

    fn toc(entries: Vec<BookTocEntry>) -> TocState {
        let resolutions = BookToc::resolve(&entries, |link| (link != "Part.xhtml").then_some((0, None)));
        let mut toc = TocState::default();
        toc.set(entries, resolutions);
        toc
    }

    #[test]
    fn bookmark_ordinals_wait_for_source_positions_and_support_books_without_a_toc() {
        let mut toc = TocState::default();
        assert!(!toc.is_loaded());
        toc.set(vec![], vec![]);
        assert!(!toc.is_loaded());
        toc.install_source_positions(HashMap::new(), 1);
        assert!(toc.is_loaded());
        assert_eq!(toc.bookmark_ordinal_for_cfi("epubcfi(/6/2!/4/2/1:0)"), 0);
        assert_eq!(toc.annotation_tree([Some(0)]).other, vec![0]);
    }

    #[test]
    fn sidebar_positions_include_unresolved_headings_and_repeated_names_and_links() {
        let mut toc = toc(vec![entry("Part", vec![entry("Chapter", vec![]), entry("Chapter", vec![])]), entry("Last", vec![])]);
        let items = toc.tree_items();
        assert_eq!(items[0].id, "epub-toc-0");
        assert_eq!(items[0].children[0].id, "epub-toc-1");
        assert_eq!(items[0].children[1].id, "epub-toc-2");
        assert_eq!(items[1].id, "epub-toc-3");
        assert_eq!(toc.link_for_id("epub-toc-0"), Some("Part.xhtml"));
        assert_eq!(toc.link_for_id("epub-toc-1"), Some("Chapter.xhtml"));
        assert_eq!(toc.link_for_id("epub-toc-2"), Some("Chapter.xhtml"));
        assert_eq!(toc.link_for_id("epub-toc-3"), Some("Last.xhtml"));
        assert_eq!(toc.link_for_id("Chapter.xhtml"), None);
        assert_eq!(toc.link_for_id("epub-toc-4"), None);
        assert_eq!(toc.link_for_id("epub-toc-invalid"), None);

        toc.set_active_link(Some("Chapter.xhtml".into()));
        assert_eq!(toc.active_id().as_deref(), Some("epub-toc-1"));
        toc.read_status.completed.insert("Chapter.xhtml".into());
        assert!(toc.is_read("epub-toc-1"));
        assert!(toc.is_read("epub-toc-2"));
        assert!(!toc.is_read("epub-toc-0"));
        let target = toc.link_for_id("epub-toc-2").unwrap().to_owned();
        toc.reset_chapter_read_status(&target);
        assert!(!toc.is_read("epub-toc-1"));
        assert!(!toc.is_read("epub-toc-2"));
        // Sidebar positions include Part, while persisted annotation ordinals
        // still refer to the resolved entries only.
        assert_eq!(toc.ordinal_of_active(), Some(0));
    }

    #[test]
    fn narration_skips_named_sections_without_leaking_before_or_after_them() {
        let entries = vec![BookTocEntry { title: "Contents".into(), target: "contents".into(), children: vec![] }, BookTocEntry { title: "Chapter One".into(), target: "one".into(), children: vec![] }];
        let resolutions = BookToc::resolve(&entries, |link| Some((0, Some(link.into()))));
        let mut toc = TocState::default();
        toc.set(entries, resolutions);
        toc.install_source_positions(HashMap::from([(0, vec![(vec![4, 4], 0), (vec![4, 8], 1)])]), 2);
        assert!(!toc.narration_excludes_cfi("epubcfi(/6/2!/4/2/1:0)"));
        assert!(toc.narration_excludes_cfi("epubcfi(/6/2!/4/4/1:0)"));
        assert!(toc.narration_excludes_cfi("epubcfi(/6/2!/4/6/1:0)"));
        assert!(!toc.narration_excludes_cfi("epubcfi(/6/2!/4/8/1:0)"));
        assert!(!toc.narration_excludes_cfi("epubcfi(/6/4!/4/2/1:0)"));
        assert!(!toc.narration_excludes_cfi("invalid"));
    }

    #[test]
    fn narration_document_labels_do_not_exclude_following_unlisted_documents() {
        let mut toc = TocState::default();
        toc.set(vec![entry("Table of Contents", vec![])], vec![ResolvedTocEntry { entry: 0, document: 0, anchor: None }]);
        toc.install_source_positions(HashMap::new(), 2);
        assert!(toc.narration_excludes_cfi("epubcfi(/6/2!/4/2/1:0)"));
        assert!(!toc.narration_excludes_cfi("epubcfi(/6/4!/4/2/1:0)"));
    }

    #[test]
    fn sleep_chapters_group_nested_sections_in_the_same_document() {
        let entries = vec![
            BookTocEntry { title: "One".into(), target: "one".into(), children: vec![BookTocEntry { title: "Section".into(), target: "section".into(), children: vec![] }] },
            BookTocEntry { title: "Two".into(), target: "two".into(), children: vec![] },
        ];
        let resolutions = BookToc::resolve(&entries, |link| Some((0, Some(link.into()))));
        let source = r#"<html><body><h1 id="one">One</h1><p>Body</p><h2 id="section">Section</h2><p>Body</p><h1 id="two">Two</h1><p>Body</p></body></html>"#;
        let positions = anchor_source_positions(&resolutions, 0, source, MarkupSyntax::Xml).unwrap();
        let section_path = positions.iter().find(|(_, ordinal)| *ordinal == 1).unwrap().0.iter().map(|step| format!("/{step}")).collect::<String>();
        let second_path = positions.iter().find(|(_, ordinal)| *ordinal == 2).unwrap().0.iter().map(|step| format!("/{step}")).collect::<String>();
        let section_cfi = format!("epubcfi(/6/2!{section_path}/1:0)");
        let second_cfi = format!("epubcfi(/6/2!{second_path}/1:0)");
        let mut state = TocState::default();
        state.set(entries, resolutions);
        state.install_source_positions(HashMap::from([(0, positions)]), 1);
        assert_eq!(state.chapter_for_cfi(&section_cfi), Some(0));
        assert_eq!(state.chapter_for_cfi(&second_cfi), Some(2));
    }

    #[test]
    fn document_lookup_preserves_chapter_fallbacks_without_a_source_path() {
        let entries = vec![
            BookTocEntry { title: "One".into(), target: "one".into(), children: vec![BookTocEntry { title: "Section".into(), target: "section".into(), children: vec![] }] },
            BookTocEntry { title: "Two".into(), target: "two".into(), children: vec![] },
        ];
        let resolutions = BookToc::resolve(&entries, |link| Some((1, Some(link.into()))));
        let mut state = TocState::default();
        state.set(entries, resolutions);
        state.install_source_positions(HashMap::from([(1, vec![(vec![4, 2], 0), (vec![4, 6], 1), (vec![4, 10], 2)])]), 3);

        // A document-only target cannot distinguish its anchors, so use the
        // first entry; a precise CFI still selects the later chapter.
        assert_eq!(state.ordinal_for_document(1), Some(0));
        assert_eq!(state.chapter_for_document(1), Some(0));
        assert_eq!(state.chapter_for_cfi("epubcfi(/6/4!/4/10/1:0)"), Some(2));
        assert_eq!(state.chapter_for_document(0), None);
        assert_eq!(state.chapter_for_document(2), Some(2));
        assert_eq!(state.chapter_for_document(3), None);
        assert_eq!(state.chapter_for_document(usize::MAX), None);
    }

    #[test]
    fn annotations_keep_their_section_and_ancestors_but_prune_empty_subtrees() {
        let toc = toc(vec![entry("Part", vec![entry("Empty chapter", vec![]), entry("Chapter", vec![entry("Section", vec![]), entry("Empty section", vec![])])]), entry("Empty part", vec![])]);
        // Part is unresolved, so annotation ordinal 2 names Section rather
        // than Chapter (which has position 2 in the complete publisher tree).
        let tree = toc.annotation_tree([Some(2), Some(1), Some(2)]);
        assert!(tree.other.is_empty());
        assert_eq!(tree.branches.len(), 1);
        let part = &tree.branches[0];
        assert_eq!(part.title, "Part");
        assert!(part.annotations.is_empty());
        assert_eq!(part.children.len(), 1);
        let chapter = &part.children[0];
        assert_eq!(chapter.id, 2);
        assert_eq!(chapter.title, "Chapter");
        assert_eq!(chapter.annotations, vec![1]);
        assert_eq!(chapter.children.len(), 1);
        assert_eq!(chapter.children[0].title, "Section");
        assert_eq!(chapter.children[0].annotations, vec![0, 2]);
    }

    #[test]
    fn directly_annotated_parents_survive_without_empty_children() {
        let toc = toc(vec![entry("Chapter", vec![entry("Empty section", vec![])])]);
        let tree = toc.annotation_tree([Some(0)]);
        assert_eq!(tree.branches.len(), 1);
        assert_eq!(tree.branches[0].annotations, vec![0]);
        assert!(tree.branches[0].children.is_empty());
        assert!(toc.annotation_tree([]).branches.is_empty());
    }

    #[test]
    fn missing_locations_and_books_without_toc_keep_all_annotations() {
        let toc = toc(vec![entry("Chapter", vec![])]);
        let tree = toc.annotation_tree([None, Some(9), Some(0)]);
        assert_eq!(tree.other, vec![0, 1]);
        assert_eq!(tree.branches[0].annotations, vec![2]);
        let tree = TocState::default().annotation_tree([None, Some(0)]);
        assert!(tree.branches.is_empty());
        assert_eq!(tree.other, vec![0, 1]);
    }

    #[test]
    fn duplicate_links_show_each_annotation_once() {
        let toc = toc(vec![entry("Chapter", vec![]), entry("Chapter", vec![])]);
        let tree = toc.annotation_tree([Some(0), Some(1)]);
        assert_eq!(tree.branches.len(), 1);
        assert_eq!(tree.branches[0].annotations, vec![0, 1]);
        assert!(tree.other.is_empty());
    }

    #[test]
    fn initial_position_is_not_lost_when_toc_arrives_later() {
        let mut state = TocState::default();
        let position = SourcePosition { document: 0, path: vec![4, 6, 1], utf16_offset: Some(0) };
        assert!(!state.update_position(&position));
        let entries = vec![BookTocEntry { title: "Section".into(), target: "/chapter.xhtml#section".into(), children: vec![] }];
        let resolutions = BookToc::resolve(&entries, |_| Some((0, Some("section".into()))));
        state.set(entries, resolutions);
        assert_eq!(state.active_link().as_deref(), Some("/chapter.xhtml#section"));
        assert_eq!(state.ordinal_of_active(), Some(0));
    }

    #[test]
    fn late_source_index_recomputes_the_retained_position() {
        let entries = vec![BookTocEntry { title: "First".into(), target: "chapter.xhtml#first".into(), children: vec![] }, BookTocEntry { title: "Second".into(), target: "chapter.xhtml#second".into(), children: vec![] }];
        let resolutions = BookToc::resolve(&entries, |href| Some((0, href.split_once('#').map(|(_, anchor)| anchor.into()))));
        let positions = anchor_source_positions(&resolutions, 0, "<html><head/><body><h1 id='first'>First</h1><div id='second'/><p>Second</p></body></html>", MarkupSyntax::Xml).unwrap();
        let mut state = TocState::default();
        let position = SourcePosition { document: 0, path: vec![4, 6, 1], utf16_offset: Some(0) };
        state.update_position(&position);
        state.set(entries, resolutions);
        state.install_source_positions(HashMap::from([(0, positions)]), 1);
        assert_eq!(state.active_title().as_deref(), Some("Second"));
        assert_eq!(state.active_link().as_deref(), Some("chapter.xhtml#second"));
        assert!(!state.update_position(&position), "installing the TOC already resolved the latest page");
    }

    #[test]
    fn source_order_tracks_empty_markers_images_and_reverse_navigation() {
        let entries = ["first", "empty", "image"].map(|anchor| BookTocEntry { title: anchor.into(), target: format!("chapter.xhtml#{anchor}"), children: vec![] }).to_vec();
        let resolutions = BookToc::resolve(&entries, |href| Some((0, href.split_once('#').map(|(_, anchor)| anchor.into()))));
        let source = "<html><head/><body><h1 id='first'>First</h1><div id='empty'/><p>After empty marker</p><div id='image'><img src='picture.svg'/></div><p>After image</p></body></html>";
        let positions = anchor_source_positions(&resolutions, 0, source, MarkupSyntax::Xml).unwrap();
        let mut state = TocState::default();
        state.set(entries, resolutions);
        state.install_source_positions(HashMap::from([(0, positions)]), 1);
        for (path, offset, expected) in [(vec![4, 2, 1], Some(0), "first"), (vec![4, 6, 1], Some(0), "empty"), (vec![4, 8, 2], None, "image"), (vec![4, 10, 1], Some(0), "image"), (vec![4, 6, 1], Some(0), "empty")] {
            state.update_position(&SourcePosition { document: 0, path, utf16_offset: offset });
            assert_eq!(state.active_title().as_deref(), Some(expected));
            assert_eq!(state.active_link(), Some(format!("chapter.xhtml#{expected}")));
        }
    }

    #[test]
    fn selected_text_is_assigned_to_its_section_at_creation() {
        let entries = vec![BookTocEntry { title: "Chapter".into(), target: "/chapter.xhtml#chapter".into(), children: vec![BookTocEntry { title: "Section".into(), target: "/chapter.xhtml#section".into(), children: vec![] }] }];
        let resolutions = BookToc::resolve(&entries, |href| Some((0, href.split_once('#').map(|(_, fragment)| fragment.to_owned()))));
        let source = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head/><body><h1 id="chapter">Chapter</h1><p>Before the section</p><h2 id="section">Section</h2><p>Annotated text</p></body></html>"#;
        let positions = anchor_source_positions(&resolutions, 0, source, MarkupSyntax::Xml).unwrap();
        let mut state = TocState::default();
        state.set(entries, resolutions);
        state.install_source_positions(HashMap::from([(0, positions)]), 1);
        let ordinal = state.ordinal_for_cfi("epubcfi(/6/2!/4,/8/1:0,/8/1:9)");
        assert_eq!(ordinal, Some(1));
        let tree = state.annotation_tree([ordinal]);
        assert!(tree.other.is_empty());
        assert_eq!(tree.branches[0].children[0].annotations, vec![0]);
        assert_eq!(state.ordinal_for_cfi("epubcfi(/6/2!/4/4/1:0)"), Some(0));
        assert_eq!(state.ordinal_for_cfi("epubcfi(/6/4!/4/4/1:0)"), None);
    }

    #[test]
    fn cfi_ranges_handle_assertion_commas_and_multi_digit_paths() {
        assert_eq!(parse_source_position("epubcfi(/6/2!/4[body^,name],/12[section]/1:0,/12[section]/1:8)"), Some(SourcePosition { document: 0, path: vec![4, 12, 1], utf16_offset: Some(0) }));
        assert_eq!(parse_source_position("epubcfi(/6/2!/4/8[sec^]tion]/1:12)"), Some(SourcePosition { document: 0, path: vec![4, 8, 1], utf16_offset: Some(12) }));
        assert!(parse_source_position("epubcfi(/6/2!/4[unclosed/1:0)").is_none());
    }

    #[test]
    fn epub_archive_navigation_and_source_locations_agree_across_chapters() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let source = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head/><body><h1 id="chapter">Chapter</h1><p>Before</p><h2 id="section">Section</h2><p>Annotated text</p></body></html>"#;
        for (name, bytes) in [
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
            (
                "OEBPS/content.opf",
                r#"<package version="3.0"><manifest><item id="chapter" href="Text/chapter.xhtml" media-type="application/xhtml+xml"/><item id="next" href="Text/next.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="chapter"/><itemref idref="next"/></spine></package>"#,
            ),
            (
                "OEBPS/nav.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="Text/chapter.xhtml#chapter">Chapter</a><ol><li><a href="Text/chapter.xhtml#section">Section</a></li></ol></li><li><a href="Text/next.xhtml">Next chapter</a></li></ol></nav></body></html>"#,
            ),
            ("OEBPS/Text/chapter.xhtml", source),
            ("OEBPS/Text/next.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head/><body><p>Next chapter</p></body></html>"#),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes.as_bytes()).unwrap();
        }
        let book = crate::book::NativeBook::from_reader(book_content::BookFormat::Epub, archive.finish().unwrap()).unwrap();
        let entries = book.toc().to_vec();
        let resolutions = BookToc::resolve(&entries, |href| BookToc::resolve_publication_href(href, book.document_uris(), |path| book.provider.resolve("", path)));
        assert_eq!(resolutions.len(), 3);
        let positions = TocState::source_positions(&resolutions, book.document_uris(), book.provider.as_ref());
        let mut state = TocState::default();
        state.update_position(&SourcePosition { document: 1, path: Vec::new(), utf16_offset: None });
        state.set(entries, resolutions);
        state.install_source_positions(positions, book.document_uris().len());
        assert_eq!(state.ordinal_of_active(), Some(2));
        let parsed = html_pipeline::parse_document(source, MarkupSyntax::Xml).unwrap();
        let text = html_pipeline::DocumentTextIndex::from_parsed(&parsed).unwrap();
        let start = text.text().find("Annotated text").unwrap();
        let position = text.source_position_in(start..start + 9).unwrap();
        let cfi = skrift_column_paginator::cfi::generate_cfi_from_source_position(0, &position);
        assert_eq!(state.ordinal_for_cfi(&cfi), Some(1));
        let tree = state.annotation_tree([state.ordinal_for_cfi(&cfi)]);
        assert!(tree.other.is_empty());
        assert_eq!(tree.branches[0].children[0].title, "Section");
    }
}
