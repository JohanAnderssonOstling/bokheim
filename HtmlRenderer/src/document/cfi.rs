//! EPUB Canonical Fragment Identifier (CFI) support
//!
//! Format: epubcfi(/6/4[id]!/4/2/1:25)
//! - /6    = <spine> element in the OPF package document
//! - /4    = second <itemref> in the spine (even step = element index)
//! - !     = indirection into the referenced content document
//! - /4/2  = path through DOM element children (even steps only)
//! - /1:25 = first text node child (odd step), character offset 25

use super::{DocumentState, InlineKind, NodeRef};

/// Generate an EPUB CFI for a glyph position in a document.
pub fn generate_cfi(doc: &DocumentState, doc_index: usize, glyph_idx: u32) -> Option<String> {
    let (text_node_idx, char_offset) = doc.get_dom_node_for_glyph(glyph_idx)?;

    // Walk up to the parent element to build the element path.
    let parent_idx = doc.node_ref(text_node_idx as usize)?.parent()? as u32;
    let text_step = doc.get_text_node_step(text_node_idx)?;
    let path = doc.get_dom_path_to_node(parent_idx);
    if path.is_empty() {
        return None;
    }

    let mut cfi = String::from("epubcfi(");

    // /6/N! — spine element + itemref step + indirection
    let spine_step = (doc_index + 1) * 2;
    cfi.push_str(&format!("/6/{}!", spine_step));

    // Path through DOM to parent element (skip the root node at path[0])
    for i in 1..path.len() {
        let node = path[i];
        if let Some(step) = doc.get_element_step(node) {
            cfi.push_str(&format!("/{}", step));
            if let Some(id) = doc.get_dom_id(node as usize) {
                cfi.push_str(&format!("[{}]", id));
            }
        }
    }

    // Odd step for the text node + character offset
    cfi.push_str(&format!("/{}:{}", text_step, char_offset));

    cfi.push(')');
    Some(cfi)
}

/// Parse just the spine doc index from a CFI without needing the content document.
pub fn parse_cfi_spine_only(cfi: &str) -> Option<usize> {
    let inner = cfi.strip_prefix("epubcfi(")?.strip_suffix(')')?;
    let (spine_part, _) = inner.split_once('!')?;
    parse_spine_position(spine_part)
}

/// Parse a CFI and return (doc_index, glyph_idx).
pub fn parse_cfi(doc: &DocumentState, cfi: &str) -> Option<(usize, u32)> {
    let inner = cfi.strip_prefix("epubcfi(")?.strip_suffix(')')?;
    let (spine_part, doc_part) = inner.split_once('!')?;
    let doc_index = parse_spine_position(spine_part)?;
    let glyph_idx = parse_document_position(doc, doc_part)?;
    Some((doc_index, glyph_idx))
}

/// Parse /6/N into a zero-based doc index.
fn parse_spine_position(spine_part: &str) -> Option<usize> {
    // Expected: /6/N (spine element step then itemref step)
    let rest = spine_part.strip_prefix("/6/")?;
    // Strip any ID assertion on the itemref
    let step_str = rest.split('[').next()?;
    let step: usize = step_str.parse().ok()?;
    if step % 2 != 0 || step < 2 {
        return None;
    }
    Some((step / 2) - 1)
}

/// Parse the content-document part of a CFI (after '!') into a glyph index.
fn parse_document_position(doc: &DocumentState, doc_part: &str) -> Option<u32> {
    if doc_part.is_empty() {
        return Some(0);
    }

    let segments: Vec<&str> = doc_part.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Some(0);
    }

    // Check whether the last segment carries a character offset ("step:offset").
    let (path_segments, char_offset) = match segments.last()?.split_once(':') {
        Some((step_str, offset_str)) => {
            let offset: usize = offset_str.parse().ok()?;
            let mut segs = segments.clone();
            *segs.last_mut()? = step_str;
            (segs, Some(offset))
        }
        None => (segments, None),
    };

    // Parse numeric steps, stripping ID assertions like "[chap01]".
    let steps: Vec<usize> = path_segments.iter().filter_map(|seg| seg.split('[').next()?.parse().ok()).collect();

    // Split into element steps (even) and the optional trailing text-node step (odd).
    let (element_steps, text_step) = match steps.last().copied() {
        Some(last) if last % 2 != 0 => (&steps[..steps.len() - 1], Some(last)),
        _ => (&steps[..], None),
    };

    // Navigate to the parent element.
    let parent_node = if element_steps.is_empty() { doc.dom_root()? as u32 } else { doc.find_dom_node_by_steps(element_steps)? };

    // Resolve the specific text node if a text step was given.
    let target = match text_step {
        Some(ts) => doc.find_text_node_by_step(parent_node, ts).unwrap_or(parent_node),
        None => parent_node,
    };

    find_glyph_in_node(doc, target, char_offset)
}

/// Find the first glyph in `node_idx`, optionally offset by `char_offset` characters.
/// Handles both text nodes (searched via dom_text_node) and element nodes (via dom_element).
fn find_glyph_in_node(doc: &DocumentState, node_idx: u32, char_offset: Option<usize>) -> Option<u32> {
    match doc.node_ref(node_idx as usize)? {
        NodeRef::Text(_) => {
            for run in doc.inline_runs() {
                if let InlineKind::Text { glyphs } = &run.kind {
                    if run.dom_text_node == Some(node_idx) {
                        return Some(apply_offset(glyphs.start, glyphs.end, char_offset));
                    }
                }
            }
            None
        }
        NodeRef::Element(element) => {
            // Try to find a run directly owned by this element's box.
            for run in doc.inline_runs() {
                if let InlineKind::Text { glyphs } = &run.kind {
                    if doc.box_dom_element_idx(run.box_idx as usize) == Some(node_idx) {
                        return Some(apply_offset(glyphs.start, glyphs.end, char_offset));
                    }
                }
            }
            // Recurse into children (char_offset only applies at the leaf).
            for child in element.children() {
                if let Some(g) = find_glyph_in_node(doc, child as u32, None) {
                    return Some(g);
                }
            }
            None
        }
    }
}

fn apply_offset(start: u32, end: u32, char_offset: Option<usize>) -> u32 {
    match char_offset {
        Some(off) => (start + off as u32).min(end.saturating_sub(1)),
        None => start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spine_position() {
        assert_eq!(parse_spine_position("/6/2"), Some(0));
        assert_eq!(parse_spine_position("/6/4"), Some(1));
        assert_eq!(parse_spine_position("/6/10"), Some(4));
        assert_eq!(parse_spine_position("/6/2[chap01]"), Some(0));
        assert_eq!(parse_spine_position("/6/1"), None); // odd
        assert_eq!(parse_spine_position("/6/0"), None); // too small
        assert_eq!(parse_spine_position("/2"), None); // missing /6/ prefix
    }
}
