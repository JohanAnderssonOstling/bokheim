//! Translation and Wikipedia lookups independent of account and library state.
use language_tags::LanguageTag;
use reqwest::Client as HttpClient;
use std::sync::Arc;
use tokio::sync::OnceCell;

pub mod translation;
mod wikipedia;
pub use wikipedia::{WikipediaArticle, WikipediaChoice, WikipediaLookup};

/// Parse book metadata or an operating-system locale into a BCP 47 tag.
pub fn parse_language_tag(input: &str) -> Option<LanguageTag> {
    // POSIX locale separators and encoding suffixes are normalized only here.
    let locale = input.trim().split(['.', '@']).next()?.replace('_', "-");
    let mut tag = LanguageTag::parse(&locale).ok()?.canonicalize().ok()?;
    // Keep the catalog's ISO language aliases while delegating tag grammar,
    // casing, and canonicalization to language-tags.
    let language = dictionary_catalog::language(tag.primary_language())?;
    if language != tag.primary_language() {
        tag = LanguageTag::parse(&tag.as_str().replacen(tag.primary_language(), &language, 1)).ok()?;
    }
    let tag = tag.canonicalize().ok()?;
    tag.validate().ok()?;
    Some(tag)
}

/// Cloneable lookup client with shared HTTP connections.
#[derive(Clone, Default)]
pub struct Client {
    services: Arc<Services>,
}

#[derive(Default)]
struct Services {
    wikipedia_client: OnceCell<Option<HttpClient>>,
    translation_client: OnceCell<Option<HttpClient>>,
}

impl Client {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn translate_selection(&self, selection: String, source: LanguageTag, target: LanguageTag) -> Result<String, String> {
        self.services.translate_selection(&selection, &source, &target).await
    }

    /// Offline, missing pages, and request failures finish quietly without retrying.
    pub async fn lookup_wikipedia(&self, query: String, languages: Vec<LanguageTag>) -> Option<WikipediaLookup> {
        self.services.lookup_wikipedia(&query, &languages).await
    }
}
