//! Optional online summaries. A lookup makes one attempt and returns no
//! result on every failure; it never joins the dictionary refresh loop.
use std::{collections::HashSet, time::Duration};

use language_tags::LanguageTag;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::Services;
use url::Url;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikipediaArticle {
    pub title: String,
    pub extract: String,
    pub url: String,
    pub language: LanguageTag,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikipediaChoice {
    pub title: String,
    pub description: String,
    pub language: LanguageTag,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WikipediaLookup {
    Article(WikipediaArticle),
    Choices(Vec<WikipediaChoice>),
}

enum Summary {
    Article(WikipediaArticle),
    Ambiguous(String),
}

fn request_url(query: &str, languages: &[LanguageTag]) -> Option<(Url, LanguageTag)> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 160 || query.chars().any(char::is_control) {
        return None;
    }
    let language = languages.iter().filter(|tag| tag.is_valid()).map(LanguageTag::primary_language).find(|language| matches!(language.len(), 2 | 3) && !matches!(*language, "und" | "mul" | "zxx")).unwrap_or("en");
    let language = LanguageTag::parse(if language == "nb" { "no" } else { language }).ok()?;
    let mut url = Url::parse(&format!("https://{language}.wikipedia.org/api/rest_v1/page/summary/")).ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(&query.replace(' ', "_"));
    Some((url, language))
}

fn client() -> Option<Client> {
    let builder = http_response::client_builder();
    #[cfg(not(target_arch = "wasm32"))]
    let builder = builder.retry(reqwest::retry::never()).user_agent(concat!("Bokheim/", env!("CARGO_PKG_VERSION"), " (https://bokheim.se)"));
    builder.build().ok()
}

impl Services {
    pub(crate) async fn lookup_wikipedia(&self, query: &str, languages: &[LanguageTag]) -> Option<WikipediaLookup> {
        let (url, language) = request_url(query, languages)?;
        http_response::run(async {
            // Initialize on the first valid lookup and share the connection pool for
            // the lookup client's lifetime, including requests for alternative articles.
            let client = self.wikipedia_client.get_or_init(|| async { client() }).await.as_ref()?;
            let bytes = fetch(client, url).await?;
            match parse(&bytes, &language)? {
                Summary::Article(article) => Some(WikipediaLookup::Article(article)),
                Summary::Ambiguous(title) => {
                    // Ambiguity is a successful response, not a failed request to
                    // retry. Fetch choices once; the reader chooses the next article.
                    let mut url = Url::parse(&format!("https://{language}.wikipedia.org/w/rest.php/v1/search/page")).ok()?;
                    url.query_pairs_mut().append_pair("q", &title).append_pair("limit", "11");
                    let choices = parse_choices(&fetch(client, url).await?, &language, &title)?;
                    Some(WikipediaLookup::Choices(choices))
                }
            }
        })
        .await
        .ok()
        .flatten()
    }
}

async fn fetch(client: &Client, url: Url) -> Option<Vec<u8>> {
    let response = client.get(url).timeout(Duration::from_secs(8)).send().await.ok()?.error_for_status().ok()?;
    http_response::read(response, MAX_RESPONSE_BYTES).await.ok()
}

fn parse(bytes: &[u8], language: &LanguageTag) -> Option<Summary> {
    #[derive(Deserialize)]
    struct SummaryResponse {
        #[serde(rename = "type")]
        kind: String,
        title: String,
        extract: String,
    }
    let summary: SummaryResponse = serde_json::from_slice(bytes).ok()?;
    if summary.title.trim().is_empty() {
        return None;
    }
    if summary.kind == "disambiguation" {
        return Some(Summary::Ambiguous(summary.title));
    }
    if summary.kind != "standard" || summary.extract.trim().is_empty() {
        return None;
    }
    let mut url = Url::parse(&format!("https://{language}.wikipedia.org/wiki/")).ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(&summary.title.replace(' ', "_"));
    Some(Summary::Article(WikipediaArticle { title: summary.title, extract: summary.extract, url: url.into(), language: language.clone() }))
}

fn parse_choices(bytes: &[u8], language: &LanguageTag, ambiguous_title: &str) -> Option<Vec<WikipediaChoice>> {
    #[derive(Deserialize)]
    struct Page {
        title: String,
        description: Option<String>,
    }
    #[derive(Deserialize)]
    struct Search {
        pages: Vec<Page>,
    }
    let search: Search = serde_json::from_slice(bytes).ok()?;
    let mut seen = HashSet::new();
    let choices: Vec<_> = search
        .pages
        .into_iter()
        .filter(|page| page.title != ambiguous_title && !page.title.trim().is_empty() && page.title.chars().count() <= 160 && seen.insert(page.title.clone()))
        .take(10)
        .filter_map(|page| {
            let mut url = Url::parse(&format!("https://{language}.wikipedia.org/wiki/")).ok()?;
            url.path_segments_mut().ok()?.pop_if_empty().push(&page.title.replace(' ', "_"));
            Some(WikipediaChoice { title: page.title, description: page.description.unwrap_or_default(), language: language.clone(), url: url.into() })
        })
        .collect();
    (!choices.is_empty()).then_some(choices)
}

#[cfg(test)]
mod tests;
