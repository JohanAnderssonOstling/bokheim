use crate::{CatalogBook, CatalogSource};

/// Explicit client language preferences used to constrain catalogue-wide
/// search and multilingual feeds. An empty list means all languages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogLanguageFilter {
    supported: Vec<String>,
}

impl CatalogLanguageFilter {
    pub fn new(languages: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let mut supported = languages.into_iter().filter_map(|language| canonical_language(language.as_ref())).filter(|language| language != "mul" && language != "und").collect::<Vec<_>>();
        supported.sort();
        supported.dedup();
        Self { supported }
    }

    pub fn supported(&self) -> &[String] {
        &self.supported
    }

    pub fn source_is_multilingual(&self, source: &CatalogSource) -> bool {
        let languages = source.languages.iter().filter_map(|language| canonical_language(language)).collect::<Vec<_>>();
        languages.is_empty() || languages.iter().any(|language| language == "mul" || language == "und") || languages.iter().take(2).count() > 1
    }

    pub fn book_supported(&self, book: &CatalogBook) -> bool {
        if self.supported.is_empty() {
            return true;
        }
        book.languages.iter().filter_map(|language| canonical_language(language.as_str())).any(|language| self.supports(&language))
    }

    fn supports(&self, candidate: &str) -> bool {
        self.supported.iter().any(|supported| languages_overlap(supported, candidate))
    }
}

fn languages_overlap(left: &str, right: &str) -> bool {
    left == right || left.strip_prefix(right).is_some_and(|suffix| suffix.starts_with('-')) || right.strip_prefix(left).is_some_and(|suffix| suffix.starts_with('-'))
}

fn canonical_language(value: &str) -> Option<String> {
    let value = value.trim().replace('_', "-").to_ascii_lowercase();
    let named = match value.as_str() {
        "multilingual" | "multiple languages" => "mul",
        "unspecified" | "unknown" => "und",
        "english" => "en",
        "spanish" => "es",
        "french" => "fr",
        "bulgarian" => "bg",
        "polish" => "pl",
        "norwegian" => "no",
        "belarusian" => "be",
        "hungarian" => "hu",
        "swedish" => "sv",
        "italian" => "it",
        "japanese" => "ja",
        "latin" => "la",
        "sámi" | "sami" | "sámi languages" | "sami languages" => "smi",
        _ => value.as_str(),
    };
    let valid = !named.is_empty()
        && named.len() <= 63
        && named.split('-').all(|part| !part.is_empty() && part.len() <= 8 && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        && named.split('-').next().is_some_and(|part| ((2..=8).contains(&part.len()) || part == "x") && part.bytes().all(|byte| byte.is_ascii_alphabetic()));
    valid.then(|| named.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CatalogAuthentication, CatalogBookIdentity, CatalogSourceId, LanguageTag};

    fn source(languages: &[&str]) -> CatalogSource {
        CatalogSource {
            id: CatalogSourceId::parse("opds-test").unwrap(),
            title: "Test".to_owned(),
            description: String::new(),
            languages: languages.iter().map(|language| (*language).to_owned()).collect(),
            authentication: CatalogAuthentication::None,
            capabilities: Vec::new(),
        }
    }

    fn book(languages: &[&str]) -> CatalogBook {
        CatalogBook {
            identity: CatalogBookIdentity { edition_id: "book".to_owned(), work_id: None },
            title: "Book".to_owned(),
            authors: Vec::new(),
            languages: languages.iter().map(|language| LanguageTag::parse(language).unwrap()).collect(),
            description: None,
            image_url: None,
            metadata: Default::default(),
            actions: Vec::new(),
        }
    }

    #[test]
    fn source_language_metadata_identifies_multilingual_catalogues() {
        let filter = CatalogLanguageFilter::new(["sv-SE", "en"]);
        assert!(!filter.source_is_multilingual(&source(&["Swedish"])));
        assert!(filter.source_is_multilingual(&source(&["English", "Swedish"])));
        assert!(filter.source_is_multilingual(&source(&["Multilingual"])));
        assert!(filter.source_is_multilingual(&source(&[])));
    }

    #[test]
    fn regional_tags_match_supported_book_languages() {
        let filter = CatalogLanguageFilter::new(["sv-SE", "en"]);
        assert!(filter.book_supported(&book(&["sv"])));
        assert!(filter.book_supported(&book(&["en-GB"])));
        assert!(!filter.book_supported(&book(&["fr"])));
        assert!(!filter.book_supported(&book(&[])));
    }

    #[test]
    fn empty_preference_includes_every_source_and_book_language() {
        let filter = CatalogLanguageFilter::new(std::iter::empty::<&str>());
        assert!(filter.book_supported(&book(&["fr"])));
        assert!(filter.book_supported(&book(&[])));
        assert!(filter.supported().is_empty());
    }
}
