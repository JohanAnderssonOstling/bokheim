//! Recover Foliate identity after its CFI changes and custom fields disappear.
use book_model::{Annotation, AnnotationAnchor};
use std::{
    collections::{HashMap, HashSet},
    io,
};

/// Match a complete Foliate annotation list to one book's history, including
/// tombstones. Creation times are evidence only when the old location vanished
/// and the match is unique on both sides. Quotes are never used as identity.
pub(super) fn match_foliate_annotations(incoming: &[Annotation], history: &[Annotation]) -> io::Result<Vec<Option<usize>>> {
    let locations = incoming.iter().filter_map(|a| a.anchor.epub_cfi_value().map(|cfi| (a.book_id, cfi))).collect::<HashSet<_>>();
    let mut ids = HashMap::new();
    let mut anchors = HashMap::<_, Vec<_>>::new();
    for (index, annotation) in history.iter().enumerate() {
        if !matches!(annotation.anchor.origin(), AnnotationAnchor::EpubCfi { .. }) {
            continue;
        }
        ids.insert((annotation.book_id, annotation.id.as_str()), index);
        if let Some(cfi) = annotation.anchor.epub_cfi_value() {
            anchors.entry((annotation.book_id, cfi)).or_default().push(index);
        }
    }
    let conflict = || io::Error::other("Foliate annotation identity is ambiguous; multiple annotations match a location or creation time");
    let mut used = HashSet::new();
    let mut matched = vec![None; incoming.len()];
    // Reserve every explicit identity/location before considering creation time.
    // A later record may still carry the ID of an earlier record's candidate.
    for (slot, annotation) in matched.iter_mut().zip(incoming) {
        let candidate = if let Some(index) = ids.get(&(annotation.book_id, annotation.id.as_str())) {
            Some(*index)
        } else {
            match annotation.anchor.epub_cfi_value().and_then(|cfi| anchors.get(&(annotation.book_id, cfi))).map(Vec::as_slice) {
                None | Some([]) => None,
                Some([index]) => Some(*index),
                _ => return Err(conflict()),
            }
        };
        if let Some(index) = candidate {
            if !used.insert(index) {
                return Err(conflict());
            }
            *slot = Some(index);
        }
    }
    let mut created = HashMap::<_, Vec<_>>::new();
    for (index, annotation) in history.iter().enumerate() {
        if !used.contains(&index) && annotation.created_at > 0 && matches!(annotation.anchor.origin(), AnnotationAnchor::EpubCfi { .. }) {
            if let Some(cfi) = annotation.anchor.epub_cfi_value() {
                if !locations.contains(&(annotation.book_id, cfi)) {
                    created.entry((annotation.book_id, annotation.created_at)).or_default().push(index);
                }
            }
        }
    }
    for (slot, annotation) in matched.iter_mut().zip(incoming) {
        if slot.is_some() || annotation.created_at == 0 {
            continue;
        }
        match created.get(&(annotation.book_id, annotation.created_at)).map(Vec::as_slice) {
            None | Some([]) => {}
            Some([index]) => {
                if !used.insert(*index) {
                    return Err(conflict());
                }
                *slot = Some(*index);
            }
            _ => return Err(conflict()),
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::AnnotationStyle;
    use content_address::BookId;

    fn note(id: &str, location: &str, created_at: u64) -> Annotation {
        Annotation {
            id: id.into(),
            book_id: BookId::from(blake3::hash(b"book")),
            anchor: AnnotationAnchor::epub_publication_cfi(location),
            exact_text: "Repeated quote".into(),
            note: String::new(),
            style: AnnotationStyle::Highlight,
            color: 0xffff00,
            created_at,
            modified_at: created_at,
            deleted: false,
            toc_ordinal: None,
            progress: None,
        }
    }
    #[test]
    fn moves_and_tombstones_keep_identity_without_using_the_quote() {
        let mut saved = note("saved", "old", 7);
        saved.deleted = true;
        assert_eq!(match_foliate_annotations(&[note("new-cfi-id", "moved", 7)], &[saved.clone()]).unwrap(), vec![Some(0)]);
        assert_eq!(match_foliate_annotations(&[note("another", "other", 8)], &[saved]).unwrap(), vec![None]);
    }
    #[test]
    fn existing_location_prevents_a_new_highlight_being_treated_as_a_move() {
        let saved = note("saved", "old", 7);
        assert_eq!(match_foliate_annotations(&[note("new", "new", 7), note("old-cfi-id", "old", 7)], &[saved]).unwrap(), vec![None, Some(0)]);
    }
    #[test]
    fn explicit_identity_is_reserved_before_creation_fallback() {
        let saved = note("saved", "old", 7);
        assert_eq!(match_foliate_annotations(&[note("new", "new", 7), note("saved", "moved", 7)], &[saved]).unwrap(), vec![None, Some(0)]);
    }
    #[test]
    fn ambiguous_moves_are_rejected_in_both_directions() {
        assert!(match_foliate_annotations(&[note("moved", "new", 7)], &[note("a", "old-a", 7), note("b", "old-b", 7)]).is_err());
        assert!(match_foliate_annotations(&[note("a", "new-a", 7), note("b", "new-b", 7)], &[note("old", "old", 7)]).is_err());
    }
    #[test]
    fn missing_dates_and_other_books_are_not_matched() {
        assert_eq!(match_foliate_annotations(&[note("new", "new", 0)], &[note("old", "old", 0)]).unwrap(), vec![None]);
        let mut other = note("old", "old", 7);
        other.book_id = BookId::from(blake3::hash(b"other"));
        assert_eq!(match_foliate_annotations(&[note("new", "new", 7)], &[other]).unwrap(), vec![None]);
    }
}
