//! Translation and Wikipedia lookups independent of account and library state.
pub use operation_failure::OperationError as LookupError;
use language_tags::LanguageTag;
use reqwest::Client as HttpClient;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

pub mod translation;
mod wikipedia;
pub use wikipedia::{WikipediaArticle, WikipediaChoice, WikipediaLookup, can_lookup_wikipedia};

/// Parse book metadata or an operating-system locale into a BCP 47 tag.
pub fn parse_language_tag(input: &str) -> Option<LanguageTag> {
    // POSIX locale separators and encoding suffixes are normalized only here.
    let locale = input.trim().split(['.', '@']).next()?.replace('_', "-");
    let tag = LanguageTag::parse(&locale).ok()?.canonicalize().ok()?;
    tag.validate().ok()?;
    // Private-use and unspecified metadata tags cannot select a lookup language.
    if !matches!(tag.primary_language().len(), 2 | 3) || matches!(tag.primary_language(), "und" | "mul" | "zxx") {
        return None;
    }
    Some(tag)
}

/// Cloneable lookup client with shared HTTP connections and Wikipedia stylesheets.
#[derive(Clone, Default)]
pub struct Client {
    services: Arc<Services>,
}

#[derive(Default)]
struct Services {
    wikipedia_client: OnceCell<HttpClient>,
    wikipedia_stylesheets: Mutex<wikipedia::StylesheetCache>,
    translation_client: OnceCell<HttpClient>,
}

impl Client {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn translate_selection(&self, selection: String, source: LanguageTag, target: LanguageTag) -> Result<String, translation::TranslationError> {
        self.services.translate_selection(&selection, &source, &target).await
    }

    /// One attempt, distinguishing missing pages from unavailable service.
    pub async fn lookup_wikipedia(&self, query: String, languages: Vec<LanguageTag>) -> Option<WikipediaLookup> {
        self.services.lookup_wikipedia(&query, &languages).await
    }

    pub async fn wikipedia_html(&self, title: String, language: LanguageTag) -> Result<String, LookupError> {
        self.services.wikipedia_html(&title, &language).await
    }

    pub async fn wikipedia_image(&self, uri: String) -> Result<Vec<u8>, LookupError> {
        self.services.wikipedia_resource(&uri, 8 * 1024 * 1024).await
    }

    pub async fn wikipedia_stylesheet(&self, uri: String) -> Result<Arc<[u8]>, LookupError> {
        self.services.wikipedia_stylesheet(&uri).await
    }

    pub async fn wikipedia_equivalents(&self, article: WikipediaArticle, targets: Vec<LanguageTag>) -> Result<Vec<WikipediaChoice>, LookupError> {
        self.services.wikipedia_equivalents(&article, &targets).await
    }
}
