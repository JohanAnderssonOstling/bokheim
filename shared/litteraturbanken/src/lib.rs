//! Client and provider model for Litteraturbanken's public EPUB catalogue.

use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use serde::Deserialize;
use serde_json::json;
use std::fmt;
use std::time::Duration;

pub const CATALOG_URL: &str = "https://litteraturbanken.se/epub?visa=epub&sort=popularitet";
pub const SEARCH_TEMPLATE: &str = "?visa=epub&sort=popularitet&bokheim_query={searchTerms}";

const HOST: &str = "litteraturbanken.se";
const CATALOG_PATH: &str = "/epub";
const API_PATH: &str = "/api/query/etext";
const SEARCH_PARAMETER: &str = "bokheim_query";
const SORT_PARAMETER: &str = "bokheim_sort";
const LANGUAGE_PARAMETER: &str = "bokheim_language";
const GENDER_PARAMETER: &str = "bokheim_gender";
const MODERNIZED_PARAMETER: &str = "bokheim_modernized";
const MAX_RESULTS: &str = "10000";
const NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const INCLUDED_FIELDS: &str = "lbworkid,titlepath,title,titleid,work_titleid,shorttitle,mediatype,authors.authorid,authors.full_name,authors.name_for_index,authors.type,language";
const EXCLUDED_FIELDS: &str = "text,parts,sourcedesc,pages,errata,content_vector";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub title: String,
    pub source_url: Url,
    pub root_url: Url,
    pub total_items: u32,
    pub publications: Vec<Publication>,
    pub facets: Vec<Facet>,
    pub search_template: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facet {
    pub title: String,
    pub links: Vec<FacetLink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacetLink {
    pub title: String,
    pub url: Url,
    pub active: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct QueryOptions {
    search: Option<String>,
    sort: Option<String>,
    language: Option<String>,
    gender: Option<String>,
    modernized: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publication {
    pub id: String,
    pub title: String,
    pub authors: Vec<Author>,
    pub language: Option<String>,
    pub download_url: Url,
    pub file_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub sort_name: Option<String>,
    pub provider_url: Option<Url>,
}

#[derive(Debug)]
pub enum Error {
    InvalidUrl(String),
    Network(reqwest::Error),
    TimedOut,
    HttpStatus(StatusCode),
    ResponseTooLarge { limit: u64 },
    InvalidResponse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(error) => write!(formatter, "invalid Litteraturbanken URL: {error}"),
            Self::Network(error) => write!(formatter, "Litteraturbanken request failed: {error}"),
            Self::TimedOut => formatter.write_str("Litteraturbanken did not respond before the timeout"),
            Self::HttpStatus(status) => write!(formatter, "Litteraturbanken returned HTTP {status}"),
            Self::ResponseTooLarge { limit } => write!(formatter, "Litteraturbanken response exceeds the {limit}-byte limit"),
            Self::InvalidResponse(error) => write!(formatter, "invalid Litteraturbanken response: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(error) => Some(error),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error)
    }
}

#[derive(Deserialize)]
struct ApiResponse {
    hits: u32,
    data: Vec<ApiWork>,
}

#[derive(Deserialize)]
struct ApiWork {
    lbworkid: String,
    title: String,
    titleid: String,
    work_titleid: Option<String>,
    shorttitle: Option<String>,
    language: Option<String>,
    #[serde(default)]
    authors: Vec<ApiAuthor>,
}

#[derive(Deserialize)]
struct ApiAuthor {
    authorid: String,
    full_name: String,
    name_for_index: Option<String>,
    #[serde(rename = "type")]
    role: Option<String>,
}

pub fn is_catalog_url(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some(HOST) && url.path() == CATALOG_PATH
}

/// Adds the headers required to keep binary downloads out of the provider's
/// HTML snapshot path. Other providers' requests are returned unchanged.
pub fn decorate_download_request(request: RequestBuilder, url: &Url) -> RequestBuilder {
    if is_download_url(url) {
        request.header(reqwest::header::USER_AGENT, "Mozilla/5.0").header("x-requested-with", "XMLHttpRequest").header(reqwest::header::REFERER, CATALOG_URL)
    } else {
        request
    }
}

pub async fn fetch_catalog(client: &Client, source_url: Url, byte_limit: u64) -> Result<Catalog, Error> {
    if !is_catalog_url(&source_url) {
        return Err(Error::InvalidUrl("URL is not the Litteraturbanken EPUB catalogue".to_owned()));
    }
    let options = query_options(&source_url);
    let search = api_search(&options);
    let api_url = source_url.join(API_PATH).map_err(|error| Error::InvalidUrl(error.to_string()))?;
    let request = client.get(api_url).header(reqwest::header::ACCEPT, "application/json, text/plain, */*").header("x-requested-with", "XMLHttpRequest").header(reqwest::header::REFERER, source_url.as_str()).query(&[
        ("to", MAX_RESULTS),
        ("include", INCLUDED_FIELDS),
        ("exclude", EXCLUDED_FIELDS),
        ("sort_field", api_sort(options.sort.as_deref())),
        ("search", search.as_str()),
    ]);
    let response = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, request.send()).await.map_err(|_| Error::TimedOut)??;
    if !response.status().is_success() {
        return Err(Error::HttpStatus(response.status()));
    }
    let bytes = read_bounded(response, byte_limit).await?;
    let response: ApiResponse = serde_json::from_slice(&bytes).map_err(|error| Error::InvalidResponse(error.to_string()))?;
    catalog_from_response(source_url, options, response)
}

fn catalog_from_response(source_url: Url, options: QueryOptions, response: ApiResponse) -> Result<Catalog, Error> {
    let publications = response.data.into_iter().filter_map(|work| publication(&source_url, work).transpose()).collect::<Result<Vec<_>, _>>()?;
    let root_url = root_url(&source_url);
    let title = options.search.as_deref().filter(|query| !query.trim().is_empty()).map_or_else(|| "Litteraturbanken".to_owned(), |query| format!("Search results for {query}"));
    let facets = catalog_facets(&source_url, &options);
    let search_template = search_template(&source_url);
    Ok(Catalog { title, source_url, root_url, total_items: response.hits, publications, facets, search_template })
}

fn publication(base: &Url, work: ApiWork) -> Result<Option<Publication>, Error> {
    let Some(main_author) = work.authors.iter().find(|author| author.role.is_none()).or_else(|| work.authors.first()) else { return Ok(None) };
    let title_id = work.work_titleid.as_deref().unwrap_or(&work.titleid);
    let file_name = format!("{}_{}.epub", main_author.authorid, title_id);
    let mut download_url = base.join("/txt/epub/").map_err(|error| Error::InvalidUrl(error.to_string()))?;
    download_url.path_segments_mut().map_err(|_| Error::InvalidUrl("download URL cannot be a base".to_owned()))?.pop_if_empty().push(&file_name);
    let authors =
        work.authors.into_iter().filter(|author| author.role.is_none()).map(|author| Author { provider_url: author_url(base, &author.authorid).ok(), id: author.authorid, name: author.full_name, sort_name: author.name_for_index }).collect();
    Ok(Some(Publication { id: work.lbworkid, title: work.shorttitle.filter(|title| !title.trim().is_empty()).unwrap_or(work.title), authors, language: work.language.map(canonical_language), download_url, file_name }))
}

fn api_search(options: &QueryOptions) -> String {
    let mut boolean = json!({
        "filter": [
            { "term": { "has_epub": true } },
            { "term": { "show": true } }
        ]
    });
    if let Some(language) = options.language.as_deref().and_then(api_language) {
        boolean["filter"].as_array_mut().unwrap().push(json!({ "term": { "language": language } }));
    }
    if let Some(gender) = options.gender.as_deref().filter(|gender| matches!(*gender, "female" | "male")) {
        boolean["filter"].as_array_mut().unwrap().push(json!({ "term": { "authors.gender": gender } }));
    }
    if let Some(modernized) = options.modernized.as_deref().and_then(|value| match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }) {
        boolean["filter"].as_array_mut().unwrap().push(json!({ "term": { "modernized": modernized } }));
    }
    if let Some(query) = options.search.as_deref().filter(|query| !query.trim().is_empty()) {
        boolean["must"] = json!([{
            "multi_match": {
                "query": query,
                "fields": ["main_author.full_name.search", "title.search"]
            }
        }]);
    }
    json!({ "query": { "bool": boolean } }).to_string()
}

fn api_sort(sort: Option<&str>) -> &'static str {
    match sort {
        Some("author") => "main_author.name_for_index.lowercase|asc",
        Some("title") => "sortkey|asc",
        Some("year_asc") => "sort_date_imprint.date|asc",
        Some("year_desc") => "sort_date_imprint.date|desc",
        _ => "epub_popularity|desc",
    }
}

fn api_language(language: &str) -> Option<&'static str> {
    match language {
        "sv" => Some("swe"),
        "en" => Some("eng"),
        "de" => Some("deu"),
        "fr" => Some("fra"),
        "la" => Some("lat"),
        "smi" => Some("smi"),
        _ => None,
    }
}

fn query_options(url: &Url) -> QueryOptions {
    let mut options = QueryOptions::default();
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            SEARCH_PARAMETER => options.search = Some(value.into_owned()),
            SORT_PARAMETER => options.sort = Some(value.into_owned()),
            LANGUAGE_PARAMETER => options.language = Some(value.into_owned()),
            GENDER_PARAMETER => options.gender = Some(value.into_owned()),
            MODERNIZED_PARAMETER => options.modernized = Some(value.into_owned()),
            _ => {}
        }
    }
    options
}

fn catalog_facets(source_url: &Url, options: &QueryOptions) -> Vec<Facet> {
    vec![
        facet(source_url, "Sort by", SORT_PARAMETER, options.sort.as_deref(), &[("Popularity", "popularity"), ("Author", "author"), ("Title", "title"), ("Year, oldest first", "year_asc"), ("Year, newest first", "year_desc")]),
        facet(source_url, "Language", LANGUAGE_PARAMETER, options.language.as_deref(), &[("All languages", ""), ("Swedish", "sv"), ("English", "en"), ("German", "de"), ("French", "fr"), ("Latin", "la"), ("Sámi languages", "smi")]),
        facet(source_url, "Author", GENDER_PARAMETER, options.gender.as_deref(), &[("All authors", ""), ("Women", "female"), ("Men", "male")]),
        facet(source_url, "Text", MODERNIZED_PARAMETER, options.modernized.as_deref(), &[("All texts", ""), ("Modernized spelling", "true"), ("Original spelling", "false")]),
    ]
}

fn facet(source_url: &Url, title: &str, parameter: &str, selected: Option<&str>, choices: &[(&str, &str)]) -> Facet {
    let links = choices
        .iter()
        .map(|(label, value)| {
            let mut url = source_url.clone();
            let retained = url.query_pairs().filter(|(key, _)| key != parameter).map(|(key, value)| (key.into_owned(), value.into_owned())).collect::<Vec<_>>();
            url.query_pairs_mut().clear().extend_pairs(retained).extend_pairs((!value.is_empty()).then_some((parameter, *value)));
            FacetLink { title: (*label).to_owned(), url, active: selected.unwrap_or("") == *value || (parameter == SORT_PARAMETER && selected.is_none() && *value == "popularity") }
        })
        .collect();
    Facet { title: title.to_owned(), links }
}

fn search_template(source_url: &Url) -> String {
    let mut url = source_url.clone();
    let retained = url.query_pairs().filter(|(key, _)| key != SEARCH_PARAMETER).map(|(key, value)| (key.into_owned(), value.into_owned())).collect::<Vec<_>>();
    url.query_pairs_mut().clear().extend_pairs(retained).append_pair(SEARCH_PARAMETER, "{searchTerms}");
    url.to_string().replace("%7BsearchTerms%7D", "{searchTerms}")
}

fn root_url(source_url: &Url) -> Url {
    let mut root = source_url.clone();
    root.set_query(Some("visa=epub&sort=popularitet"));
    root
}

fn author_url(base: &Url, author_id: &str) -> Result<Url, Error> {
    let mut url = base.join("/författare/").map_err(|error| Error::InvalidUrl(error.to_string()))?;
    url.path_segments_mut().map_err(|_| Error::InvalidUrl("author URL cannot be a base".to_owned()))?.pop_if_empty().push(author_id);
    Ok(url)
}

fn canonical_language(language: String) -> String {
    match language.as_str() {
        "swe" => "sv".to_owned(),
        "deu" => "de".to_owned(),
        "eng" => "en".to_owned(),
        "fra" => "fr".to_owned(),
        _ => language,
    }
}

fn is_download_url(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some(HOST) && url.path().starts_with("/txt/epub/")
}

async fn read_bounded(response: reqwest::Response, limit: u64) -> Result<Vec<u8>, Error> {
    if response.content_length().is_some_and(|length| length > limit) {
        return Err(Error::ResponseTooLarge { limit });
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, stream.next()).await.map_err(|_| Error::TimedOut)? {
        let chunk = chunk?;
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > limit {
            return Err(Error::ResponseTooLarge { limit });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_the_curated_catalogue_path() {
        assert!(is_catalog_url(&Url::parse(CATALOG_URL).unwrap()));
        assert!(!is_catalog_url(&Url::parse("https://example.com/epub").unwrap()));
        assert!(!is_catalog_url(&Url::parse("https://litteraturbanken.se/api/query/etext").unwrap()));
    }

    #[test]
    fn converts_a_work_to_a_downloadable_publication() {
        let work = ApiWork {
            lbworkid: "lb1728740".to_owned(),
            title: "Doktor Glas. Roman".to_owned(),
            titleid: "DoktorGlas".to_owned(),
            work_titleid: None,
            shorttitle: Some("Doktor Glas".to_owned()),
            language: Some("swe".to_owned()),
            authors: vec![ApiAuthor { authorid: "SöderbergH".to_owned(), full_name: "Hjalmar Söderberg".to_owned(), name_for_index: Some("Söderberg, Hjalmar".to_owned()), role: None }],
        };
        let publication = publication(&Url::parse(CATALOG_URL).unwrap(), work).unwrap().unwrap();
        assert_eq!(publication.id, "lb1728740");
        assert_eq!(publication.title, "Doktor Glas");
        assert_eq!(publication.language.as_deref(), Some("sv"));
        assert_eq!(publication.authors[0].name, "Hjalmar Söderberg");
        assert_eq!(publication.download_url.as_str(), "https://litteraturbanken.se/txt/epub/S%C3%B6derbergH_DoktorGlas.epub");
    }

    #[test]
    fn search_filter_is_json_encoded_without_interpolation() {
        let search: serde_json::Value = serde_json::from_str(&api_search(&QueryOptions { search: Some("Glas \\\"roman\\\"".to_owned()), ..QueryOptions::default() })).unwrap();
        assert_eq!(search["query"]["bool"]["must"][0]["multi_match"]["query"], "Glas \\\"roman\\\"");
    }

    #[test]
    fn provider_controls_are_encoded_as_api_sort_and_filters() {
        let url = Url::parse("https://litteraturbanken.se/epub?visa=epub&bokheim_sort=year_desc&bokheim_language=de&bokheim_gender=female&bokheim_modernized=true").unwrap();
        let options = query_options(&url);
        assert_eq!(api_sort(options.sort.as_deref()), "sort_date_imprint.date|desc");
        let search: serde_json::Value = serde_json::from_str(&api_search(&options)).unwrap();
        let filters = search["query"]["bool"]["filter"].as_array().unwrap();
        assert!(filters.contains(&json!({ "term": { "language": "deu" } })));
        assert!(filters.contains(&json!({ "term": { "authors.gender": "female" } })));
        assert!(filters.contains(&json!({ "term": { "modernized": true } })));
    }

    #[test]
    fn facets_preserve_other_controls_and_search_template() {
        let url = Url::parse("https://litteraturbanken.se/epub?visa=epub&bokheim_sort=title&bokheim_language=sv").unwrap();
        let options = query_options(&url);
        let facets = catalog_facets(&url, &options);
        assert_eq!(facets.len(), 4);
        let german = facets[1].links.iter().find(|link| link.title == "German").unwrap();
        assert!(german.url.as_str().contains("bokheim_sort=title"));
        assert!(german.url.as_str().contains("bokheim_language=de"));
        assert_eq!(search_template(&url), "https://litteraturbanken.se/epub?visa=epub&bokheim_sort=title&bokheim_language=sv&bokheim_query={searchTerms}");
    }

    #[tokio::test]
    #[ignore = "requires the live Litteraturbanken API"]
    async fn live_catalogue_and_search_are_available() {
        let client = Client::builder().build().unwrap();
        let root = fetch_catalog(&client, Url::parse(CATALOG_URL).unwrap(), 24 * 1024 * 1024).await.unwrap();
        assert!(root.publications.len() > 1_000);
        let search_url = Url::parse("https://litteraturbanken.se/epub?visa=epub&sort=popularitet&bokheim_query=Doktor%20Glas").unwrap();
        let search = fetch_catalog(&client, search_url, 24 * 1024 * 1024).await.unwrap();
        assert!(search.publications.iter().any(|publication| publication.title == "Doktor Glas"));
        let german_url = Url::parse("https://litteraturbanken.se/epub?visa=epub&bokheim_sort=title&bokheim_language=de").unwrap();
        let german = fetch_catalog(&client, german_url, 24 * 1024 * 1024).await.unwrap();
        assert!(!german.publications.is_empty());
        assert!(german.publications.iter().all(|publication| publication.language.as_deref() == Some("de")));
    }
}
