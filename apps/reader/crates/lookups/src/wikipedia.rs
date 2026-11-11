//! Optional online summaries with explicit service failures and no automatic retry.
use std::{
    collections::{HashSet, VecDeque},
    time::Duration,
    sync::Arc,
};

use language_tags::LanguageTag;
use reqwest::{
    Client,
    header::{AGE, CACHE_CONTROL, HeaderMap, VARY},
};
use serde::{Deserialize, Serialize};

use crate::{LookupError, Services};
use url::Url;
use web_time::Instant;

const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_STYLESHEET_BYTES: usize = 1024 * 1024;
const MAX_CACHED_STYLESHEETS: usize = 32;
const MAX_CACHED_STYLESHEET_BYTES: usize = 4 * 1024 * 1024;

struct Stylesheet {
    uri: String,
    expires: Instant,
    bytes: Arc<[u8]>,
}

#[derive(Default)]
pub(super) struct StylesheetCache(VecDeque<Stylesheet>);

impl StylesheetCache {
    fn get(&mut self, uri: &str) -> Option<Arc<[u8]>> {
        let now = Instant::now();
        self.0.retain(|entry| entry.expires > now);
        let index = self.0.iter().position(|entry| entry.uri == uri)?;
        let entry = self.0.remove(index)?;
        let bytes = entry.bytes.clone();
        self.0.push_back(entry);
        Some(bytes)
    }

    fn insert(&mut self, uri: String, expires: Instant, bytes: Arc<[u8]>) {
        let now = Instant::now();
        self.0.retain(|entry| entry.expires > now && entry.uri != uri);
        if expires <= now || bytes.len() > MAX_CACHED_STYLESHEET_BYTES {
            return;
        }
        while self.0.len() >= MAX_CACHED_STYLESHEETS || self.0.iter().map(|entry| entry.bytes.len()).sum::<usize>() + bytes.len() > MAX_CACHED_STYLESHEET_BYTES {
            self.0.pop_front();
        }
        self.0.push_back(Stylesheet { uri, expires, bytes });
    }
}

fn stylesheet_lifetime(headers: &HeaderMap) -> Option<Duration> {
    let mut max_age = None;
    for value in headers.get_all(CACHE_CONTROL) {
        for directive in value.to_str().ok()?.split(',') {
            let directive = directive.trim();
            let (name, value) = directive.split_once('=').unwrap_or((directive, ""));
            if name.trim().eq_ignore_ascii_case("no-store") || name.trim().eq_ignore_ascii_case("no-cache") {
                return None;
            }
            if name.trim().eq_ignore_ascii_case("max-age") {
                if max_age.is_some() {
                    return None;
                }
                max_age = Some(value.trim().trim_matches('"').parse::<u64>().ok()?);
            }
        }
    }
    for value in headers.get_all(VARY) {
        if value.to_str().ok()?.split(',').any(|field| field.trim() == "*") {
            return None;
        }
    }
    let age = match headers.get(AGE) {
        Some(value) => value.to_str().ok()?.parse::<u64>().ok()?,
        None => 0,
    };
    Duration::from_secs(max_age?).checked_sub(Duration::from_secs(age))
}

fn resource_url(uri: &str) -> Option<Url> {
    let mut url = Url::parse(uri).ok()?;
    let host = url.host_str()?;
    if url.scheme() != "https" || !(host == "wikipedia.org" || host.ends_with(".wikipedia.org") || host == "wikimedia.org" || host.ends_with(".wikimedia.org")) {
        return None;
    }
    url.set_fragment(None);
    Some(url)
}

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
    Unavailable(LookupError),
}

enum Summary {
    Article(WikipediaArticle),
    Ambiguous(String),
}

fn wikipedia_language(tag: &LanguageTag) -> Option<&str> {
    match tag.primary_language() {
        "nb" => Some("no"),
        "und" | "mul" | "zxx" => None,
        language if matches!(language.len(), 2 | 3) => Some(language),
        _ => None,
    }
}

fn article_url(title: &str, language: &LanguageTag) -> Option<Url> {
    if title.trim().is_empty() || title.chars().count() > 255 || title.chars().any(char::is_control) {
        return None;
    }
    let language = wikipedia_language(language)?;
    let mut url = Url::parse(&format!("https://{language}.wikipedia.org/w/rest.php/v1/page/")).ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(&title.replace(' ', "_"));
    Some(url)
}

pub fn can_lookup_wikipedia(query: &str) -> bool {
    let query = query.trim();
    !query.is_empty() && query.chars().take(161).count() <= 160 && !query.chars().any(char::is_control)
}

fn request_url(query: &str, languages: &[LanguageTag]) -> Option<(Url, LanguageTag)> {
    if !can_lookup_wikipedia(query) {
        return None;
    }
    let query = query.trim();
    let language = languages.iter().find_map(wikipedia_language).unwrap_or("en");
    let mut url = Url::parse(&format!("https://{language}.wikipedia.org/api/rest_v1/page/summary/")).ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(&query.replace(' ', "_"));
    Some((url, LanguageTag::parse(language).ok()?))
}

fn client() -> Result<Client, LookupError> {
    let builder = http_response::client_builder();
    #[cfg(not(target_arch = "wasm32"))]
    let builder = builder.retry(reqwest::retry::never()).user_agent(concat!("Bokheim/", env!("CARGO_PKG_VERSION"), " (https://bokheim.se)"));
    builder.build().map_err(LookupError::from)
}

impl Services {
    pub(crate) async fn wikipedia_equivalents(&self, article: &WikipediaArticle, targets: &[LanguageTag]) -> Result<Vec<WikipediaChoice>, LookupError> {
        #[derive(Deserialize)]
        struct PageLanguage {
            code: String,
            title: String,
        }
        let language = wikipedia_language(&article.language).ok_or("Unsupported Wikipedia language")?;
        let mut url = article_url(&article.title, &article.language).ok_or("Invalid Wikipedia article URL")?;
        url.path_segments_mut().map_err(|_| "Invalid Wikipedia article URL")?.extend(["links", "language"]);
        http_response::run(async {
            let client = self.wikipedia_client.get_or_try_init(|| async { client() }).await.map_err(LookupError::operation)?;
            let response = client.get(url).timeout(Duration::from_secs(8)).send().await.map_err(LookupError::from)?.error_for_status().map_err(LookupError::from)?;
            let bytes = http_response::read(response, 256 * 1024).await.map_err(LookupError::operation)?;
            let links: Vec<PageLanguage> = serde_json::from_slice(&bytes).map_err(LookupError::operation)?;
            Ok(
                targets
                    .iter()
                    .filter_map(|target| {
                        let wiki_language = wikipedia_language(target)?;
                        if wiki_language == language {
                            return Some(WikipediaChoice { title: article.title.clone(), description: String::new(), language: target.clone(), url: article.url.clone() });
                        }
                        let link = links.iter().find(|link| link.code == wiki_language)?;
                        let mut url = Url::parse(&format!("https://{wiki_language}.wikipedia.org/wiki/")).ok()?;
                        url.path_segments_mut().ok()?.pop_if_empty().push(&link.title.replace(' ', "_"));
                        Some(WikipediaChoice { title: link.title.clone(), description: String::new(), language: target.clone(), url: url.into() })
                    })
                    .collect(),
            )
        })
        .await
        .map_err(LookupError::operation)?
    }

    pub(crate) async fn wikipedia_resource(&self, uri: &str, max_bytes: usize) -> Result<Vec<u8>, LookupError> {
        let url = resource_url(uri).ok_or("Invalid Wikipedia resource URL")?;
        http_response::run(async {
            let client = self.wikipedia_client.get_or_try_init(|| async { client() }).await.map_err(LookupError::operation)?;
            let response = client.get(url).timeout(Duration::from_secs(15)).send().await.map_err(LookupError::from)?.error_for_status().map_err(LookupError::from)?;
            http_response::read(response, max_bytes).await.map_err(LookupError::operation)
        })
        .await
        .map_err(LookupError::operation)?
    }

    pub(crate) async fn wikipedia_stylesheet(&self, uri: &str) -> Result<Arc<[u8]>, LookupError> {
        let url = resource_url(uri).ok_or("Invalid Wikipedia resource URL")?;
        let cached = self.wikipedia_stylesheets.lock().ok().and_then(|mut cache| cache.get(url.as_str()));
        if let Some(cached) = cached {
            return Ok(cached);
        }
        http_response::run(async {
            let client = self.wikipedia_client.get_or_try_init(|| async { client() }).await.map_err(LookupError::operation)?;
            let started = Instant::now();
            let response = client.get(url.clone()).timeout(Duration::from_secs(15)).send().await.map_err(LookupError::from)?.error_for_status().map_err(LookupError::from)?;
            // Count request/body time against freshness; cache only complete successes.
            let expires = stylesheet_lifetime(response.headers()).and_then(|lifetime| started.checked_add(lifetime));
            let bytes: Arc<[u8]> = http_response::read(response, MAX_STYLESHEET_BYTES).await.map_err(LookupError::operation)?.into();
            if let Some(expires) = expires {
                if let Ok(mut cache) = self.wikipedia_stylesheets.lock() {
                    cache.insert(url.into(), expires, bytes.clone());
                }
            }
            Ok(bytes)
        })
        .await
        .map_err(LookupError::operation)?
    }

    pub(crate) async fn wikipedia_html(&self, title: &str, language: &LanguageTag) -> Result<String, LookupError> {
        let mut url = article_url(title, language).ok_or("Invalid Wikipedia article URL")?;
        url.path_segments_mut().map_err(|_| "Invalid Wikipedia article URL")?.push("html");
        http_response::run(async {
            let client = self.wikipedia_client.get_or_try_init(|| async { client() }).await.map_err(LookupError::operation)?;
            let response = client.get(url).timeout(Duration::from_secs(15)).send().await.map_err(LookupError::from)?.error_for_status().map_err(LookupError::from)?;
            let bytes = http_response::read(response, 4 * 1024 * 1024).await.map_err(LookupError::operation)?;
            String::from_utf8(bytes).map_err(LookupError::operation).and_then(|html| if html.trim().is_empty() { Err("Wikipedia article is empty".into()) } else { Ok(html) })
        })
        .await
        .map_err(LookupError::operation)?
    }

    pub(crate) async fn lookup_wikipedia(&self, query: &str, languages: &[LanguageTag]) -> Option<WikipediaLookup> {
        let (url, language) = request_url(query, languages)?;
        let result = http_response::run(async {
            // Share successful initialization; a later retry can recover from failure.
            let client = self.wikipedia_client.get_or_try_init(|| async { client() }).await?;
            let Some(bytes) = fetch(client, url).await? else { return Ok::<_, LookupError>(None) };
            let Some(summary) = parse(&bytes, &language) else { return Ok(None) };
            match summary {
                Summary::Article(article) => Ok(Some(WikipediaLookup::Article(article))),
                Summary::Ambiguous(title) => {
                    // Full-text search includes incidental mentions throughout articles.
                    // A word lookup needs title suggestions, including matching redirects.
                    let mut url = Url::parse(&format!("https://{language}.wikipedia.org/w/rest.php/v1/search/title")).map_err(LookupError::operation)?;
                    url.query_pairs_mut().append_pair("q", query.trim()).append_pair("limit", "20");
                    let Some(bytes) = fetch(client, url).await? else { return Ok(None) };
                    Ok(parse_choices(&bytes, &language, &title).map(WikipediaLookup::Choices))
                }
            }
        })
        .await;
        match result {
            Ok(Ok(lookup)) => lookup,
            Ok(Err(error)) => Some(WikipediaLookup::Unavailable(error)),
            Err(error) => Some(WikipediaLookup::Unavailable(LookupError::operation(error))),
        }
    }
}

async fn fetch(client: &Client, url: Url) -> Result<Option<Vec<u8>>, LookupError> {
    let response = client.get(url).timeout(Duration::from_secs(8)).send().await.map_err(LookupError::operation)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status().map_err(LookupError::operation)?;
    http_response::read(response, MAX_RESPONSE_BYTES).await.map(Some).map_err(LookupError::operation)
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
