//! Title normalization shared by inspection and library ingestion.
use crate::{BookMetadataState, PublicationMetadata, PublicationTitle};

const COLON_SOURCE: &str = "title:colon-fallback";

impl PublicationMetadata {
    /// The preferred subtitle, stored independently of the primary book title.
    pub fn subtitle(&self) -> Option<&str> {
        self.alternate_titles.iter()
            .filter(|title| title.title_type().is_some_and(|kind| kind.eq_ignore_ascii_case("subtitle")))
            .min_by_key(|title| title.source() == COLON_SOURCE)
            .map(|title| title.value().trim()).filter(|value| !value.is_empty())
    }

    pub fn set_subtitle(&mut self, subtitle: &str, source: &str) -> Result<(), crate::ValueError> {
        let entry = PublicationTitle::new(None, subtitle, Some("subtitle".into()), None, source)?;
        self.alternate_titles.retain(|title| !title.title_type().is_some_and(|kind| kind.eq_ignore_ascii_case("subtitle")));
        self.alternate_titles.push(entry);
        Ok(())
    }

    /// Explicit metadata wins. Otherwise split once, retaining further colons in
    /// the subtitle. Keep the original combined title as source evidence.
    pub fn normalize_title(&mut self, title: &mut String) {
        let original = title.trim();
        let explicit = self.alternate_titles.iter().find(|entry| entry.source() != COLON_SOURCE && entry.title_type().is_some_and(|kind| kind.eq_ignore_ascii_case("subtitle"))).map(|entry| entry.value().trim().to_owned());
        let Some((main, tail)) = original.split_once(':') else { return };
        let (main, tail) = (main.trim(), tail.trim());
        if main.is_empty() || tail.is_empty() { return; }
        if let Some(subtitle) = explicit {
            // An explicitly separate subtitle must not cause a colon inside the
            // main title to be misinterpreted. Strip only a matching suffix.
            if let Some(prefix) = original.strip_suffix(&subtitle).and_then(|prefix| prefix.trim_end().strip_suffix(':')) {
                if !prefix.trim().is_empty() { *title = prefix.trim().to_owned(); }
            }
            return;
        }
        let Ok(subtitle) = PublicationTitle::new(None, tail, Some("subtitle".into()), None, COLON_SOURCE) else { return };
        let Ok(expanded) = PublicationTitle::new(None, original, Some("expanded".into()), None, COLON_SOURCE) else { return };
        let main = main.to_owned();
        if !self.alternate_titles.contains(&expanded) { self.alternate_titles.push(expanded); }
        self.alternate_titles.retain(|entry| !(entry.source() == COLON_SOURCE && entry.title_type() == Some("subtitle")));
        self.alternate_titles.push(subtitle);
        *title = main;
    }
}

impl BookMetadataState {
    pub fn subtitle(&self) -> Option<&str> { self.publication.subtitle() }
    pub fn normalize_title(&mut self) { self.publication.normalize_title(&mut self.title); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn book(title: &str) -> BookMetadataState {
        BookMetadataState { title: title.into(), contributors: vec![], description: String::new(), publication: Default::default() }
    }
    #[test]
    fn colon_split_preserves_evidence_and_is_idempotent() {
        let mut book = book("  Why Machines Learn : The Math: An Introduction  ");
        book.normalize_title();
        assert_eq!(book.title, "Why Machines Learn");
        assert_eq!(book.subtitle(), Some("The Math: An Introduction"));
        assert!(book.publication.alternate_titles.iter().any(|title| title.value() == "Why Machines Learn : The Math: An Introduction"));
        let before = book.clone(); book.normalize_title(); assert_eq!(book, before);
    }
    #[test]
    fn explicit_subtitle_wins_and_main_title_colons_are_preserved() {
        let mut book = book("Star Trek: Voyager");
        book.publication.set_subtitle("A Guide", "epub:package:title").unwrap();
        book.normalize_title();
        assert_eq!(book.title, "Star Trek: Voyager");
        assert_eq!(book.subtitle(), Some("A Guide"));
        book.title = "Star Trek: Voyager: A Guide".into();
        book.normalize_title();
        assert_eq!(book.title, "Star Trek: Voyager");
    }
    #[test]
    fn empty_parts_and_titles_without_colons_remain_unchanged() {
        for title in ["Frankenstein", ": subtitle", "Title: ", ""] {
            let mut book = book(title); book.normalize_title();
            assert_eq!(book.title, title); assert_eq!(book.subtitle(), None);
        }
    }
}
