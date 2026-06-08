use crate::document::{Document, InlineKind, TextTransform, WhiteSpace};
use crate::layout::get_style;
use crate::text::glyph_cache::GlyphCache;

/// Shape all text in the document using computed styles.
/// This collapses whitespace based on white-space mode, then
/// replaces char codes in doc.box_tree.glyphs with actual glyph indices.
/// Must be called after style resolution.
pub fn shape_document(doc: &mut Document, glyph_cache: &mut GlyphCache) {
    if doc.box_tree().glyphs.is_empty() {
        return;
    }

    // First: collapse whitespace based on white_space style
    collapse_whitespace(doc);

    if doc.box_tree().glyphs.is_empty() {
        return;
    }

    // Build box_family lookup
    let mut box_family: Vec<Option<String>> = vec![None; doc.box_count()];
    for idx in 0..doc.box_count() {
        let style = get_style(doc, idx);
        if let Some(family_idx) = style.font_family() {
            box_family[idx] = Some(doc.string(family_idx).to_string());
        }
    }

    // Replace each char code with its shaped glyph index. Iterate run by run so
    // the owning box (and thus its style) is resolved once per run rather than
    // per glyph, and no glyph→box map is needed: a run's glyphs all share a box.
    let mut prev_char: Option<char> = None;
    let mut prev_box: Option<usize> = None;
    for (box_idx, range) in doc.text_runs() {
        // Snapshot the style values up front so the immutable style borrow ends
        // before the glyph buffer is mutated below.
        let style = get_style(doc, box_idx);
        let small_caps = style.font_variant_small_caps();
        let transform = style.text_transform();
        let font_size = style.font_size();
        let font_weight = style.font_weight();
        let font_style = style.font_style();
        let color = style.color();
        let font_family_idx = style.font_family();
        let family = box_family[box_idx].as_deref();

        for i in range {
            let char_code = doc.box_tree().glyphs[i as usize];
            let mut c = char::from_u32(char_code as u32).unwrap_or('?');

            // Apply font variant small caps
            if small_caps && c.is_alphabetic() {
                c = c.to_uppercase().next().unwrap_or(c);
            }

            // Apply text transform
            match transform {
                TextTransform::Uppercase => {
                    c = c.to_uppercase().next().unwrap_or(c);
                }
                TextTransform::Lowercase => {
                    c = c.to_lowercase().next().unwrap_or(c);
                }
                TextTransform::Capitalize => {
                    let boundary = prev_box != Some(box_idx) || prev_char.map(|p| p.is_whitespace() || p == '\n').unwrap_or(true);
                    if boundary && c.is_alphabetic() {
                        c = c.to_uppercase().next().unwrap_or(c);
                    }
                }
                TextTransform::None => {}
            }

            let glyph_idx = glyph_cache.shape_with_font_properties(c, font_size, font_weight, font_style, color, font_family_idx, family);
            doc.box_tree_mut().glyphs[i as usize] = glyph_idx;
            prev_char = Some(c);
            prev_box = Some(box_idx);
        }
    }
}

/// Collapse whitespace in doc.box_tree.glyphs based on each character's box white_space style.
/// Updates inline text run ranges to match the new glyph indices.
fn collapse_whitespace(doc: &mut Document) {
    let mut new_glyphs: Vec<u16> = Vec::with_capacity(doc.box_tree().glyphs.len());
    let mut remap: Vec<u32> = vec![u32::MAX; doc.box_tree().glyphs.len()]; // old index -> new index

    let mut prev_was_space = false;
    let mut at_line_start = true;

    // Iterate run by run (in glyph order); each run's box gives the white-space
    // mode for its whole glyph range, so no per-glyph box map is required.
    for (box_idx, range) in doc.text_runs() {
        let white_space = get_style(doc, box_idx).white_space();
        let collapse = matches!(white_space, WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine);
        let preserve_newlines = matches!(white_space, WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine);

        for i in range {
            let i = i as usize;
            let char_code = doc.box_tree().glyphs[i];
            let c = char::from_u32(char_code as u32).unwrap_or('?');

            if c == '\n' {
                if preserve_newlines {
                    // Keep the newline
                    remap[i] = new_glyphs.len() as u32;
                    new_glyphs.push('\n' as u16);
                    prev_was_space = false;
                    at_line_start = true;
                } else if collapse {
                    // Treat newline as space for collapsing
                    if !prev_was_space && !at_line_start {
                        remap[i] = new_glyphs.len() as u32;
                        new_glyphs.push(' ' as u16);
                        prev_was_space = true;
                    }
                    // else: skip (collapsed)
                }
            } else if c.is_whitespace() && c != '\u{00A0}' {
                // Regular whitespace (space, tab, etc.) but not nbsp
                if collapse {
                    // Collapse: skip if at line start or after another space
                    if !prev_was_space && !at_line_start {
                        remap[i] = new_glyphs.len() as u32;
                        new_glyphs.push(' ' as u16);
                        prev_was_space = true;
                    }
                    // else: skip (collapsed)
                } else {
                    // pre or pre-wrap: preserve whitespace
                    remap[i] = new_glyphs.len() as u32;
                    new_glyphs.push(char_code);
                    prev_was_space = c == ' ' || c == '\t';
                    at_line_start = false;
                }
            } else {
                // Non-whitespace character
                remap[i] = new_glyphs.len() as u32;
                new_glyphs.push(char_code);
                prev_was_space = false;
                at_line_start = false;
            }
        }
    }

    // Update inline text runs with new glyph ranges
    for run in &mut doc.box_tree_mut().inline_runs {
        let InlineKind::Text { glyphs } = &mut run.kind else {
            continue;
        };
        let old_start = glyphs.start as usize;
        let old_end = glyphs.end as usize;

        // Find new start: first remapped index that's valid
        let mut new_start = u32::MAX;
        for i in old_start..old_end {
            if i < remap.len() && remap[i] != u32::MAX {
                new_start = remap[i];
                break;
            }
        }

        // Find new end: last remapped index + 1
        let mut new_end = new_start;
        for i in old_start..old_end {
            if i < remap.len() && remap[i] != u32::MAX {
                new_end = remap[i] + 1;
            }
        }

        if new_start == u32::MAX {
            // All characters were collapsed, make empty run
            *glyphs = 0..0;
        } else {
            *glyphs = new_start..new_end;
        }
    }

    doc.box_tree_mut().glyphs = new_glyphs;
}
