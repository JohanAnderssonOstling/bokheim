//! Stable library-scoped reader destinations and their URL representation.

use crate::BookLocator;
use book_model::{AnnotationAnchor, EpubCfi};
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
