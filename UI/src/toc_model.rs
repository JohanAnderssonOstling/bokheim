//! Pure data helpers for mapping the renderer's TOC entries into the UI's
//! `TocEntry` tree and resolving active-entry/anchor lookups for the reader.

use crate::components::toc_tree::TocEntry;
use html_renderer::HTMLRenderer;
use std::collections::{HashMap, HashSet};

fn map_toc_entries_with<F>(entries: Vec<html_renderer::TocEntry>, resolve: &F) -> Vec<TocEntry>
where
    F: Fn(&html_renderer::TocEntry) -> Option<(usize, Option<String>)>,
{
    entries
        .into_iter()
        .map(|entry| {
            let resolved = resolve(&entry);
            TocEntry { title: entry.title, link: entry.link, resolved, children: map_toc_entries_with(entry.children, resolve) }
        })
        .collect()
}

pub fn map_toc_entries(renderer: &HTMLRenderer, entries: Vec<html_renderer::TocEntry>) -> Vec<TocEntry> {
    map_toc_entries_with(entries, &|entry| renderer.resolve_href(entry.link.as_str()))
}

pub fn build_toc_resolution_maps(entries: &[TocEntry], resolved_set: &mut HashSet<(usize, Option<String>)>, first_by_doc: &mut HashMap<usize, (usize, Option<String>)>) {
    for entry in entries {
        if let Some(resolved) = entry.resolved.as_ref() {
            resolved_set.insert(resolved.clone());
            first_by_doc.entry(resolved.0).or_insert_with(|| resolved.clone());
        }
        if !entry.children.is_empty() {
            build_toc_resolution_maps(&entry.children, resolved_set, first_by_doc);
        }
    }
}

fn collect_toc_anchors_by_doc(entries: &[TocEntry], map: &mut HashMap<usize, Vec<String>>) {
    for entry in entries {
        if let Some((doc_index, Some(fragment))) = entry.resolved.as_ref() {
            map.entry(*doc_index).or_default().push(fragment.clone());
        }
        collect_toc_anchors_by_doc(&entry.children, map);
    }
}

pub fn build_toc_anchor_strings_by_doc(entries: &[TocEntry]) -> Vec<Vec<String>> {
    let mut map: HashMap<usize, Vec<String>> = HashMap::new();
    collect_toc_anchors_by_doc(entries, &mut map);
    let max_doc = map.keys().copied().max().unwrap_or(0);
    let mut out = vec![Vec::new(); max_doc + 1];
    for (doc, anchors) in map {
        out[doc] = anchors;
    }
    out
}

pub fn toc_title_for_resolved(entries: &[TocEntry], resolved: Option<(usize, Option<String>)>) -> Option<String> {
    let resolved = resolved?;
    toc_title_for_resolved_inner(entries, &resolved)
}

fn toc_title_for_resolved_inner(entries: &[TocEntry], resolved: &(usize, Option<String>)) -> Option<String> {
    for entry in entries {
        if let Some(entry_resolved) = entry.resolved.as_ref() {
            if entry_resolved.0 == resolved.0 && entry_resolved.1 == resolved.1 {
                return Some(entry.title.clone());
            }
        }
        if let Some(found) = toc_title_for_resolved_inner(&entry.children, resolved) {
            return Some(found);
        }
    }
    None
}
