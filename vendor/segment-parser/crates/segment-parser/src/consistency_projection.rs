use structure::{SegmentTree, TagData, UnitKind};
use unicode_normalization::UnicodeNormalization;

pub fn consistency_projection(tree: &SegmentTree, ignore_case: bool) -> String {
    let mut projection = String::with_capacity(tree.text().len().saturating_add(tree.unit_count().saturating_mul(4)));
    let mut text = String::with_capacity(tree.text().len());
    let mut pending_whitespace = false;
    let mut has_content = false;
    tree.iter_units().for_each(|unit| {
        if unit.kind() == UnitKind::Whitespace {
            pending_whitespace = has_content;
            return;
        }
        if pending_whitespace {
            text.push(' ');
            pending_whitespace = false;
        }
        if let Some(tag) = unit.tag() {
            append_text(&mut projection, &mut text, ignore_case);
            append_tag(&mut projection, tag);
        } else {
            text.push_str(unit.text());
        }
        has_content = true;
    });
    append_text(&mut projection, &mut text, ignore_case);
    projection
}

fn normalized_text(text: &str, ignore_case: bool) -> String {
    if ignore_case {
        let normalized = text.nfc().collect::<String>();
        return crate::unicode_case_fold(&normalized).nfc().collect();
    }
    text.nfc().collect()
}

fn append_text(projection: &mut String, text: &mut String, ignore_case: bool) {
    if text.is_empty() {
        return;
    }
    projection.push('T');
    append_field(projection, &normalized_text(text, ignore_case));
    text.clear();
}

fn append_tag(projection: &mut String, tag: &TagData) {
    projection.push('G');
    projection.push(char::from(b'a'.saturating_add(tag.kind() as u8)));
    append_field(projection, tag.id());
    append_field(projection, tag.name());
    append_field(projection, tag.raw());
    append_field(projection, tag.equiv_text().unwrap_or(""));
}

fn append_field(projection: &mut String, value: &str) {
    projection.push_str(&value.len().to_string());
    projection.push(':');
    projection.push_str(value);
}
