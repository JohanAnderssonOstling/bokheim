//! Server-owned exact-ISBN access to the Library of Congress SRU authority service.
//!
//! LCC, DDC, LCSH, and LCGFT values are cached with provider provenance.
//! This module neither bundles classification schedules nor matches embedded
//! book subjects against them.

use std::collections::HashMap;
use std::sync::Mutex;
use crate::{Classification, ClassificationScheme, ClassificationSource, Isbn13, MetadataError, SubjectHeading};
use futures_util::StreamExt;
use reqwest::Url;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const PROVIDER_ID: &str = "library_of_congress";
pub(crate) const DEFAULT_SRU_ENDPOINT: &str = "http://lx2.loc.gov:210/LCDB";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_BATCH_ISBNS: usize = 50;
const PAGE_SIZE: usize = 100;
const MAX_BATCH_RECORDS: usize = 1000;
const REQUEST_INTERVAL_MS: i64 = 6000;

#[derive(Clone)]
pub(crate) struct LibraryOfCongressClient {
    client: reqwest::Client,
    endpoint: Url,
    next_request_ms: Arc<Mutex<i64>>,
    cache: Option<Arc<Mutex<rusqlite::Connection>>>,
    request_lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum LibraryOfCongressLookup {
    Matched { classifications: Vec<Classification>, subjects: Vec<SubjectHeading> },
    NoMatch,
    Ambiguous,
}

#[derive(Clone, Debug)]
struct LibraryOfCongressRecord {
    provider_record_id: Option<String>,
    isbns: Vec<Isbn13>,
    classifications: Vec<Classification>,
    subjects: Vec<SubjectHeading>,
}

impl LibraryOfCongressClient {
    pub(crate) fn new() -> Result<Self, MetadataError> {
        Self::with_endpoint(DEFAULT_SRU_ENDPOINT)
    }

    pub(crate) fn with_endpoint(endpoint: &str) -> Result<Self, MetadataError> {
        let endpoint = Url::parse(endpoint).map_err(|error| MetadataError(format!("invalid Library of Congress SRU endpoint: {error}")))?;
        let official_http = endpoint.scheme() == "http" && endpoint.host_str() == Some("lx2.loc.gov") && endpoint.port() == Some(210);
        let test_http = cfg!(test) && endpoint.scheme() == "http" && endpoint.host_str() == Some("127.0.0.1");
        if (endpoint.scheme() != "https" && !official_http && !test_http) || !endpoint.username().is_empty() || endpoint.password().is_some() || endpoint.query().is_some() || endpoint.fragment().is_some() {
            return Err(MetadataError("Library of Congress SRU endpoint must use HTTPS or the official LC port-210 server, without credentials, query, or fragment".to_owned()));
        }
        let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(15)).build().map_err(|error| MetadataError(format!("could not create Library of Congress HTTP client: {error}")))?;
        Ok(Self { client, endpoint, next_request_ms: Arc::new(Mutex::new(0)), cache: None, request_lock: Arc::new(tokio::sync::Mutex::new(())) })
    }

    pub(crate) fn with_cache(mut self, path: &std::path::Path) -> Result<Self, MetadataError> {
        self.cache = Some(Arc::new(Mutex::new(crate::crawl::open_provider(path)?)));
        Ok(self)
    }

    pub(crate) fn cached(&self, isbn: Isbn13) -> Result<Option<LibraryOfCongressLookup>, MetadataError> {
        let Some(cache) = &self.cache else { return Ok(None) };
        let c = cache.lock().map_err(crate::error)?;
        crate::crawl::load_loc(&c, isbn)
    }

    pub(crate) fn retry_ready(&self, isbn: Isbn13) -> Result<bool, MetadataError> {
        use rusqlite::OptionalExtension;
        let Some(cache) = &self.cache else { return Ok(true) };
        let c = cache.lock().map_err(crate::error)?;
        let retry: Option<i64> = c.query_row("SELECT retry_at_ms FROM provider_retry WHERE provider='loc' AND isbn13=?", [isbn], |r| r.get(0)).optional().map_err(crate::error)?;
        Ok(!retry.is_some_and(|t| t > epoch_ms()))
    }

    /// Cache a batch only after every result page has been retrieved and validated.
    pub(crate) async fn lookup_batch(&self, keys: &[Isbn13]) -> Result<Vec<LibraryOfCongressLookup>, MetadataError> {
        if keys.len() > MAX_BATCH_ISBNS {
            return Err(MetadataError("LC batch exceeds 50 ISBNs".into()));
        }
        let mut found = HashMap::new();
        for &key in keys {
            if let Some(v) = self.cached(key)? {
                found.insert(key, v);
            }
        }
        if keys.iter().all(|k| found.contains_key(k)) {
            return Ok(keys.iter().map(|k| found[k].clone()).collect());
        }
        let _guard = self.request_lock.lock().await;
        let mut pending = Vec::new();
        for &key in keys {
            if let Some(v) = self.cached(key)? {
                found.insert(key, v);
            } else if !pending.contains(&key) {
                if !self.retry_ready(key)? {
                    return Err(MetadataError("Library of Congress retry backoff".into()));
                }
                pending.push(key);
            }
        }
        if !pending.is_empty() {
            let lookup = self.request_batch(&pending).await;
            if let Some(cache) = &self.cache {
                let mut c = cache.lock().map_err(crate::error)?;
                let tx = c.transaction().map_err(crate::error)?;
                for (i, key) in pending.iter().enumerate() {
                    match &lookup {
                        Ok(values) => {
                            crate::crawl::save_loc(&tx, *key, &values[i])?;
                            tx.execute("DELETE FROM provider_retry WHERE provider='loc' AND isbn13=?", [key]).map_err(crate::error)?;
                        }
                        Err(e) => {
                            tx.execute("INSERT OR REPLACE INTO provider_retry VALUES('loc',?,?,?)", rusqlite::params![key, epoch_ms() + 3_600_000, e.to_string()]).map_err(crate::error)?;
                        }
                    }
                }
                tx.commit().map_err(crate::error)?;
            }
            for (key, value) in pending.into_iter().zip(lookup?) {
                found.insert(key, value);
            }
        }
        Ok(keys.iter().map(|k| found[k].clone()).collect())
    }

    // Persistent reservation is shared by foreground, background and restarts.
    fn reserve_request(&self, now: i64) -> Result<i64, MetadataError> {
        use rusqlite::OptionalExtension;
        if let Some(cache) = &self.cache {
            let mut c = cache.lock().map_err(crate::error)?;
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(crate::error)?;
            let next = tx.query_row("SELECT next_ms FROM provider_rate WHERE provider='loc'", [], |r| r.get::<_, i64>(0)).optional().map_err(crate::error)?.unwrap_or(0);
            if next > now {
                return Ok(next - now);
            }
            tx.execute("INSERT INTO provider_rate VALUES('loc',?) ON CONFLICT(provider) DO UPDATE SET next_ms=excluded.next_ms", [now + REQUEST_INTERVAL_MS]).map_err(crate::error)?;
            tx.commit().map_err(crate::error)?;
        } else {
            let mut next = self.next_request_ms.lock().map_err(crate::error)?;
            if *next > now {
                return Ok(*next - now);
            }
            *next = now + REQUEST_INTERVAL_MS;
        }
        Ok(0)
    }

    async fn request_batch(&self, isbns: &[Isbn13]) -> Result<Vec<LibraryOfCongressLookup>, MetadataError> {
        let terms = isbns.iter().flat_map(|isbn| std::iter::once(isbn.to_string()).chain(isbn.to_isbn10())).map(|i| format!("bath.isbn=\"{i}\"")).collect::<Vec<_>>();
        let query = terms.join(" or ");
        let mut records = Vec::new();
        let mut expected_total = None;
        let mut seen = HashSet::new();
        loop {
            loop {
                let delay = self.reserve_request(epoch_ms())?;
                if delay == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(delay as u64)).await;
            }
            let start = records.len() + 1;
            let response = self
                .client
                .get(self.endpoint.clone())
                .query(&[("version", "1.1"), ("operation", "searchRetrieve"), ("query", query.as_str()), ("maximumRecords", &PAGE_SIZE.to_string()), ("startRecord", &start.to_string()), ("recordSchema", "marcxml")])
                .send()
                .await
                .map_err(crate::error)?;
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS || response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                let seconds = response.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<i64>().ok()).unwrap_or(60).clamp(6, 86400);
                let until = epoch_ms() + seconds * 1000;
                if let Some(cache) = &self.cache {
                    cache.lock().map_err(crate::error)?.execute("UPDATE provider_rate SET next_ms=MAX(next_ms,?) WHERE provider='loc'", [until]).map_err(crate::error)?;
                } else {
                    *self.next_request_ms.lock().map_err(crate::error)? = until;
                }
            }
            let response = response.error_for_status().map_err(crate::error)?;
            if response.content_length().is_some_and(|n| n > MAX_RESPONSE_BYTES as u64) {
                return Err(MetadataError("LC response size limit".into()));
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(crate::error)?;
                if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    return Err(MetadataError("LC response size limit".into()));
                }
                bytes.extend_from_slice(&chunk);
            }
            let (total, page) = parse_sru_page(&bytes)?;
            if total > MAX_BATCH_RECORDS {
                return Err(MetadataError("LC result set exceeds batch limit".into()));
            }
            if expected_total.is_some_and(|n| n != total) {
                return Err(MetadataError("LC result count changed during pagination".into()));
            }
            expected_total = Some(total);
            if records.len() + page.len() > total || (page.is_empty() && records.len() < total) {
                return Err(MetadataError("LC incomplete result page".into()));
            }
            for record in page {
                let id = record.provider_record_id.as_ref().ok_or_else(|| MetadataError("LC record lacks a stable identifier".into()))?;
                if !seen.insert(id.clone()) {
                    return Err(MetadataError("LC repeated record during pagination".into()));
                }
                records.push(record);
            }
            if records.len() == total {
                break;
            }
        }
        tracing::info!(isbns = isbns.len(), records = records.len(), "LC batch completed");
        isbns.iter().map(|&isbn| match_isbn_verified(isbn, &records)).collect()
    }
}

fn epoch_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

fn match_isbn_verified(isbn: Isbn13, records: &[LibraryOfCongressRecord]) -> Result<LibraryOfCongressLookup, MetadataError> {
    let mut exact = records.iter().filter(|r| r.isbns.contains(&isbn));
    let Some(record) = exact.next() else { return Ok(LibraryOfCongressLookup::NoMatch) };
    if exact.next().is_some() {
        return Ok(LibraryOfCongressLookup::Ambiguous);
    }
    let mut classifications = record.classifications.clone();
    for c in &mut classifications {
        c.evidence.push(metadata_contract::ClassificationEvidence {
            method: format!("loc_api_isbn:{}", record.provider_record_id.as_deref().unwrap_or("unknown")),
            isbn13: isbn.to_string(),
            open_library_work_id: String::new(),
            open_library_edition_id: String::new(),
        });
    }
    Ok(LibraryOfCongressLookup::Matched { classifications, subjects: record.subjects.clone() })
}

fn parse_sru_page(bytes: &[u8]) -> Result<(usize, Vec<LibraryOfCongressRecord>), MetadataError> {
    let xml = std::str::from_utf8(bytes).map_err(crate::error)?;
    let document = roxmltree::Document::parse(xml).map_err(crate::error)?;
    if document.root_element().tag_name().name() != "searchRetrieveResponse" {
        return Err(MetadataError("unexpected LC response envelope".into()));
    }
    if let Some(d) = document.descendants().find(|n| n.tag_name().name() == "diagnostic") {
        return Err(MetadataError(format!("LC SRU diagnostic: {}", d.descendants().filter_map(|n| n.text()).collect::<Vec<_>>().join(" "))));
    }
    let total = document
        .root_element()
        .children()
        .find(|n| n.tag_name().name() == "numberOfRecords")
        .and_then(|n| n.text())
        .and_then(|v| v.trim().parse::<usize>().ok())
        .ok_or_else(|| MetadataError("LC response lacks a valid record count".into()))?;
    let mut records = document.descendants().filter(|n| n.tag_name().name() == "record" && n.tag_name().namespace() == Some("http://www.loc.gov/MARC21/slim")).map(parse_marc_record).collect::<Vec<_>>();
    if records.len() > PAGE_SIZE || records.len() > total || (total > 0 && records.is_empty()) {
        return Err(MetadataError("invalid LC page record count".into()));
    }
    for r in &mut records {
        r.isbns.sort();
        r.isbns.dedup();
        r.classifications.sort();
        r.classifications.dedup();
        r.subjects.sort();
        r.subjects.dedup();
    }
    Ok((total, records))
}

fn parse_marc_record(record: roxmltree::Node<'_, '_>) -> LibraryOfCongressRecord {
    let fields = record.children().filter(|node| node.tag_name().name() == "datafield").collect::<Vec<_>>();
    let first = |tag: &str, code: &str| fields.iter().find(|field| field.attribute("tag") == Some(tag)).and_then(|field| subfield(*field, code));
    let provider_record_id =
        record.children().find(|n| n.tag_name().name() == "controlfield" && n.attribute("tag") == Some("001")).and_then(|n| n.text()).map(compact_whitespace).filter(|s| !s.is_empty()).or_else(|| first("010", "a").map(compact_whitespace));
    let isbns = fields.iter().filter(|field| field.attribute("tag") == Some("020")).filter_map(|field| subfield(*field, "a")).filter_map(|value| value.split_whitespace().next()).filter_map(Isbn13::parse).collect();
    let mut classifications = marc_classifications(&fields, "050", ClassificationScheme::LibraryOfCongress);
    for field in fields.iter().filter(|f| f.attribute("tag") == Some("082")) {
        if let Some(notation) = subfield(*field, "a").and_then(|v| metadata_contract::classification_consensus_notation(ClassificationScheme::DeweyDecimal, v)) {
            classifications.push(Classification { scheme: ClassificationScheme::DeweyDecimal, notation, source: ClassificationSource::ExactEdition, evidence: Vec::new() });
        }
    }
    let subjects = fields.iter().filter(|field| matches!(field.attribute("tag"), Some("600" | "610" | "611" | "630" | "648" | "650" | "651" | "655"))).filter_map(|field| marc_subject(*field)).collect();
    LibraryOfCongressRecord { provider_record_id, isbns, classifications, subjects }
}

fn marc_classifications(fields: &[roxmltree::Node<'_, '_>], tag: &str, scheme: ClassificationScheme) -> Vec<Classification> {
    fields
        .iter()
        .filter(|field| field.attribute("tag") == Some(tag))
        .filter_map(|field| {
            let code = [subfield(*field, "a"), subfield(*field, "b")].into_iter().flatten().map(compact_whitespace).collect::<Vec<_>>().join(" ");
            metadata_contract::classification_consensus_notation(scheme, &code).map(|_| Classification { evidence: Vec::new(), scheme, notation: code, source: ClassificationSource::ExactEdition })
        })
        .collect()
}

fn marc_subject(field: roxmltree::Node<'_, '_>) -> Option<SubjectHeading> {
    let components = field
        .children()
        .filter(|node| node.tag_name().name() == "subfield" && matches!(node.attribute("code"), Some("a" | "v" | "x" | "y" | "z")))
        .filter_map(|node| node.text())
        .map(compact_whitespace)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if components.is_empty() {
        return None;
    }
    let authority = subfield(field, "2").map(compact_whitespace).or_else(|| (field.attribute("ind2") == Some("0")).then(|| "lcsh".to_owned())).unwrap_or_else(|| "unspecified".to_owned());
    let authority = authority.to_ascii_lowercase();
    let authority_uri = subfield(field, "0").map(compact_whitespace).filter(|value| value.starts_with("https://id.loc.gov/") || value.starts_with("http://id.loc.gov/"));
    Some(SubjectHeading { source: PROVIDER_ID.to_owned(), authority, components, authority_uri })
}

fn subfield<'input>(field: roxmltree::Node<'input, 'input>, code: &str) -> Option<&'input str> {
    field.children().find(|node| node.tag_name().name() == "subfield" && node.attribute("code") == Some(code)).and_then(|node| node.text())
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['/', ':', ';', ',', '.']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARC_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<searchRetrieveResponse xmlns="http://www.loc.gov/zing/srw/">
  <numberOfRecords>1</numberOfRecords><records><record><recordData>
    <marc:record xmlns:marc="http://www.loc.gov/MARC21/slim">
      <marc:datafield tag="010"><marc:subfield code="a">  2019000001 </marc:subfield></marc:datafield>
      <marc:datafield tag="020"><marc:subfield code="a">978-0-306-40615-7 (hardcover)</marc:subfield></marc:datafield>
      <marc:datafield tag="245"><marc:subfield code="a">A test book :</marc:subfield><marc:subfield code="b">metadata.</marc:subfield></marc:datafield>
      <marc:datafield tag="050"><marc:subfield code="a">QA76.9</marc:subfield><marc:subfield code="b">.D3 2019</marc:subfield></marc:datafield>
      <marc:datafield tag="082"><marc:subfield code="a">005.1</marc:subfield></marc:datafield>
      <marc:datafield tag="650" ind2="0"><marc:subfield code="a">Computer software</marc:subfield><marc:subfield code="x">Development</marc:subfield><marc:subfield code="0">https://id.loc.gov/authorities/subjects/sh00000001</marc:subfield></marc:datafield>
      <marc:datafield tag="655"><marc:subfield code="a">Handbooks and manuals</marc:subfield><marc:subfield code="2">lcgft</marc:subfield></marc:datafield>
    </marc:record>
  </recordData></record></records>
</searchRetrieveResponse>"#;


    async fn lookup_one(client: &LibraryOfCongressClient, isbn: &str) -> Result<LibraryOfCongressLookup, MetadataError> {
        client.lookup_batch(&[Isbn13::parse(isbn).unwrap()]).await?.pop().ok_or_else(|| MetadataError("empty LC result".into()))
    }
    #[tokio::test]
    async fn cache_survives_reopen_and_http_errors_are_retryable() {
        use axum::{Router, routing::get};
        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, Router::new().route("/", get(|| async { MARC_RESPONSE }))).await.unwrap() });
        let path = dir.path().join("cache.sqlite");
        let client = LibraryOfCongressClient::with_endpoint(&endpoint).unwrap().with_cache(&path).unwrap();
        let found = lookup_one(&client, "9780306406157").await.unwrap();
        assert!(matches!(&found,LibraryOfCongressLookup::Matched{classifications,..} if classifications.len()==2 && classifications[0].evidence[0].method.starts_with("loc_api_isbn:")));
        assert_eq!(lookup_one(&client, "9781691706631").await.unwrap(), LibraryOfCongressLookup::NoMatch);
        task.abort();
        drop(client);
        let client = LibraryOfCongressClient::with_endpoint(&endpoint).unwrap().with_cache(&path).unwrap();
        assert_eq!(lookup_one(&client, "0306406152").await.unwrap(), found);
        assert_eq!(lookup_one(&client, "9781691706631").await.unwrap(), LibraryOfCongressLookup::NoMatch);
        assert!(lookup_one(&client, "9780374610326").await.is_err());
        assert_eq!(client.cached(Isbn13::parse("9780374610326").unwrap()).unwrap(), None);
        let c = rusqlite::Connection::open(path).unwrap();
        assert!(c.query_row("SELECT retry_at_ms FROM provider_retry WHERE provider='loc'", [], |r| r.get::<_, i64>(0)).unwrap() > epoch_ms());

        {
            let isbn = Isbn13::parse("978-0-306-40615-7").unwrap();
            assert_eq!(Isbn13::parse("0-306-40615-2"), Some(isbn));
            assert_eq!(Isbn13::parse("978-0-306-40615-7 (hardcover)"), Some(isbn));
            assert_eq!(isbn.to_string(), "9780306406157");
            assert_eq!(isbn.to_isbn10().as_deref(), Some("0306406152"));
            assert_eq!(Isbn13::parse("080442957X").unwrap().to_isbn10().as_deref(), Some("080442957X"));
            assert_eq!(Isbn13::parse("9791234567896").unwrap().to_isbn10(), None);
            assert_eq!(Isbn13::parse("978-0-306-40615-8"), None);
            assert_eq!(Isbn13::parse("030640615X"), None);
            assert_eq!(Isbn13::parse("97803064061X7"), None);
        }
    }


    #[test]
    fn rate_reservation_is_shared_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let first = LibraryOfCongressClient::new().unwrap().with_cache(&path).unwrap();
        let second = LibraryOfCongressClient::new().unwrap().with_cache(&path).unwrap();
        assert_eq!(first.reserve_request(10000).unwrap(), 0);
        assert_eq!(second.reserve_request(10001).unwrap(), 5999);
        assert_eq!(first.clone().reserve_request(15999).unwrap(), 1);
        drop(first);
        drop(second);
        let third = LibraryOfCongressClient::new().unwrap().with_cache(&path).unwrap();
        assert_eq!(third.reserve_request(16000).unwrap(), 0);
        assert_eq!(third.reserve_request(16000).unwrap(), 6000);
    }

    #[tokio::test]
    async fn batch_pages_map_exact_isbns_and_keep_other_books_separate() {
        use axum::{Router, extract::Query, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/",
                    get(|Query(q): Query<HashMap<String, String>>| async move {
                        assert!(q["query"].contains("0306406152"));
                        assert!(q["query"].contains(" or "));
                        let xml = MARC_RESPONSE.replace("<numberOfRecords>1", "<numberOfRecords>2");
                        if q["startRecord"] == "1" {
                            xml
                        } else {
                            assert_eq!(q["startRecord"], "2");
                            xml.replace("2019000001", "2019000002").replace("978-0-306-40615-7", "9781691706631").replace("QA76.9", "BF637")
                        }
                    }),
                ),
            )
            .await
            .unwrap()
        });
        let dir = tempfile::tempdir().unwrap();
        let client = LibraryOfCongressClient::with_endpoint(&endpoint).unwrap().with_cache(&dir.path().join("cache")).unwrap();
        let isbns = ["9780306406157", "9781691706631", "9780374610326"].map(|isbn| Isbn13::parse(isbn).unwrap());
        let results = client.lookup_batch(&isbns).await.unwrap();
        assert!(matches!(&results[0],LibraryOfCongressLookup::Matched{classifications,..} if classifications.iter().any(|c|c.notation.starts_with("QA76"))));
        assert!(matches!(&results[1],LibraryOfCongressLookup::Matched{classifications,..} if classifications.iter().any(|c|c.notation.starts_with("BF637"))));
        assert_eq!(results[2], LibraryOfCongressLookup::NoMatch);
        task.abort();
        assert_eq!(client.lookup_batch(&isbns).await.unwrap(), results);
    }

    #[tokio::test]
    async fn failed_later_page_caches_neither_positive_nor_negative_results() {
        use axum::{Router, extract::Query, http::StatusCode, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/",
                    get(|Query(q): Query<HashMap<String, String>>| async move {
                        if q["startRecord"] == "1" { (StatusCode::OK, MARC_RESPONSE.replace("<numberOfRecords>1", "<numberOfRecords>2")) } else { (StatusCode::INTERNAL_SERVER_ERROR, "error".into()) }
                    }),
                ),
            )
            .await
            .unwrap()
        });
        let dir = tempfile::tempdir().unwrap();
        let client = LibraryOfCongressClient::with_endpoint(&endpoint).unwrap().with_cache(&dir.path().join("cache")).unwrap();
        let isbns = ["9780306406157", "9781691706631"].map(|isbn| Isbn13::parse(isbn).unwrap());
        assert!(client.lookup_batch(&isbns).await.is_err());
        for isbn in isbns {
            assert_eq!(client.cached(isbn).unwrap(), None);
            assert!(!client.retry_ready(isbn).unwrap());
        }
        task.abort();
    }

}
