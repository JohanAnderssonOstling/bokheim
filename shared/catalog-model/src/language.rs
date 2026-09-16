use crate::{CatalogPublication, CatalogSource};

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

    pub fn publication_supported(&self, publication: &CatalogPublication) -> bool {
        if self.supported.is_empty() {
            return true;
        }
        publication.languages.iter().filter_map(|language| canonical_language(language.as_str())).any(|language| self.supports(&language))
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
    use crate::{CatalogAuthentication, CatalogPublicationIdentity, CatalogSourceId, LanguageTag};

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

    fn publication(languages: &[&str]) -> CatalogPublication {
        CatalogPublication {
            identity: CatalogPublicationIdentity { edition_id: "book".to_owned(), work_id: None },
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
    fn regional_tags_match_supported_publication_languages() {
        let filter = CatalogLanguageFilter::new(["sv-SE", "en"]);
        assert!(filter.publication_supported(&publication(&["sv"])));
        assert!(filter.publication_supported(&publication(&["en-GB"])));
        assert!(!filter.publication_supported(&publication(&["fr"])));
        assert!(!filter.publication_supported(&publication(&[])));
    }

    #[test]
    fn empty_preference_includes_every_source_and_publication_language() {
        let filter = CatalogLanguageFilter::new(std::iter::empty::<&str>());
        assert!(filter.publication_supported(&publication(&["fr"])));
        assert!(filter.publication_supported(&publication(&[])));
        assert!(filter.supported().is_empty());
    }
}
