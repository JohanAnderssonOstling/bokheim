//! Separate annotation hit targets from the quiet marks drawn over PDF text.
use crate::PdfAnnotationOverlay;
use pdf_reader_core::NormalizedRect;
use std::collections::BTreeMap;

pub(crate) fn overlaps(left: NormalizedRect, right: NormalizedRect) -> bool {
    left.left() < right.right() && right.left() < left.right() && left.top() < right.bottom() && right.top() < left.bottom()
}

/// Union overlapping note groups without repeatedly darkening their text.
/// An open note keeps its own appearance across its exact range.
pub(crate) fn quiet_overlays(annotations: &[PdfAnnotationOverlay], active: Option<&str>) -> Vec<PdfAnnotationOverlay> {
    let mut parents = (0..annotations.len()).collect::<Vec<_>>();
    fn root(parents: &[usize], mut index: usize) -> usize {
        while parents[index] != index {
            index = parents[index];
        }
        index
    }
    for (index, annotation) in annotations.iter().enumerate() {
        for (other_index, other) in annotations.iter().enumerate().skip(index + 1).take_while(|(_, other)| other.page_index == annotation.page_index) {
            if annotation.rects.iter().any(|left| other.rects.iter().any(|right| overlaps(*left, *right))) {
                let left = root(&parents, index);
                let right = root(&parents, other_index);
                parents[right] = left;
            }
        }
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for index in 0..annotations.len() {
        groups.entry(root(&parents, index)).or_default().push(index);
    }
    let mut result = Vec::new();
    for group in groups.values() {
        let leader = &annotations[group[0]];
        let mut covered = Vec::<NormalizedRect>::new();
        let mut order = group.clone();
        order.sort_by_key(|index| Some(annotations[*index].id.as_str()) != active);
        for index in order {
            let annotation = &annotations[index];
            let appearance = if Some(annotation.id.as_str()) == active { annotation } else { leader };
            let mut rects = Vec::new();
            for rect in &annotation.rects {
                let mut parts = vec![*rect];
                for cut in &covered {
                    parts = parts.into_iter().flat_map(|part| subtract(part, *cut)).collect();
                    if parts.is_empty() {
                        break;
                    }
                }
                covered.extend(parts.iter().copied());
                rects.extend(parts);
            }
            if !rects.is_empty() {
                result.push(PdfAnnotationOverlay { rects, style: appearance.style, color: appearance.color, ..annotation.clone() });
            }
        }
    }
    result.sort_by_key(|annotation| annotation.page_index);
    result
}

fn subtract(rect: NormalizedRect, cut: NormalizedRect) -> Vec<NormalizedRect> {
    if !overlaps(rect, cut) {
        return vec![rect];
    }
    let left = rect.left().max(cut.left());
    let right = rect.right().min(cut.right());
    let top = rect.top().max(cut.top());
    let bottom = rect.bottom().min(cut.bottom());
    [(rect.left(), rect.top(), rect.width(), top - rect.top()), (rect.left(), bottom, rect.width(), rect.bottom() - bottom), (rect.left(), top, left - rect.left(), bottom - top), (right, top, rect.right() - right, bottom - top)]
        .into_iter()
        .filter(|(_, _, width, height)| *width > 0.0 && *height > 0.0)
        .map(|(left, top, width, height)| NormalizedRect::new(left, top, width, height))
        .collect()
}
