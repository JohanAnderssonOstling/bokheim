//! Optional online summaries. A lookup makes one attempt and returns no
//! result on every failure; it never joins the dictionary refresh loop.
use std::{collections::HashSet, time::Duration};

use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::core::AppBackend;
use crate::Url;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikipediaArticle {
    pub title: String,
    pub extract: String,
    pub url: String,
    pub language: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikipediaChoice {
    pub title: String,
    pub description: String,
    pub language: String,
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

fn request_url(query: &str, languages: &[String]) -> Option<(Url, String)> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 160 || query.chars().any(char::is_control) {
        return None;
    }
    let language = languages.iter().find_map(|tag| dictionary_catalog::language(tag)).unwrap_or_else(|| "en".into());
    let language = if language == "nb" { "no".into() } else { language };
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

impl AppBackend {
    pub(crate) async fn lookup_wikipedia(&self, query: &str, languages: &[String]) -> Option<WikipediaLookup> {
        let (url, language) = request_url(query, languages)?;
        // Initialize on the first valid lookup and share the connection pool for
        // the backend's lifetime, including requests for alternative articles.
        let client = self.state.wikipedia_client.get_or_init(|| async { client() }).await.as_ref()?;
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
    }
}

async fn fetch(client: &Client, url: Url) -> Option<Vec<u8>> {
    let response = client.get(url).timeout(Duration::from_secs(8)).send().await.ok()?.error_for_status().ok()?;
    if response.content_length().is_some_and(|size| size > MAX_RESPONSE_BYTES as u64) {
        return None;
    }
    let mut stream = Box::pin(response.bytes_stream());
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

fn parse(bytes: &[u8], language: &str) -> Option<Summary> {
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
    Some(Summary::Article(WikipediaArticle { title: summary.title, extract: summary.extract, url: url.into(), language: language.into() }))
}

fn parse_choices(bytes: &[u8], language: &str, ambiguous_title: &str) -> Option<Vec<WikipediaChoice>> {
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
            Some(WikipediaChoice { title: page.title, description: page.description.unwrap_or_default(), language: language.into(), url: url.into() })
        })
        .collect();
    (!choices.is_empty()).then_some(choices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_titles_and_uses_one_book_language_without_network_fallbacks() {
        let (url, language) = request_url(" Haus / Tür? ", &["de_DE.UTF-8".into(), "en".into()]).unwrap();
        assert_eq!(language, "de");
        assert_eq!(url.host_str(), Some("de.wikipedia.org"));
        assert!(url.path().ends_with("Haus_%2F_T%C3%BCr%3F"));
        assert!(url.query().is_none());
        assert_eq!(request_url("Earth", &[]).unwrap().1, "en");
        assert_eq!(request_url("Jorden", &["nb-NO".into()]).unwrap().1, "no");
        for query in [" ", "Earth\nMoon", &"x".repeat(161)] {
            assert!(request_url(query, &[]).is_none());
        }
    }

    #[test]
    fn summaries_are_plain_text_and_link_only_to_the_requested_wikipedia() {
        let Some(Summary::Article(article)) = parse(br#"{"type":"standard","title":"Albert Einstein","extract":"A physicist.","content_urls":{"desktop":{"page":"https://example.com/unsafe"}}}"#, "de") else { panic!("expected an article") };
        assert_eq!(article.title, "Albert Einstein");
        assert_eq!(article.extract, "A physicist.");
        assert_eq!(article.url, "https://de.wikipedia.org/wiki/Albert_Einstein");
        for json in [br#"{"type":"standard","title":"Earth","extract":" "}"#.as_slice(), b"{", br#"{"error":"not found"}"#] {
            assert!(parse(json, "en").is_none());
        }
    }

    #[test]
    fn ambiguity_returns_choices_without_picking_an_article() {
        assert!(matches!(parse(br#"{"type":"disambiguation","title":"Mercury","extract":"May refer to..."}"#, "en"), Some(Summary::Ambiguous(title)) if title == "Mercury"));
        let choices =
            parse_choices(br#"{"pages":[{"title":"Mercury","description":"Ambiguous"},{"title":"Mercury (planet)","description":"Planet"},{"title":"Mercury (planet)"},{"title":"Mercury (element)","description":null}]}"#, "en", "Mercury")
                .unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].title, "Mercury (planet)");
        assert_eq!(choices[0].description, "Planet");
        assert_eq!(choices[1].description, "");
        assert!(parse_choices(br#"{"pages":[]}"#, "en", "Mercury").is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn successive_requests_reuse_a_connection() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/summary", listener.local_addr().unwrap())).unwrap();
        let client = client().unwrap();
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            // Both requests must arrive on this socket. A second connection
            // would leave this server waiting until the test times out.
            for _ in 0..2 {
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    assert!(request.len() < 4096);
                    request.push(socket.read_u8().await.unwrap());
                }
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").await.unwrap();
            }
        };
        let requests = async {
            for _ in 0..2 {
                assert_eq!(fetch(&client, url.clone()).await.as_deref(), Some(b"{}".as_slice()));
            }
        };
        tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(requests, server) }).await.expect("requests should share one connection");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn failed_http_requests_finish_quietly_without_retrying() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        for status in ["404 Not Found", "429 Too Many Requests", "503 Service Unavailable"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = Url::parse(&format!("http://{}/summary", listener.local_addr().unwrap())).unwrap();
            let server = async {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                socket.read(&mut request).await.unwrap();
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\nRetry-After: 0\r\n\r\n").as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
                assert!(tokio::time::timeout(Duration::from_millis(100), listener.accept()).await.is_err(), "Wikipedia failures must not schedule another attempt");
            };
            let client = client().unwrap();
            let (result, ()) = tokio::join!(fetch(&client, url), server);
            assert!(result.is_none());
        }
    }
}
