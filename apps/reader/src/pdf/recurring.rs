//! Disposable matches over cached PDF text, never persisted or exported.
use book_model::Annotation;
use content_address::BookId;
use html::text::TextPatternMatch;
use pdf_reader_core::NormalizedRect;

#[derive(Default)]
pub(super) struct RecurringState {
    pub(super) occurrences: Vec<Occurrence>,
    pub(super) editor_overlay: Option<String>,
    pub(super) editor_passage: Option<pdf_view_gpui::PdfSelectionSnapshot>,
}

pub(super) struct Occurrence {
    pub(super) annotation_id: String,
    pub(super) overlay: pdf_view_gpui::PdfAnnotationOverlay,
    pub(super) exact_text: String,
}

pub(super) fn matched_page_occurrences<'a>(matches: impl Iterator<Item = (&'a Annotation, TextPatternMatch)>, current_book: BookId, page: usize, characters: &[(char, Option<NormalizedRect>)], chapter: impl Fn(usize) -> usize) -> Vec<Occurrence> {
    let mut result = Vec::new();
    for (annotation, found) in matches {
        let origin_page = annotation.anchor.pdf_anchor().map(|anchor| anchor.page_index as usize);
        if !crate::shell::recurring::applies_at(annotation, current_book, origin_page.map(&chapter), Some(chapter(page)), origin_page == Some(page)) {
            continue;
        }
        let start = found.range.start;
        let matched = &characters[found.range];
        if matched.iter().any(|(_, bounds)| bounds.is_none()) {
            continue;
        }
        let mut rects: Vec<NormalizedRect> = Vec::new();
        for bounds in matched.iter().filter(|(character, _)| !character.is_whitespace()).filter_map(|(_, bounds)| *bounds) {
            if let Some(last) = rects.last_mut().filter(|last| {
                let overlap = last.bottom().min(bounds.bottom()) - last.top().max(bounds.top());
                overlap >= last.height().min(bounds.height()) * 0.5
            }) {
                *last = last.union(bounds);
            } else {
                rects.push(bounds);
            }
        }
        if rects.is_empty() {
            continue;
        }
        if annotation.content_hash == current_book && annotation.anchor.pdf_anchor().is_some_and(|anchor| anchor.page_index as usize == page && anchor.rects.first().is_some_and(|origin| (origin.left - rects[0].left()).abs() < 0.005 && (origin.top - rects[0].top()).abs() < 0.005)) {
            continue;
        }
        let overlay = super::marks::annotation_overlay_at(annotation, format!("recurring:{}:{page}:{start}", annotation.id), page, rects);
        let exact_text = matched.iter().map(|(character, _)| *character).collect();
        result.push(Occurrence { annotation_id: annotation.id.clone(), overlay, exact_text });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, RecurringScope};

    fn page_occurrences(annotations: &[&Annotation], current_book: BookId, page: usize, characters: &[(char, Option<NormalizedRect>)], chapter: impl Fn(usize) -> usize) -> Vec<Occurrence> {
        let mut matcher = crate::shell::recurring::RecurringMatcher::default();
        matcher.update(annotations.iter().copied());
        let text = characters.iter().map(|(character, _)| *character).collect::<String>();
        let matches = matcher.matching_annotations(&text, annotations.iter().copied());
        matched_page_occurrences(matches, current_book, page, characters, chapter)
    }

    fn annotation(page: u32, scope: RecurringScope) -> Annotation {
        Annotation {
            id: "name".into(),
            content_hash: BookId::from_bytes([1; 32]),
            anchor: AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(page, vec![PdfAnnotationRect::new(0.1, 0.2, 0.09, 0.02)], None)).with_recurring_scope(Some(scope)),
            exact_text: "Elizabeth".into(),
            style: AnnotationStyle::Highlight,
            color: 0xffcc00,
            note: "One shared note".into(),
            created_at: 1,
            toc_ordinal: Some(1),
            progress: Some(20.0),
            modified_at: 2,
            deleted: false,
        }
    }

    #[test]
    fn recurring_pdf_matches_skip_originals_and_partial_words_and_respect_chapters() {
        let text = "Elizabeth ELIZABETH Elizabethan".chars().enumerate().map(|(index, character)| (character, Some(NormalizedRect::new(0.1 + index as f32 * 0.01, 0.2, 0.01, 0.02)))).collect::<Vec<_>>();
        let note = annotation(0, RecurringScope::Book);
        let matches = page_occurrences(&[&note], note.content_hash, 0, &text, |page| page / 2);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].annotation_id, note.id);
        assert_eq!(matches[0].exact_text, "ELIZABETH");
        assert_eq!(matches[0].overlay.page_index, 0);
        assert_eq!(note.anchor.pdf_anchor().unwrap().page_index, 0);
        let inherited = page_occurrences(&[&note], BookId::from_bytes([2; 32]), 0, &text, |page| page / 2);
        assert!(inherited.is_empty(), "book-scoped notes stay in their own book");
        let library = annotation(0, RecurringScope::Library);
        assert_eq!(page_occurrences(&[&library], BookId::from_bytes([2; 32]), 0, &text, |page| page / 2).len(), 2);
        let chapter = annotation(2, RecurringScope::Chapter);
        assert!(page_occurrences(&[&chapter], chapter.content_hash, 0, &text, |page| page / 2).is_empty());
        assert_eq!(page_occurrences(&[&chapter], chapter.content_hash, 3, &text, |page| page / 2).len(), 2);
    }

    #[test]
    fn recurring_pdf_matches_reject_unpainted_context_and_cut_off_words() {
        let note = annotation(0, RecurringScope::Library);
        let other_book = BookId::from_bytes([2; 32]);
        let rect = NormalizedRect::new(0.1, 0.2, 0.01, 0.02);
        let hidden = "Elizabeth".chars().map(|character| (character, None)).collect::<Vec<_>>();
        assert!(page_occurrences(&[&note], other_book, 0, &hidden, |_| 0).is_empty());
        let clipped = "Elizabethan".chars().enumerate().map(|(index, character)| (character, (index < 9).then_some(rect))).collect::<Vec<_>>();
        assert!(page_occurrences(&[&note], other_book, 0, &clipped, |_| 0).is_empty(), "the offscreen suffix retains whole-word boundaries");
        let visible = "Elizabeth ".chars().enumerate().map(|(index, character)| (character, (index < 9).then_some(rect))).collect::<Vec<_>>();
        assert_eq!(page_occurrences(&[&note], other_book, 0, &visible, |_| 0).len(), 1);
    }
}
