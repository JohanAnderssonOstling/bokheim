//! Fetching feed pages, straight from the device: no Bokheim server in
//! between.
use crate::{opensearch_template, parse, Feed, Search};
use library_model::OpdsCredentials;
use url::{Origin, Url};

/// How a feed is reached: its credentials, sent only to the feed's own
/// origin, so a link to another host never receives them.
pub(crate) struct Access<'a> {
    origin: Origin,
    credentials: Option<&'a OpdsCredentials>,
}

impl<'a> Access<'a> {
    pub(crate) fn new(root: &Url, credentials: Option<&'a OpdsCredentials>) -> Self {
        Self { origin: root.origin(), credentials }
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
    let mut request = reqwest::Client::new().get(url.clone());
    if let Some(credentials) = access.credentials.filter(|_| url.origin() == access.origin) {
        request = request.basic_auth(&credentials.username, Some(&credentials.password));
    }
    let response = request.send().await.map_err(|error| format!("could not load {url}: {error}"))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(Failure::SignIn);
    }
    let response = response.error_for_status().map_err(|error| format!("could not load {url}: {error}"))?;
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(str::to_owned);
    let served = response.url().clone();
    let bytes = response.bytes().await.map_err(|error| format!("could not load {url}: {error}"))?;
    Ok((bytes.to_vec(), content_type, served))
}

/// A file the feed links to: a book or a cover.
pub(crate) async fn bytes(url: &Url, access: &Access<'_>) -> Result<Vec<u8>, Failure> {
    Ok(get(url, access).await?.0)
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
