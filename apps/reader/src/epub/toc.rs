//! The table of contents, and which of its entries the reader is inside.
//!
//! TOC links are hrefs written by the publisher; the renderer resolves each to
//! a `(document index, anchor)` pair. Highlighting the current chapter means
//! matching the renderer's position back to a link. Resolutions stay in TOC
//! order so duplicate targets and document-level fallbacks are deterministic.

use std::sync::Arc;

use html_view_core::TocEntry;

/// Where a TOC link points, as `(document index, optional anchor)`.
pub(crate) type Resolution = (usize, Option<String>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedTocEntry {
    link: String,
    document: usize,
    anchor: Option<String>,
}

#[derive(Default)]
pub(crate) struct TocState {
    entries: Arc<Vec<TocEntry>>,
    resolutions: Vec<ResolvedTocEntry>,
    /// The link the reader is currently inside, if any.
    active_link: Option<String>,
}

impl TocState {
    pub(crate) fn set(&mut self, entries: Vec<TocEntry>, resolutions: Vec<ResolvedTocEntry>) {
        self.entries = Arc::new(entries);
        self.resolutions = resolutions;
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
        true
    }

    /// Resolves every link depth-first, preserving the publisher's TOC order
    /// and skipping links the renderer cannot place.
    pub(crate) fn resolve(entries: &[TocEntry], resolve: impl Fn(&str) -> Option<Resolution>) -> Vec<ResolvedTocEntry> {
        fn visit(entries: &[TocEntry], resolve: &impl Fn(&str) -> Option<Resolution>, resolutions: &mut Vec<ResolvedTocEntry>) {
            for entry in entries {
                if let Some((document, anchor)) = resolve(&entry.link) {
                    resolutions.push(ResolvedTocEntry { link: entry.link.clone(), document, anchor });
                }
                visit(&entry.children, resolve, resolutions);
            }
        }

        let mut resolutions = Vec::new();
        visit(entries, &resolve, &mut resolutions);
        resolutions
    }

    /// Groups publisher TOC anchors for the renderer's nearest-preceding-anchor
    /// filter. Anchor order is retained and duplicate targets are omitted.
    pub(crate) fn anchor_strings_by_doc(resolutions: &[ResolvedTocEntry], document_count: usize) -> Vec<Vec<String>> {
        let mut anchors_by_doc = vec![Vec::new(); document_count];
        for resolution in resolutions {
            let Some(anchor) = resolution.anchor.as_ref() else { continue };
            let Some(document_anchors) = anchors_by_doc.get_mut(resolution.document) else { continue };
            if !document_anchors.contains(anchor) {
                document_anchors.push(anchor.clone());
            }
        }
        anchors_by_doc
    }

    /// The TOC link covering `(doc, anchor)`. Exact matches win; otherwise a
    /// document-level entry, or the first entry for that document in publisher
    /// order, supplies the deterministic fallback.
    pub(crate) fn link_at(&self, doc: usize, anchor: &Option<String>) -> Option<String> {
        self.resolutions
            .iter()
            .find(|resolved| resolved.document == doc && resolved.anchor == *anchor)
            .or_else(|| self.resolutions.iter().find(|resolved| resolved.document == doc && resolved.anchor.is_none()))
            .or_else(|| self.resolutions.iter().find(|resolved| resolved.document == doc))
            .map(|resolved| resolved.link.clone())
    }

    /// Depth-first ordinal of the entry the reader is inside, for reading
    /// position. Resolutions are stored in TOC order, so the index is the
    /// ordinal; duplicate links resolve to their first entry, matching
    /// `active_title`.
    pub(crate) fn ordinal_of_active(&self) -> Option<usize> {
        let active = self.active_link.as_ref()?;
        self.resolutions.iter().position(|resolved| &resolved.link == active)
    }

    /// A stored annotation ordinal refers to resolved TOC order, which may
    /// omit publisher entries the renderer could not place.
    pub(crate) fn title_at_ordinal(&self, ordinal: usize) -> Option<&str> {
        let link = self.resolutions.get(ordinal)?.link.as_str();
        fn find<'a>(entries: &'a [TocEntry], link: &str) -> Option<&'a str> {
            for entry in entries {
                if entry.link == link {
                    return Some(entry.title.as_str());
                }
                if let Some(title) = find(&entry.children, link) {
                    return Some(title);
                }
            }
            None
        }
        find(&self.entries, link)
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
