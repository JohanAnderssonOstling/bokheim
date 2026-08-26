use catalog_contract::{CatalogPage, CatalogPublication, CatalogSource};

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

    pub fn system_default() -> Self {
        Self::new(sys_locale::get_locales())
    }

    pub fn supported(&self) -> &[String] {
        &self.supported
    }

    pub fn source_supported(&self, source: &CatalogSource) -> bool {
        if self.supported.is_empty() {
            return true;
        }
        let languages = source.languages.iter().filter_map(|language| canonical_language(language)).collect::<Vec<_>>();
        languages.is_empty() || languages.iter().any(|language| language == "mul" || language == "und") || languages.iter().any(|language| self.supports(language))
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

    pub fn filter_sources(&self, sources: impl IntoIterator<Item = CatalogSource>) -> Vec<CatalogSource> {
        sources.into_iter().filter(|source| self.source_supported(source)).collect()
    }

    pub fn filter_page(&self, source: &CatalogSource, page: &mut CatalogPage) {
        if !self.source_is_multilingual(source) {
            return;
        }
        page.publications.retain(|publication| self.publication_supported(publication));
        for group in &mut page.groups {
            group.publications.retain(|publication| self.publication_supported(publication));
        }
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
    use catalog_contract::{CatalogAuthentication, CatalogFreshness, CatalogPagination, CatalogPublicationIdentity, CatalogSourceId};

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
            languages: languages.iter().map(|language| catalog_contract::LanguageTag::parse(language).unwrap()).collect(),
            description: None,
            image_url: None,
            metadata: Default::default(),
            actions: Vec::new(),
        }
    }

    #[test]
    fn source_names_and_regional_tags_match_client_languages() {
        let filter = CatalogLanguageFilter::new(["sv-SE", "en"]);
        assert!(filter.source_supported(&source(&["Swedish"])));
        assert!(filter.source_supported(&source(&["en-GB"])));
        assert!(filter.source_supported(&source(&["Multilingual"])));
        assert!(!filter.source_supported(&source(&["French"])));
    }

    #[test]
    fn multilingual_pages_require_supported_publication_language() {
        let filter = CatalogLanguageFilter::new(["sv-SE", "en"]);
        let source = source(&["Multilingual"]);
        let mut page = CatalogPage {
            source_id: source.id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Books".to_owned(),
            locator: "https://example.com/opds".to_owned(),
            navigation: Vec::new(),
            publications: vec![publication(&["sv"]), publication(&["fr"]), publication(&[])],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        filter.filter_page(&source, &mut page);
        assert_eq!(page.publications.len(), 1);
        assert_eq!(page.publications[0].languages, ["sv"]);
    }

    #[test]
    fn single_language_sources_do_not_require_per_book_tags() {
        let filter = CatalogLanguageFilter::new(["en"]);
        let source = source(&["English"]);
        let mut page = CatalogPage {
            source_id: source.id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Books".to_owned(),
            locator: "https://example.com/opds".to_owned(),
            navigation: Vec::new(),
            publications: vec![publication(&[])],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        filter.filter_page(&source, &mut page);
        assert_eq!(page.publications.len(), 1);
    }

    #[test]
    fn sources_without_language_metadata_filter_each_publication() {
        let filter = CatalogLanguageFilter::new(["en", "sv"]);
        let source = source(&[]);
        let mut page = CatalogPage {
            source_id: source.id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Books".to_owned(),
            locator: "https://example.com/opds".to_owned(),
            navigation: Vec::new(),
            publications: vec![publication(&["en"]), publication(&["sv-SE"]), publication(&["fr"]), publication(&[])],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        filter.filter_page(&source, &mut page);
        assert_eq!(page.publications.len(), 2);
        assert_eq!(page.publications[0].languages, ["en"]);
        assert_eq!(page.publications[1].languages, ["sv-SE"]);
    }

    #[test]
    fn empty_preference_includes_every_source_and_publication_language() {
        let filter = CatalogLanguageFilter::new(std::iter::empty::<&str>());
        assert!(filter.source_supported(&source(&["French"])));
        assert!(filter.publication_supported(&publication(&["fr"])));
        assert!(filter.publication_supported(&publication(&[])));
        assert!(filter.supported().is_empty());
    }
}
