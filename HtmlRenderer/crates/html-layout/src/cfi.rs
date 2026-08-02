//! EPUB CFI operations that require shaped document state.

use html_dom::{DomNodeId, NodeRef};

use crate::LaidOutDocument;

pub use html_dom::cfi::parse_cfi_spine_only;

pub fn generate_cfi(doc: &LaidOutDocument, doc_index: usize, glyph_idx: u32) -> Option<String> {
    let (text_node_idx, char_offset) = doc.get_dom_node_for_glyph(glyph_idx)?;
    let text_node = doc.document().node_id_from_raw(text_node_idx)?;
    let parent_idx = doc.document().node_ref(text_node)?.parent()?;
    let text_step = doc.document().get_text_node_step(text_node)?;
    let path = doc.document().get_dom_path_to_node(parent_idx);
    if path.is_empty() {
        return None;
    }

    let mut cfi = format!("epubcfi(/6/{}!", (doc_index + 1) * 2);
    for &node in path.iter().skip(1) {
        if let Some(step) = doc.document().get_element_step(node) {
            cfi.push_str(&format!("/{step}"));
            if let Some(id) = doc.document().get_dom_id(node) {
                cfi.push_str(&format!("[{id}]"));
            }
        }
    }
    cfi.push_str(&format!("/{text_step}:{char_offset})"));
    Some(cfi)
}

pub fn parse_cfi(doc: &LaidOutDocument, cfi: &str) -> Option<(usize, u32)> {
    let doc_index = parse_cfi_spine_only(cfi)?;
    let inner = cfi.strip_prefix("epubcfi(")?.strip_suffix(')')?;
    let (_, doc_part) = inner.split_once('!')?;
    Some((doc_index, parse_document_position(doc, doc_part)?))
}

fn parse_document_position(doc: &LaidOutDocument, doc_part: &str) -> Option<u32> {
    if doc_part.is_empty() {
        return Some(0);
    }
    let segments: Vec<&str> = doc_part.split('/').filter(|segment| !segment.is_empty()).collect();
    if segments.is_empty() {
        return Some(0);
    }
    let (path_segments, char_offset) = match segments.last()?.split_once(':') {
        Some((step, offset)) => {
            let mut path = segments.clone();
            *path.last_mut()? = step;
            (path, Some(offset.parse().ok()?))
        }
        None => (segments, None),
    };
    let steps: Vec<usize> = path_segments.iter().filter_map(|segment| segment.split('[').next()?.parse().ok()).collect();
    let (element_steps, text_step) = match steps.last().copied() {
        Some(last) if last % 2 != 0 => (&steps[..steps.len() - 1], Some(last)),
        _ => (&steps[..], None),
    };
    let parent = if element_steps.is_empty() { doc.document().dom_root()? } else { doc.document().find_dom_node_by_steps(element_steps)? };
    let target = text_step.and_then(|step| doc.document().find_text_node_by_step(parent, step)).unwrap_or(parent);
    find_glyph_in_node(doc, target, char_offset)
}

fn find_glyph_in_node(doc: &LaidOutDocument, node_idx: DomNodeId, char_offset: Option<usize>) -> Option<u32> {
    match doc.document().node_ref(node_idx)? {
        NodeRef::Text(_) => doc.render_view().text_runs().find_map(|run| (run.dom_text_node() == Some(node_idx.raw())).then(|| apply_offset(run.glyphs().start, run.glyphs().end, char_offset))),
        NodeRef::Element(element) => {
            if let Some(glyph) = doc.render_view().text_runs().find_map(|run| {
                let Some(box_node_idx) = doc.box_dom_element_idx(run.box_idx()).and_then(|node| doc.document().node_id_from_raw(node)) else {
                    return None;
                };
                (box_node_idx == node_idx).then(|| apply_offset(run.glyphs().start, run.glyphs().end, char_offset))
            }) {
                return Some(glyph);
            }
            element.children().find_map(|child| find_glyph_in_node(doc, child, None))
        }
    }
}

fn apply_offset(start: u32, end: u32, char_offset: Option<usize>) -> u32 {
    char_offset.map(|offset| (start + offset as u32).min(end.saturating_sub(1))).unwrap_or(start)
}
