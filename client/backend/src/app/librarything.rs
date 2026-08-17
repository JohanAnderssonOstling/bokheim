use library_model::canonical_isbn;
use reqwest::{Client, StatusCode, Url};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LIBRARYTHING_TOKEN_ENV: &str = "BOKHEIM_LIBRARYTHING_TOKEN";
const LIBRARYTHING_ORIGIN: &str = "https://www.librarything.com/";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Shared application client for LibraryThing's deliberately rate-limited
/// thingISBN API. The token is kept out of URLs included in errors and logs.
#[derive(Clone)]
pub(super) struct LibraryThingClient {
    http: Client,
    token: Arc<str>,
    next_request: Arc<Mutex<Instant>>,
}

impl LibraryThingClient {
    pub(super) fn from_environment() -> Result<Option<Self>, reqwest::Error> {
        let Some(token) = std::env::var(LIBRARYTHING_TOKEN_ENV).ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let http = Client::builder().user_agent("Bokheim/1.0 (LibraryThing thingISBN classification fallback)").timeout(Duration::from_secs(20)).build()?;
        Ok(Some(Self { http, token: token.into(), next_request: Arc::new(Mutex::new(Instant::now())) }))
    }

    pub(super) async fn sibling_isbns(&self, isbn: &str) -> Result<Vec<String>, String> {
        let canonical = canonical_isbn13(isbn).ok_or_else(|| "invalid ISBN supplied to LibraryThing".to_owned())?;
        self.wait_for_rate_limit().await;

        let mut url = Url::parse(LIBRARYTHING_ORIGIN).expect("LibraryThing origin is a valid URL");
        url.path_segments_mut().expect("LibraryThing origin can accept path segments").extend(["api", self.token.as_ref(), "thingISBN", canonical.as_str()]);
        let response = self.http.get(url).send().await.map_err(safe_request_error)?;
        let status = response.status();
        if status != StatusCode::OK {
            return Err(format!("LibraryThing returned HTTP {status}"));
        }
        if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
            return Err("LibraryThing response exceeded the size limit".to_owned());
        }
        let body = response.bytes().await.map_err(safe_request_error)?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err("LibraryThing response exceeded the size limit".to_owned());
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

fn safe_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "LibraryThing request timed out".to_owned()
    } else if error.is_connect() {
        "could not connect to LibraryThing".to_owned()
    } else {
        "LibraryThing request failed".to_owned()
    }
}

fn parse_thingisbn_response(body: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(body).map_err(|_| "LibraryThing returned non-UTF-8 XML".to_owned())?;
    let document = roxmltree::Document::parse(text).map_err(|_| "LibraryThing returned invalid XML".to_owned())?;
    let isbns = document.descendants().filter(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case("isbn")).filter_map(|node| node.text()).filter_map(canonical_isbn13).collect::<BTreeSet<_>>().into_iter().collect();
    Ok(isbns)
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
