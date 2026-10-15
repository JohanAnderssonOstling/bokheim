//! Fetching feed pages, straight from the device: no Bokheim server in
//! between.
use crate::{opensearch_template, parse, Feed, Search};
use library_model::OpdsCredentials;
use std::sync::LazyLock;
use url::{Origin, Url};

/// One client for every feed request, so pages, covers and downloads from a
/// feed reuse its connections rather than each opening its own. A server
/// that stops answering fails the request instead of holding the page;
/// the limits are on connecting and on each wait for data, not the whole
/// response, so a large book still downloads.
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    let builder = reqwest::Client::builder();
    #[cfg(not(target_arch = "wasm32"))]
    let builder = builder.connect_timeout(std::time::Duration::from_secs(10)).read_timeout(std::time::Duration::from_secs(30));
    builder.build().unwrap_or_default()
});

/// How a feed is reached: its credentials, sent only to the feed's own
/// origin, so a link to another host never receives them.
pub(crate) struct Access<'a> {
    root: &'a Url,
    origin: Origin,
    credentials: Option<&'a OpdsCredentials>,
}

impl<'a> Access<'a> {
    pub(crate) fn new(root: &'a Url, credentials: Option<&'a OpdsCredentials>) -> Self {
        Self { root, origin: root.origin(), credentials }
    }
}

pub(crate) enum Failure {
    /// The feed asks for credentials this device does not have, or refused
    /// the ones it has.
    SignIn,
    Other(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::Other(message.to_owned())
    }
}

/// The page's bytes, its content type, and the URL it was finally served
/// from, which relative links resolve against.
async fn get(url: &Url, access: &Access<'_>) -> Result<(Vec<u8>, Option<String>, Url), Failure> {
    let mut request = CLIENT.get(url.clone());
    if let Some(credentials) = access.credentials.filter(|_| url.origin() == access.origin) {
        request = request.basic_auth(&credentials.username, Some(&credentials.password));
    }
    let response = request.send().await.map_err(|error| format!("could not load {url}: {error}"))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(Failure::SignIn);
    }
    if response.headers().get("cf-mitigated").is_some_and(|value| value == "challenge") {
        const ERROR: &str = "Browser verification could not be completed for this catalog. Please try another catalog.";
        #[cfg(not(target_arch = "wasm32"))]
        {
            use base64::{engine::general_purpose::STANDARD, Engine};
            let served = response.url();
            let authorization = access.credentials.filter(|_| served.origin() == access.origin).map(|credentials| format!("Basic {}", STANDARD.encode(format!("{}:{}", credentials.username, credentials.password))));
            let (status, bytes, content_type, served) = crate::browser::get(served, authorization, access.root).await.map_err(|_| Failure::Other(ERROR.into()))?;
            if status == 401 {
                return Err(Failure::SignIn);
            }
            if !(200..300).contains(&status) {
                return Err(ERROR.into());
            }
            return Ok((bytes, content_type, served));
        }
        #[cfg(target_arch = "wasm32")]
        return Err(ERROR.into());
    }
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err("This catalog is blocking access from Bokheim. Please try another catalog.".into());
    }
    let response = response.error_for_status().map_err(|error| format!("could not load {url}: {error}"))?;
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(str::to_owned);
    let served = response.url().clone();
    let bytes = response.bytes().await.map_err(|error| format!("could not load {url}: {error}"))?;
    Ok((bytes.to_vec(), content_type, served))
}

/// A file the feed links to: a book or a cover.
pub(crate) async fn bytes(url: &Url, access: &Access<'_>) -> Result<Vec<u8>, Failure> {
    Ok(client_platform_runtime::transfers::retry(|| get(url, access)).await?.0)
}

pub(crate) async fn feed(url: &Url, access: &Access<'_>) -> Result<Feed, Failure> {
    let (bytes, content_type, served) = get(url, access).await?;
    Ok(parse(&bytes, content_type.as_deref(), &served)?)
}

/// The results page for `query` on a feed that offers search.
pub(crate) async fn search(feed: &Feed, query: &str, access: &Access<'_>) -> Result<Url, Failure> {
    let template = match &feed.search {
        Some(Search::Template(template)) => template.clone(),
        Some(Search::Description(url)) => {
            let (bytes, _, _) = get(url, access).await?;
            opensearch_template(std::str::from_utf8(&bytes).map_err(|_| "the search description is not UTF-8")?)?
        }
        None => return Err("this feed cannot be searched".into()),
    };
    Ok(Search::url(&template, query)?)
}
