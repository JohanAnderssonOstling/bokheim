//! Disposable recurring matches on the currently composed reader page.
use book_model::{Annotation, AnnotationAnchor};
use content_address::BookId;
use gpui::Context;
use html::text::TextPatternMatch;
use skrift_column_paginator::{FrameGeometryCacheKey, RendererAnnotation, RendererCommand};

use super::{LoadState, ReaderView, Selection, marks, toc::TocState};

pub(super) struct Occurrence {
    pub(super) annotation_id: String,
    pub(super) overlay: RendererAnnotation,
}

#[derive(Default)]
pub(super) struct RecurringState {
    page: Option<(usize, FrameGeometryCacheKey)>,
    matched_annotations: Vec<(String, BookId, AnnotationAnchor, String)>,
    pub(super) occurrences: Vec<Occurrence>,
    pub(super) editor_overlay: Option<String>,
    pub(super) editor_passage: Option<Selection>,
}

impl ReaderView {
    pub(in crate::epub) fn project_annotations(&self, cx: &mut Context<Self>) {
        let mut overlays = marks::overlays(self.marks.annotations(), &self.book);
        overlays.extend(self.recurring.occurrences.iter().filter_map(|occurrence| {
            let annotation = self.marks.annotation(&occurrence.annotation_id)?;
            let mut overlay = occurrence.overlay.clone();
            (overlay.style, overlay.color) = marks::renderer_appearance(annotation);
            Some(overlay)
        }));
        self.with_renderer(cx, |renderer, cx| renderer.apply(RendererCommand::SetAnnotations(overlays), cx));
    }

    pub(in crate::epub) fn refresh_recurring_annotations(&mut self, force: bool, cx: &mut Context<Self>) {
        let LoadState::Ready(renderer) = &self.load_state else { return };
        let renderer = renderer.read(cx);
        let page = renderer.visible_page_key();
        let signature = self.marks.recurring_annotations().map(|annotation| (annotation.id.clone(), annotation.book_id, annotation.anchor.clone(), annotation.exact_text.clone())).collect::<Vec<_>>();
        if !force && self.recurring.page == page && self.recurring.matched_annotations == signature {
            self.project_annotations(cx);
            return;
        }
        self.recurring.page = page;
        self.recurring.matched_annotations = signature;
        self.recurring.occurrences = if self.recurring.matched_annotations.is_empty() {
            Vec::new()
        } else if let Some(text) = renderer.visible_source_text() {
            matched_page_occurrences(self.marks.recurring_matches(text.text()), self.locator.book_id(), &self.book, &self.toc, text.document, text.text(), |start, end| {
                let range = text.glyph_range(start, end)?;
                self.book.cfi.to_publication(&renderer.source_cfi_range(range)?)
            })
        } else {
            Vec::new()
        };
        self.project_annotations(cx);
    }
}

fn matched_page_occurrences<'a>(
    matches: impl Iterator<Item = (&'a Annotation, TextPatternMatch)>, current_book: BookId, book: &crate::book::NativeBook, toc: &TocState, document: usize, text: &str, cfi_range: impl Fn(usize, usize) -> Option<String>,
) -> Vec<Occurrence> {
    let mut result = Vec::new();
    for (annotation, found) in matches {
        let start = found.range.start;
        let Some(range) = cfi_range(start, found.range.end) else { continue };
        if !occurrence_applies(annotation, current_book, book, toc, &range) {
            continue;
        }
        let origin = book.annotation_cfi(&annotation.anchor);
        let origin_position = origin.as_deref().and_then(|origin| book.cfi.parse_source_position(origin));
        let position = book.cfi.parse_source_position(&range);
        if annotation.book_id == current_book && position == origin_position {
            continue;
        }
        let exact_text = &text[found.byte_range];
        let overlay = marks::renderer_annotation_at(annotation, format!("recurring:{}:{document}:{start}", annotation.id), range, exact_text.to_owned());
        result.push(Occurrence { annotation_id: annotation.id.clone(), overlay });
    }
    result
}

fn occurrence_applies(annotation: &Annotation, current_book: BookId, book: &crate::book::NativeBook, toc: &TocState, range: &str) -> bool {
    let origin = book.annotation_cfi(&annotation.anchor);
    let origin_chapter = origin.as_deref().and_then(|origin| toc.chapter_for_cfi(origin));
    let same_document = origin.as_deref().and_then(|origin| book.cfi.document_index(origin)).is_some_and(|document| Some(document) == book.cfi.document_index(range));
    crate::shell::recurring::applies_at(annotation, current_book, origin_chapter, toc.chapter_for_cfi(range), same_document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_content::BookToc;
    use book_model::BookTocEntry;
    use book_model::{AnnotationAnchor, AnnotationStyle, RecurringScope};
    use content_address::BookId;
    use gpui::{TestAppContext, px, size};
    use html::document::{DocumentTextIndex, MarkupSyntax, parse_document};
    use html_view_gpui::HtmlView;
    use skrift_column_paginator::{RendererInitialConfig, ResourceProvider};
    use skrift_column_paginator::{SourcePosition, cfi};
    use std::{io, sync::Arc};

    struct Chapters;

    impl ResourceProvider for Chapters {
        fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
            let count = if uri == "long.html" { 100 } else { 3 };
            let text = (0..count).map(|index| format!("<p>Chapter paragraph {index}.</p>")).collect::<String>();
            Ok(format!("<html><head><style>body {{ margin: 24px }} p {{ margin: 16px 0; line-height: 28px }}</style></head><body>{text}</body></html>").into_bytes())
        }

        fn exists(&self, _: &str) -> bool {
            true
        }

        fn resolve(&self, _: &str, href: &str) -> String {
            href.to_owned()
        }

        fn list_html_candidates(&self, _: &str) -> io::Result<Vec<String>> {
            Ok(vec!["short.html".into(), "next.html".into()])
        }
    }

    #[gpui::test]
    fn recurring_source_text_follows_page_turns_and_reflow(cx: &mut TestAppContext) {
        let config = RendererInitialConfig { max_column_count: Some(1), ..Default::default() };
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(Chapters), vec!["long.html".into()], 0, None, config, window, cx));
        visual.simulate_resize(size(px(600.0), px(260.0)));
        visual.run_until_parked();
        let (first_key, first_text) = document.read_with(visual, |view, _| {
            let text = view.visible_source_text().unwrap();
            assert!(text.text().contains("Chapter paragraph 0."));
            assert!(!text.text().contains("Chapter paragraph 99."));
            (view.visible_page_key(), text.text().to_owned())
        });
        document.update(visual, |view, cx| view.next_page(cx));
        visual.run_until_parked();
        document.read_with(visual, |view, _| {
            assert_ne!(view.visible_page_key(), first_key);
            let text = view.visible_source_text().unwrap();
            assert!(!text.text().contains("Chapter paragraph 0."));
            assert!(!text.text().contains("Chapter paragraph 99."));
        });
        document.update(visual, |view, cx| view.previous_page(cx));
        visual.run_until_parked();
        visual.simulate_resize(size(px(600.0), px(520.0)));
        visual.run_until_parked();
        document.read_with(visual, |view, _| {
            assert_ne!(view.visible_page_key(), first_key, "reflow at the same anchor must refresh matches");
            let text = view.visible_source_text().unwrap();
            assert!(text.text().contains("Chapter paragraph 0."));
            assert!(text.text().len() > first_text.len());
        });
    }

    struct InlineNames;

    impl ResourceProvider for InlineNames {
        fn read_bytes(&self, _: &str) -> io::Result<Vec<u8>> {
            Ok("<html><body><p>Éva <em>😀</em> met Elizabeth.</p><p style='display:none'>InvisibleName</p><p style='visibility:hidden'>HiddenName</p></body></html>".as_bytes().to_vec())
        }
        fn exists(&self, _: &str) -> bool {
            true
        }
        fn resolve(&self, _: &str, href: &str) -> String {
            href.to_owned()
        }
        fn list_html_candidates(&self, _: &str) -> io::Result<Vec<String>> {
            Ok(vec!["names.html".into()])
        }
    }

    #[gpui::test]
    fn recurring_visible_ranges_keep_inline_markup_unicode_and_source_addresses(cx: &mut TestAppContext) {
        let (document, visual) = cx.add_window_view(|window, cx| HtmlView::from_provider(Arc::new(InlineNames), vec!["names.html".into()], 0, None, RendererInitialConfig::default(), window, cx));
        visual.simulate_resize(size(px(600.0), px(500.0)));
        visual.run_until_parked();
        document.read_with(visual, |view, _| {
            let text = view.visible_source_text().unwrap();
            assert!(!text.text().contains("InvisibleName"));
            assert!(!text.text().contains("HiddenName"));
            let byte = text.text().find("Éva 😀").expect("phrases cross inline markup");
            let start = text.text()[..byte].chars().count();
            let glyphs = text.glyph_range(start, start + "Éva 😀".chars().count()).unwrap();
            let cfi = view.source_cfi_range(glyphs).unwrap();
            assert!(cfi.ends_with(":2)"), "the non-BMP endpoint uses UTF-16: {cfi}");
            assert!(skrift_column_paginator::cfi::parse_source_position(&cfi).is_some());
        });
    }

    fn text_cfi_range(text: &DocumentTextIndex, document: usize, start: usize, end: usize) -> Option<String> {
        let first = text.source_position_in(start..end)?;
        let last = text.source_position_in(end.checked_sub(1)?..end)?;
        let from = cfi::generate_cfi_from_source_position(document, &first);
        let last_path = last.element_steps().iter().map(|step| format!("/{step}")).collect::<String>();
        let last_character = text.text_range(end - 1..end)?.chars().next()?;
        let to = format!("{last_path}/{}:{}", last.text_step(), last.utf16_offset() + last_character.len_utf16());
        let (_, from) = from.strip_prefix("epubcfi(")?.strip_suffix(')')?.split_once('!')?;
        let from_parts = from.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
        let to_parts = to.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
        let common = from_parts.iter().zip(&to_parts).take(from_parts.len().min(to_parts.len()).saturating_sub(1)).take_while(|(left, right)| left == right).count();
        let base = if common == 0 { String::new() } else { format!("/{}", from_parts[..common].join("/")) };
        Some(format!("epubcfi(/6/{}!{base},/{},/{})", (document + 1) * 2, from_parts[common..].join("/"), to_parts[common..].join("/")))
    }

    fn page_occurrences<'a>(
        annotations: impl Iterator<Item = &'a Annotation>, current_book: BookId, book: &crate::book::NativeBook, toc: &TocState, document: usize, text: &str, cfi_range: impl Fn(usize, usize) -> Option<String>,
    ) -> Vec<Occurrence> {
        let annotations = annotations.collect::<Vec<_>>();
        let mut matcher = crate::shell::recurring::RecurringMatcher::default();
        matcher.update(annotations.iter().copied());
        let matches = matcher.matching_annotations(text, annotations.iter().copied());
        matched_page_occurrences(matches, current_book, book, toc, document, text, cfi_range)
    }

    fn document_occurrences(annotations: &[Annotation], current_book: BookId, book: &crate::book::NativeBook, document: usize, text: &DocumentTextIndex) -> Vec<Occurrence> {
        page_occurrences(annotations.iter(), current_book, book, &TocState::default(), document, text.text(), |start, end| text_cfi_range(text, document, start, end))
    }

    fn book() -> crate::book::NativeBook {
        crate::book::NativeBook::from_reader(
            book_content::BookFormat::Fb2,
            std::io::Cursor::new(br#"<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0"><description><title-info><book-title>Test</book-title><lang>en</lang></title-info></description><body><section><p>Elizabeth</p></section></body></FictionBook>"#.to_vec()),
        )
        .unwrap()
    }

    fn recurring_annotation(book: BookId, anchor: AnnotationAnchor, scope: RecurringScope) -> Annotation {
        Annotation {
            id: "shared-name".into(),
            book_id: book,
            anchor: anchor.with_recurring_scope(Some(scope)),
            exact_text: "Elizabeth".into(),
            style: AnnotationStyle::Highlight,
            color: 0xffcc00,
            note: "Shared note".into(),
            created_at: 1,
            toc_ordinal: Some(1),
            progress: Some(40.0),
            modified_at: 2,
            deleted: false,
        }
    }

    #[test]
    fn matches_are_inline_only_case_insensitive_whole_words_and_keep_the_creation_anchor() {
        let parsed = parse_document("<html><body><p>Elizabeth met ELIZABETH and Elizabethan.</p><p>Elizabeth left.</p></body></html>", MarkupSyntax::Html).unwrap();
        let text = DocumentTextIndex::from_parsed(&parsed).unwrap();
        let origin = AnnotationAnchor::epub_cfi(text_cfi_range(&text, 0, 0, 9).unwrap());
        let book = book();
        let id = BookId::from_bytes([1; 32]);
        let annotation = recurring_annotation(id, origin.clone(), RecurringScope::Book);
        let matches = document_occurrences(&[annotation.clone()], id, &book, 0, &text);
        assert_eq!(matches.len(), 2, "skip the original and partial-word matches");
        assert_eq!(matches[0].overlay.exact_text, "ELIZABETH");
        assert_eq!(matches[1].overlay.exact_text, "Elizabeth");
        assert!(matches.iter().all(|item| item.annotation_id == annotation.id && item.overlay.id != annotation.id));
        assert_eq!(annotation.anchor.origin(), &origin);
        let other_book = BookId::from_bytes([2; 32]);
        assert!(document_occurrences(std::slice::from_ref(&annotation), other_book, &book, 0, &text).is_empty(), "book-scoped notes stay in their origin book");
        let mut annotation = annotation;
        annotation.anchor = annotation.anchor.with_recurring_scope(Some(RecurringScope::Library));
        let inherited = document_occurrences(&[annotation], other_book, &book, 0, &text);
        assert_eq!(inherited.len(), 3, "a foreign creation address is never treated as a local original");
    }

    #[test]
    fn chapter_scope_stops_at_a_toc_boundary_inside_one_document() {
        let source = "<html><body><h1 id='first'>First</h1><p>Elizabeth</p><h1 id='second'>Second</h1><p>Elizabeth</p></body></html>";
        let parsed = parse_document(source, MarkupSyntax::Html).unwrap();
        let text = DocumentTextIndex::from_parsed(&parsed).unwrap();
        let book = book();
        let id = BookId::from_bytes([1; 32]);
        let matches = html::text::find_text_matches(text.text(), "Elizabeth", html::text::TextSearchOptions::default());
        let (first, end) = matches[0];
        let origin = AnnotationAnchor::epub_cfi(text_cfi_range(&text, 0, first, end).unwrap());
        let entries = vec![BookTocEntry { title: "First".into(), target: "a.html#first".into(), children: vec![] }, BookTocEntry { title: "Second".into(), target: "a.html#second".into(), children: vec![] }];
        let resolutions = BookToc::resolve(&entries, |href| Some((0, href.split_once('#').map(|(_, fragment)| fragment.to_owned()))));
        let mut toc = TocState::default();
        toc.set(entries, resolutions);
        let first_position = cfi::parse_source_position(origin.epub_cfi_value().unwrap()).unwrap();
        let second_cfi = text_cfi_range(&text, 0, matches[1].0, matches[1].1).unwrap();
        let second_position = cfi::parse_source_position(&second_cfi).unwrap();
        toc.install_source_positions([(0, vec![(first_position.path, 0), (second_position.path, 1)])].into(), 1);
        let chapter = recurring_annotation(id, origin.clone(), RecurringScope::Chapter);
        assert!(page_occurrences(std::iter::once(&chapter), id, &book, &toc, 0, text.text(), |start, end| text_cfi_range(&text, 0, start, end)).is_empty());
        let whole_book = recurring_annotation(id, origin, RecurringScope::Book);
        assert_eq!(document_occurrences(&[whole_book], id, &book, 0, &text).len(), 1);
    }

    #[test]
    fn ranges_cover_inline_markup_and_utf16_endpoints() {
        let parsed = parse_document("<html><body><p>Éva <em>😀</em></p></body></html>", MarkupSyntax::Html).unwrap();
        let text = DocumentTextIndex::from_parsed(&parsed).unwrap();
        let range = text_cfi_range(&text, 0, 0, text.character_count()).unwrap();
        let start: SourcePosition = cfi::parse_source_position(&range).unwrap();
        assert_eq!(start.utf16_offset, Some(0));
        assert!(range.ends_with(":2)"), "non-BMP endpoint uses two UTF-16 code units: {range}");
    }
}
