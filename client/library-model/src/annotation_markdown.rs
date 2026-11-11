//! The same quote, note and source block for the clipboard and file exports.

use crate::BookLocator;
use book_model::{AnnotationAnchor, EpubCfi};
use std::fmt::Write;
use url::Url;

/// A library-scoped reader destination. The saved location also works after
/// the annotation has been deleted or before it has synchronized.
#[derive(Clone, Debug, PartialEq)]
pub struct ReaderLink {
    pub locator: BookLocator,
    pub annotation_id: Option<String>,
    pub anchor: AnnotationAnchor,
}

impl ReaderLink {
    pub fn to_url(&self) -> String {
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        pairs.append_pair("library", &self.locator.library_id().to_string());
        pairs.append_pair("book", &self.locator.book_id().to_string());
        if let Some(id) = &self.annotation_id {
            pairs.append_pair("annotation", id);
        }
        // These anchors contain only strings, integers and validated geometry.
        if let Some(location) = self.anchor.open_target() {
            pairs.append_pair("location", &location);
        }
        format!("bokheim://open?{}", pairs.finish())
    }

    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if value.len() > 128 * 1024 {
            return Err("Reader link is too long.");
        }
        let url = Url::parse(value).map_err(|_| "Invalid reader link.")?;
        if url.scheme() != "bokheim" || url.host_str() != Some("open") || !matches!(url.path(), "" | "/") || !url.username().is_empty() || url.password().is_some() || url.port().is_some() || url.fragment().is_some() {
            return Err("Unsupported reader link.");
        }
        let mut library = None;
        let mut book = None;
        let mut annotation_id = None;
        let mut anchor = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "library" if library.is_none() => library = Some(value.parse().map_err(|_| "Invalid library identity.")?),
                "book" if book.is_none() => book = Some(value.parse().map_err(|_| "Invalid book identity.")?),
                "annotation" if annotation_id.is_none() && !value.is_empty() => annotation_id = Some(value.into_owned()),
                "location" if anchor.is_none() => anchor = Some(AnnotationAnchor::from_open_target(&value).ok_or("Invalid passage location.")?),
                _ => return Err("Unsupported or repeated reader link parameter."),
            }
        }
        let anchor = anchor.ok_or("Reader link has no passage location.")?;
        match &anchor {
            AnnotationAnchor::EpubCfi { .. } if anchor.epub_cfi_value().is_none_or(|cfi| EpubCfi::parse(cfi).is_err()) => return Err("Reader link has an invalid passage location."),
            AnnotationAnchor::Pdf(pdf) if pdf.rects.iter().any(|rect| [rect.left, rect.top, rect.width, rect.height].iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))) => {
                return Err("Reader link has invalid page coordinates.");
            }
            _ => {}
        }
        Ok(Self { locator: BookLocator::new(library.ok_or("Reader link has no library.")?, book.ok_or("Reader link has no book.")?), annotation_id, anchor })
    }
}

pub fn annotation_source_label(title: &str, section: Option<&str>, anchor: &AnnotationAnchor) -> String {
    let mut label = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(section) = section.filter(|section| !section.trim().is_empty()) {
        label.push_str(": ");
        label.push_str(&section.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    if let Some(pdf) = anchor.pdf_anchor() {
        label.push_str(&format!(", p. {}", u64::from(pdf.page_index) + 1));
    }
    label
}

/// The passage is literal text in an italic blockquote. Notes remain authored
/// Markdown outside the quote; the source is the only generated link.
pub fn annotation_markdown(quote: &str, note: &str, source_label: &str, source: &ReaderLink) -> String {
    let mut out = String::new();
    if !quote.is_empty() {
        for line in quote.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
            if line.trim().is_empty() {
                out.push_str(">\n");
            } else {
                writeln!(out, "> *{}*", escape(line.trim())).ok();
            }
        }
        out.push('\n');
    }
    if !note.trim().is_empty() {
        out.push_str(note.trim_end());
        out.push_str("\n\n");
    }
    writeln!(out, "[{}]({})\n", escape(source_label), source.to_url()).ok();
    out
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if "\\`*_{}[]<>!|~&".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{PdfAnnotationAnchor, PdfAnnotationRect};
    use content_address::BookId;
    use uuid::Uuid;

    fn source(anchor: AnnotationAnchor) -> ReaderLink {
        ReaderLink { locator: BookLocator::new(Uuid::nil(), BookId::from_bytes([7; 32])), annotation_id: Some("note & ? # ü".into()), anchor }
    }

    #[test]
    fn reader_links_round_trip_epub_pdf_and_selection_destinations() {
        for anchor in [AnnotationAnchor::epub_publication_cfi("epubcfi(/8/2[chapter]!/4/2,/1:0,/1:7)"), AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(4, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.4)], None))] {
            let mut link = source(anchor);
            assert_eq!(ReaderLink::parse(&link.to_url()), Ok(link.clone()));
            link.annotation_id = None;
            assert_eq!(ReaderLink::parse(&link.to_url()), Ok(link));
        }
    }

    #[test]
    fn reader_links_reject_ambiguous_and_invalid_destinations() {
        let url = source(AnnotationAnchor::epub_cfi("epubcfi(/6/2)")).to_url();
        for invalid in [url.replace("bokheim:", "https:"), format!("{url}&book={}", BookId::from_bytes([8; 32])), format!("{url}#fragment"), "bokheim://open?location=wrong".into(), url.replace("//open?", "//other?")] {
            assert!(ReaderLink::parse(&invalid).is_err(), "{invalid}");
        }
        assert!(ReaderLink::parse(&source(AnnotationAnchor::epub_cfi("not a CFI")).to_url()).is_err());
        assert!(ReaderLink::parse(&source(AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(0, vec![PdfAnnotationRect::new(-0.1, 0.0, 0.2, 0.2)], None))).to_url()).is_err());
    }

    #[test]
    fn literal_multiline_quotes_do_not_turn_into_links_or_formatting() {
        let link = source(AnnotationAnchor::epub_cfi("epubcfi(/6/2)"));
        let markdown = annotation_markdown("A *literal* [link](url)\r\n\r\n> next", "My note.\n\nSecond paragraph.", "Book [title]", &link);
        assert!(markdown.starts_with("> *A \\*literal\\* \\[link\\](url)*\n>\n> *\\> next*\n\nMy note.\n\nSecond paragraph.\n\n[Book \\[title\\]](bokheim://open?"));
        assert_eq!(markdown.matches("bokheim://").count(), 1);
        let selection = annotation_markdown("Selected text", "", "Book", &link);
        assert!(selection.starts_with("> *Selected text*\n\n[Book]"));
        assert!(annotation_markdown("", "Note only", "Book", &link).starts_with("Note only\n\n[Book]"));
    }
}
