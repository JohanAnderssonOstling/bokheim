//! Related-edition discovery through LibraryThing's thingISBN service.

use crate::external::ExternalMetadataError;
use library_model::canonical_isbn;
use reqwest::{Client, StatusCode, Url};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LIBRARYTHING_ORIGIN: &str = "https://www.librarything.com/";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Rate-limited client for LibraryThing's thingISBN API. Authentication is
/// supplied by the composing application and is never included in errors.
#[derive(Clone)]
pub struct LibraryThingClient {
    http: Client,
    token: Arc<str>,
    next_request: Arc<Mutex<Instant>>,
}

impl LibraryThingClient {
    pub fn new(token: impl Into<String>) -> Result<Self, ExternalMetadataError> {
        let token = token.into();
        let token = token.trim();
        if token.is_empty() {
            return Err(ExternalMetadataError::new("LibraryThing token must not be empty"));
        }
        let http = Client::builder()
            .user_agent("Bokheim/1.0 (LibraryThing thingISBN classification fallback)")
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| ExternalMetadataError::new(format!("could not create LibraryThing client: {error}")))?;
        Ok(Self { http, token: Arc::from(token), next_request: Arc::new(Mutex::new(Instant::now())) })
    }

    pub async fn sibling_isbns(&self, isbn: &str) -> Result<Vec<String>, ExternalMetadataError> {
        let canonical = canonical_isbn13(isbn).ok_or_else(|| ExternalMetadataError::new("invalid ISBN supplied to LibraryThing"))?;
        self.wait_for_rate_limit().await;

        let mut url = Url::parse(LIBRARYTHING_ORIGIN).expect("LibraryThing origin is a valid URL");
        url.path_segments_mut().expect("LibraryThing origin can accept path segments").extend(["api", self.token.as_ref(), "thingISBN", canonical.as_str()]);
        let response = self.http.get(url).send().await.map_err(safe_request_error)?;
        let status = response.status();
        if status != StatusCode::OK {
            return Err(ExternalMetadataError::new(format!("LibraryThing returned HTTP {status}")));
        }
        if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
            return Err(ExternalMetadataError::new("LibraryThing response exceeded the size limit"));
        }
        let body = response.bytes().await.map_err(safe_request_error)?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(ExternalMetadataError::new("LibraryThing response exceeded the size limit"));
        }
        parse_thingisbn_response(&body)
    }

    async fn wait_for_rate_limit(&self) {
        let delay = {
            let mut next_request = self.next_request.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let now = Instant::now();
            let scheduled = (*next_request).max(now);
            *next_request = scheduled + Duration::from_secs(1);
            scheduled.saturating_duration_since(now)
        };
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
}

fn safe_request_error(error: reqwest::Error) -> ExternalMetadataError {
    let message = if error.is_timeout() {
        "LibraryThing request timed out"
    } else if error.is_connect() {
        "could not connect to LibraryThing"
    } else {
        "LibraryThing request failed"
    };
    ExternalMetadataError::new(message)
}

fn parse_thingisbn_response(body: &[u8]) -> Result<Vec<String>, ExternalMetadataError> {
    let text = std::str::from_utf8(body).map_err(|_| ExternalMetadataError::new("LibraryThing returned non-UTF-8 XML"))?;
    let document = roxmltree::Document::parse(text).map_err(|_| ExternalMetadataError::new("LibraryThing returned invalid XML"))?;
    Ok(document.descendants().filter(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case("isbn")).filter_map(|node| node.text()).filter_map(canonical_isbn13).collect::<BTreeSet<_>>().into_iter().collect())
}

fn canonical_isbn13(value: &str) -> Option<String> {
    let canonical = canonical_isbn(value)?;
    if canonical.len() == 13 {
        return Some(canonical);
    }
    let mut isbn13 = format!("978{}", &canonical[..9]);
    let sum = isbn13.bytes().enumerate().map(|(index, byte)| u32::from(byte - b'0') * if index % 2 == 0 { 1 } else { 3 }).sum::<u32>();
    isbn13.push(char::from(b'0' + ((10 - sum % 10) % 10) as u8));
    Some(isbn13)
}

#[cfg(test)]
mod tests {
    use super::parse_thingisbn_response;

    #[test]
    fn parses_and_canonicalizes_thingisbn_xml() {
        let xml = br#"<?xml version="1.0"?><idlist><isbn>0441172717</isbn><isbn>9780441172719</isbn><isbn>not-an-isbn</isbn></idlist>"#;
        assert_eq!(parse_thingisbn_response(xml).unwrap(), vec!["9780441172719"]);
    }
}
