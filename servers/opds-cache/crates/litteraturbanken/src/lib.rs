//! Client and provider model for Litteraturbanken's public EPUB catalogue.

use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use serde::Deserialize;
use serde_json::json;
use std::fmt;
use std::time::{Duration, SystemTime};

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
const PAGE_PARAMETER: &str = "bokheim_page";
pub const PAGE_SIZE: u32 = 48;
const NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const INCLUDED_FIELDS: &str = "lbworkid,titlepath,title,titleid,work_titleid,shorttitle,mediatype,authors.authorid,authors.full_name,authors.name_for_index,authors.type,language,series,isbn_download,urn,librisid,publisher,imprintyear,edition,imported,work_updated,export.type,export.updated,keyword,geokeyword,texttype,workintro_download,workintro_download_author,license,epub_license,epub_popularity,popularity,modernized,fraktur,cover_download_info,provenance.library,provenance.signum";
const EXCLUDED_FIELDS: &str = "text,parts,sourcedesc,pages,errata,content_vector";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub title: String,
    pub source_url: Url,
    pub root_url: Url,
    pub total_items: u32,
    pub current_page: u32,
    pub first_page: Option<Url>,
    pub previous_page: Option<Url>,
    pub next_page: Option<Url>,
    pub last_page: Option<Url>,
    pub books: Vec<Book>,
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
    page: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    pub id: String,
    pub title_id: String,
    pub work_title_id: Option<String>,
    pub title: String,
    pub alternate_titles: Vec<String>,
    pub authors: Vec<Author>,
    pub contributors: Vec<Author>,
    pub language: Option<String>,
    /// Provider-native series text. Litteraturbanken currently encodes the
    /// position as the final integer, for example `Barnbiblioteket Saga 3`.
    pub series: Option<String>,
    pub isbn: Option<String>,
    pub urn: Option<String>,
    pub libris_id: Option<String>,
    pub publisher: Vec<String>,
    pub imprint_year: Option<String>,
    pub edition: Option<String>,
    pub imported: Option<String>,
    pub work_updated: Option<String>,
    pub epub_updated: Option<String>,
    pub keywords: Vec<String>,
    pub geographic_keywords: Vec<String>,
    pub text_type: Option<String>,
    pub media_type: Option<String>,
    pub description_html: Option<String>,
    /// Provider-supplied attribution for the work introduction. This field is
    /// heterogeneous and may contain a local key, prose, or HTML.
    pub description_attribution: Option<String>,
    pub rights: Vec<String>,
    pub popularity: Option<u64>,
    pub epub_popularity: Option<u64>,
    pub modernized: Option<bool>,
    pub fraktur: Option<bool>,
    pub cover_attribution: Option<String>,
    pub sources: Vec<String>,
    pub provider_url: Option<Url>,
    pub download_url: Url,
    pub file_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub sort_name: Option<String>,
    pub role: Option<String>,
    pub provider_url: Option<Url>,
}

#[derive(Debug)]
pub enum Error {
    InvalidUrl(String),
    Network(reqwest::Error),
    TimedOut,
    HttpStatus { status: StatusCode, retry_after: Option<Duration> },
    ResponseTooLarge { limit: u64 },
    InvalidResponse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(error) => write!(formatter, "invalid Litteraturbanken URL: {error}"),
            Self::Network(error) => write!(formatter, "Litteraturbanken request failed: {error}"),
            Self::TimedOut => formatter.write_str("Litteraturbanken did not respond before the timeout"),
            Self::HttpStatus { status, retry_after } => {
                write!(formatter, "Litteraturbanken returned HTTP {status}")?;
                if let Some(retry_after) = retry_after {
                    write!(formatter, " (retry after {} seconds)", retry_after.as_secs())?;
                }
                Ok(())
            }
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

#[derive(Default, Deserialize)]
struct ApiWork {
    lbworkid: String,
    title: String,
    titleid: String,
    work_titleid: Option<String>,
    titlepath: Option<String>,
    shorttitle: Option<String>,
    mediatype: Option<String>,
    language: Option<String>,
    series: Option<String>,
    isbn_download: Option<String>,
    urn: Option<String>,
    librisid: Option<String>,
    #[serde(default)]
    publisher: Vec<String>,
    imprintyear: Option<String>,
    edition: Option<String>,
    imported: Option<String>,
    work_updated: Option<String>,
    #[serde(default)]
    export: Vec<ApiExport>,
    #[serde(default)]
    keyword: Vec<String>,
    geokeyword: Option<String>,
    texttype: Option<String>,
    workintro_download: Option<String>,
    workintro_download_author: Option<String>,
    license: Option<String>,
    epub_license: Option<String>,
    epub_popularity: Option<u64>,
    popularity: Option<u64>,
    modernized: Option<bool>,
    fraktur: Option<bool>,
    cover_download_info: Option<String>,
    #[serde(default)]
    provenance: Vec<ApiProvenance>,
    #[serde(default)]
    authors: Vec<ApiAuthor>,
}

#[derive(Deserialize)]
struct ApiExport {
    #[serde(rename = "type")]
    export_type: Option<String>,
    updated: Option<String>,
}

#[derive(Deserialize)]
struct ApiProvenance {
    library: Option<String>,
    signum: Option<String>,
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

/// Adds the same provider-specific download headers without binding callers to
/// a particular HTTP client or redirect policy.
pub fn decorate_download_headers(headers: &mut reqwest::header::HeaderMap, url: &Url) {
    if is_download_url(url) {
        headers.insert(reqwest::header::USER_AGENT, reqwest::header::HeaderValue::from_static("Mozilla/5.0"));
        headers.insert("x-requested-with", reqwest::header::HeaderValue::from_static("XMLHttpRequest"));
        headers.insert(reqwest::header::REFERER, reqwest::header::HeaderValue::from_static(CATALOG_URL));
    }
}

pub async fn fetch_catalog(client: &Client, source_url: Url, byte_limit: u64) -> Result<Catalog, Error> {
    if !is_catalog_url(&source_url) {
        return Err(Error::InvalidUrl("URL is not the Litteraturbanken EPUB catalogue".to_owned()));
    }
    let options = query_options(&source_url);
    let search = api_search(&options);
    let page = options.page.max(1);
    let offset = page.saturating_sub(1).saturating_mul(PAGE_SIZE);
    let offset = offset.to_string();
    let end = page.saturating_mul(PAGE_SIZE).to_string();
    let api_url = source_url.join(API_PATH).map_err(|error| Error::InvalidUrl(error.to_string()))?;
    let request = client.get(api_url).header(reqwest::header::ACCEPT, "application/json, text/plain, */*").header("x-requested-with", "XMLHttpRequest").header(reqwest::header::REFERER, source_url.as_str()).query(&[
        ("from", offset.as_str()),
        ("to", end.as_str()),
        ("include", INCLUDED_FIELDS),
        ("exclude", EXCLUDED_FIELDS),
        ("sort_field", api_sort(options.sort.as_deref())),
        ("search", search.as_str()),
    ]);
    let response = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, request.send()).await.map_err(|_| Error::TimedOut)??;
    if !response.status().is_success() {
        return Err(Error::HttpStatus { status: response.status(), retry_after: retry_after(response.headers(), SystemTime::now()) });
    }
    let bytes = read_bounded(response, byte_limit).await?;
    let response: ApiResponse = serde_json::from_slice(&bytes).map_err(|error| Error::InvalidResponse(error.to_string()))?;
    catalog_from_response(source_url, options, response)
}

fn retry_after(headers: &reqwest::header::HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    Some(httpdate::parse_http_date(value).ok()?.duration_since(now).unwrap_or(Duration::ZERO))
}

fn catalog_from_response(source_url: Url, options: QueryOptions, response: ApiResponse) -> Result<Catalog, Error> {
    let books = response.data.into_iter().filter_map(|work| book(&source_url, work).transpose()).collect::<Result<Vec<_>, _>>()?;
    let root_url = root_url(&source_url);
    let title = options.search.as_deref().filter(|query| !query.trim().is_empty()).map_or_else(|| "Litteraturbanken".to_owned(), |query| format!("Search results for {query}"));
    let facets = catalog_facets(&source_url, &options);
    let search_template = search_template(&source_url);
    let current_page = options.page.max(1);
    let page_count = response.hits.div_ceil(PAGE_SIZE).max(1);
    let first_page = (current_page > 1).then(|| page_url(&source_url, 1));
    let previous_page = (current_page > 1).then(|| page_url(&source_url, current_page - 1));
    let next_page = (current_page < page_count).then(|| page_url(&source_url, current_page + 1));
    let last_page = (current_page < page_count).then(|| page_url(&source_url, page_count));
    Ok(Catalog { title, source_url, root_url, total_items: response.hits, current_page, first_page, previous_page, next_page, last_page, books, facets, search_template })
}

fn book(base: &Url, work: ApiWork) -> Result<Option<Book>, Error> {
    let Some(main_author) = work.authors.iter().find(|author| author.role.is_none()).or_else(|| work.authors.first()) else { return Ok(None) };
    let main_author_id = main_author.authorid.clone();
    let has_untyped_author = work.authors.iter().any(|author| author.role.is_none());
    let title_id = work.work_titleid.as_deref().unwrap_or(&work.titleid);
    let file_name = format!("{}_{}.epub", main_author.authorid, title_id);
    let provider_url = work.titlepath.as_deref().and_then(|title_path| book_url(base, &main_author_id, title_path).ok());
    let full_title = work.title;
    let title = work.shorttitle.filter(|title| !title.trim().is_empty()).unwrap_or_else(|| full_title.clone());
    let alternate_titles = (title != full_title).then_some(full_title).into_iter().collect();
    let mut download_url = base.join("/txt/epub/").map_err(|error| Error::InvalidUrl(error.to_string()))?;
    download_url.path_segments_mut().map_err(|_| Error::InvalidUrl("download URL cannot be a base".to_owned()))?.pop_if_empty().push(&file_name);
    let mut authors = Vec::new();
    let mut contributors = Vec::new();
    for author in work.authors {
        let primary = author.role.is_none() || (!has_untyped_author && author.authorid == main_author_id);
        let credit = Author { provider_url: author_url(base, &author.authorid).ok(), id: author.authorid, name: author.full_name, sort_name: author.name_for_index, role: author.role };
        if primary {
            authors.push(credit);
        } else {
            contributors.push(credit);
        }
    }
    let epub_updated = work.export.iter().find(|value| value.export_type.as_deref() == Some("epub")).and_then(|value| value.updated.clone());
    let mut rights = [work.license, work.epub_license].into_iter().flatten().filter(|value| !value.trim().is_empty()).collect::<Vec<_>>();
    rights.sort();
    rights.dedup();
    let sources = work
        .provenance
        .into_iter()
        .filter_map(|value| match (value.library.filter(|value| !value.trim().is_empty()), value.signum.filter(|value| !value.trim().is_empty())) {
            (Some(library), Some(signum)) => Some(format!("{library}: {signum}")),
            (Some(library), None) => Some(library),
            (None, Some(signum)) => Some(signum),
            (None, None) => None,
        })
        .collect();
    Ok(Some(Book {
        id: work.lbworkid,
        title_id: work.titleid,
        work_title_id: work.work_titleid,
        title,
        alternate_titles,
        authors,
        contributors,
        language: work.language.map(canonical_language),
        series: work.series.filter(|series| !series.trim().is_empty()),
        isbn: work.isbn_download.filter(|value| !value.trim().is_empty()),
        urn: work.urn.filter(|value| !value.trim().is_empty()),
        libris_id: work.librisid.filter(|value| !value.trim().is_empty()),
        publisher: work.publisher.into_iter().filter(|value| !value.trim().is_empty()).collect(),
        imprint_year: work.imprintyear.filter(|value| !value.trim().is_empty()),
        edition: work.edition.filter(|value| !value.trim().is_empty()),
        imported: work.imported.filter(|value| !value.trim().is_empty()),
        work_updated: work.work_updated.filter(|value| !value.trim().is_empty()),
        epub_updated,
        keywords: work.keyword.into_iter().filter(|value| !value.trim().is_empty()).collect(),
        geographic_keywords: work.geokeyword.filter(|value| !value.trim().is_empty()).into_iter().collect(),
        text_type: work.texttype.filter(|value| !value.trim().is_empty()),
        media_type: work.mediatype.filter(|value| !value.trim().is_empty()),
        description_html: work.workintro_download.filter(|value| !value.trim().is_empty()),
        description_attribution: work.workintro_download_author.filter(|value| !value.trim().is_empty()),
        rights,
        popularity: work.popularity,
        epub_popularity: work.epub_popularity,
        modernized: work.modernized,
        fraktur: work.fraktur,
        cover_attribution: work.cover_download_info.filter(|value| !value.trim().is_empty()),
        sources,
        provider_url,
        download_url,
        file_name,
    }))
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
            PAGE_PARAMETER => options.page = value.parse().ok().filter(|page| *page > 0).unwrap_or(1),
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
            let retained = url.query_pairs().filter(|(key, _)| key != parameter && key != PAGE_PARAMETER).map(|(key, value)| (key.into_owned(), value.into_owned())).collect::<Vec<_>>();
            url.query_pairs_mut().clear().extend_pairs(retained).extend_pairs((!value.is_empty()).then_some((parameter, *value)));
            FacetLink { title: (*label).to_owned(), url, active: selected.unwrap_or("") == *value || (parameter == SORT_PARAMETER && selected.is_none() && *value == "popularity") }
        })
        .collect();
    Facet { title: title.to_owned(), links }
}

fn search_template(source_url: &Url) -> String {
    let mut url = source_url.clone();
    let retained = url.query_pairs().filter(|(key, _)| key != SEARCH_PARAMETER && key != PAGE_PARAMETER).map(|(key, value)| (key.into_owned(), value.into_owned())).collect::<Vec<_>>();
    url.query_pairs_mut().clear().extend_pairs(retained).append_pair(SEARCH_PARAMETER, "{searchTerms}");
    url.to_string().replace("%7BsearchTerms%7D", "{searchTerms}")
}

fn page_url(source_url: &Url, page: u32) -> Url {
    let mut url = source_url.clone();
    let retained = url.query_pairs().filter(|(key, _)| key != PAGE_PARAMETER).map(|(key, value)| (key.into_owned(), value.into_owned())).collect::<Vec<_>>();
    url.query_pairs_mut().clear().extend_pairs(retained).extend_pairs((page > 1).then_some((PAGE_PARAMETER, page.to_string())));
    url
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

fn book_url(base: &Url, author_id: &str, title_path: &str) -> Result<Url, Error> {
    let mut url = author_url(base, author_id)?;
    url.path_segments_mut().map_err(|_| Error::InvalidUrl("book URL cannot be a base".to_owned()))?.push("titlar").push(title_path).push("info");
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
    fn parses_provider_retry_after() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "45".parse().unwrap());
        assert_eq!(retry_after(&headers, SystemTime::UNIX_EPOCH), Some(Duration::from_secs(45)));
    }

    #[test]
    fn converts_a_work_to_a_downloadable_book() {
        let work = ApiWork {
            lbworkid: "lb1728740".to_owned(),
            title: "Doktor Glas. Roman".to_owned(),
            titleid: "DoktorGlas".to_owned(),
            work_titleid: None,
            titlepath: Some("DoktorGlas".to_owned()),
            shorttitle: Some("Doktor Glas".to_owned()),
            language: Some("swe".to_owned()),
            series: Some("Barnbiblioteket Saga 3".to_owned()),
            authors: vec![ApiAuthor { authorid: "SöderbergH".to_owned(), full_name: "Hjalmar Söderberg".to_owned(), name_for_index: Some("Söderberg, Hjalmar".to_owned()), role: None }],
            ..ApiWork::default()
        };
        let book = book(&Url::parse(CATALOG_URL).unwrap(), work).unwrap().unwrap();
        assert_eq!(book.id, "lb1728740");
        assert_eq!(book.title, "Doktor Glas");
        assert_eq!(book.alternate_titles, ["Doktor Glas. Roman"]);
        assert_eq!(book.language.as_deref(), Some("sv"));
        assert_eq!(book.series.as_deref(), Some("Barnbiblioteket Saga 3"));
        assert_eq!(book.authors[0].name, "Hjalmar Söderberg");
        assert_eq!(book.provider_url.as_ref().map(Url::as_str), Some("https://litteraturbanken.se/f%C3%B6rfattare/S%C3%B6derbergH/titlar/DoktorGlas/info"));
        assert_eq!(book.download_url.as_str(), "https://litteraturbanken.se/txt/epub/S%C3%B6derbergH_DoktorGlas.epub");
    }

    #[test]
    fn deserializes_and_retains_rich_catalogue_metadata() {
        let work: ApiWork = serde_json::from_value(json!({
            "lbworkid": "lb1728740",
            "title": "Doktor Glas. Roman",
            "titleid": "DoktorGlas",
            "work_titleid": "DoktorGlasSamlade",
            "titlepath": "DoktorGlas",
            "shorttitle": "Doktor Glas",
            "mediatype": "etext",
            "language": "swe",
            "series": "Barnbiblioteket Saga 3",
            "isbn_download": "978-91-8123-064-2",
            "urn": "urn:nbn:se:lb-lb1728740-etext",
            "librisid": "1728740",
            "publisher": ["Almqvist &amp; Wiksell förlag", "Stockholm", "SE"],
            "imprintyear": "1905",
            "edition": "1",
            "imported": "2012-01-17",
            "work_updated": "2026-04-27",
            "export": [{"type": "epub", "updated": "2026-05-21"}, {"type": "txt", "updated": "2026-04-28"}],
            "keyword": ["Brott", "Dagboksromaner"],
            "geokeyword": "Stockholm",
            "texttype": "roman",
            "workintro_download": "<p>En kort introduktion.</p>",
            "workintro_download_author": "HelgesonP",
            "license": "cc-0",
            "epub_license": "cc-0",
            "popularity": 2059021,
            "epub_popularity": 9705,
            "modernized": false,
            "fraktur": false,
            "cover_download_info": "Omslag från GUB.",
            "provenance": [{"library": "GUB", "signum": "Litt. Sv."}],
            "authors": [
                {"authorid": "SöderbergH", "full_name": "Hjalmar Söderberg", "name_for_index": "Söderberg, Hjalmar"},
                {"authorid": "EditorE", "full_name": "Eva Redaktör", "name_for_index": "Redaktör, Eva", "type": "editor"}
            ]
        }))
        .unwrap();

        let book = book(&Url::parse(CATALOG_URL).unwrap(), work).unwrap().unwrap();
        assert_eq!(book.title_id, "DoktorGlas");
        assert_eq!(book.work_title_id.as_deref(), Some("DoktorGlasSamlade"));
        assert_eq!(book.alternate_titles, ["Doktor Glas. Roman"]);
        assert_eq!(book.media_type.as_deref(), Some("etext"));
        assert_eq!(book.isbn.as_deref(), Some("978-91-8123-064-2"));
        assert_eq!(book.publisher, ["Almqvist &amp; Wiksell förlag", "Stockholm", "SE"]);
        assert_eq!(book.epub_updated.as_deref(), Some("2026-05-21"));
        assert_eq!(book.keywords, ["Brott", "Dagboksromaner"]);
        assert_eq!(book.geographic_keywords, ["Stockholm"]);
        assert_eq!(book.description_attribution.as_deref(), Some("HelgesonP"));
        assert_eq!(book.contributors[0].role.as_deref(), Some("editor"));
        assert_eq!(book.rights, ["cc-0"]);
        assert_eq!(book.sources, ["GUB: Litt. Sv."]);
        assert_eq!(book.popularity, Some(2_059_021));
        assert_eq!(book.epub_popularity, Some(9_705));
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
        let url = Url::parse("https://litteraturbanken.se/epub?visa=epub&bokheim_sort=title&bokheim_language=sv&bokheim_page=3").unwrap();
        let options = query_options(&url);
        let facets = catalog_facets(&url, &options);
        assert_eq!(facets.len(), 4);
        let german = facets[1].links.iter().find(|link| link.title == "German").unwrap();
        assert!(german.url.as_str().contains("bokheim_sort=title"));
        assert!(german.url.as_str().contains("bokheim_language=de"));
        assert!(!german.url.as_str().contains("bokheim_page"));
        assert_eq!(search_template(&url), "https://litteraturbanken.se/epub?visa=epub&bokheim_sort=title&bokheim_language=sv&bokheim_query={searchTerms}");
    }

    #[test]
    fn large_catalogues_are_exposed_as_bounded_pages() {
        let source_url = Url::parse("https://litteraturbanken.se/epub?visa=epub&sort=popularitet&bokheim_page=2").unwrap();
        let catalog = catalog_from_response(source_url, QueryOptions { page: 2, ..QueryOptions::default() }, ApiResponse { hits: 1_618, data: Vec::new() }).unwrap();
        assert_eq!(catalog.current_page, 2);
        assert_eq!(catalog.first_page.as_ref().unwrap().as_str(), CATALOG_URL);
        assert_eq!(catalog.previous_page.as_ref().unwrap().as_str(), CATALOG_URL);
        assert!(catalog.next_page.as_ref().unwrap().as_str().contains("bokheim_page=3"));
        assert!(catalog.last_page.as_ref().unwrap().as_str().contains("bokheim_page=34"));
    }

    #[tokio::test]
    #[ignore = "requires the live Litteraturbanken API"]
    async fn live_catalogue_and_search_are_available() {
        let client = Client::builder().build().unwrap();
        let root = fetch_catalog(&client, Url::parse(CATALOG_URL).unwrap(), 24 * 1024 * 1024).await.unwrap();
        assert_eq!(root.books.len(), PAGE_SIZE as usize);
        assert!(root.total_items > 1_000);
        assert!(root.next_page.is_some());
        let second = fetch_catalog(&client, root.next_page.clone().unwrap(), 24 * 1024 * 1024).await.unwrap();
        assert_eq!(second.current_page, 2);
        assert_eq!(second.books.len(), PAGE_SIZE as usize);
        assert_ne!(second.books[0].id, root.books[0].id);
        let search_url = Url::parse("https://litteraturbanken.se/epub?visa=epub&sort=popularitet&bokheim_query=Doktor%20Glas").unwrap();
        let search = fetch_catalog(&client, search_url, 24 * 1024 * 1024).await.unwrap();
        assert!(search.books.iter().any(|book| book.title == "Doktor Glas"));
        let german_url = Url::parse("https://litteraturbanken.se/epub?visa=epub&bokheim_sort=title&bokheim_language=de").unwrap();
        let german = fetch_catalog(&client, german_url, 24 * 1024 * 1024).await.unwrap();
        assert!(!german.books.is_empty());
        assert!(german.books.iter().all(|book| book.language.as_deref() == Some("de")));
    }
}
