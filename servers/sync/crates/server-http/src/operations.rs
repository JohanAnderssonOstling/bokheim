use reqwest::{redirect::Policy, Client, Url};
use serde::Serialize;
use serde_json::Value;
use std::net::IpAddr;
use std::time::{Duration, Instant};

const DEFAULT_CATALOG_URL: &str = "http://127.0.0.1:8092/";
const DEFAULT_METADATA_URL: &str = "http://127.0.0.1:8091/";
const RESPONSE_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub struct OperationalServices {
    client: Client,
    catalog_url: Option<Url>,
    metadata_url: Option<Url>,
}

#[derive(Serialize)]
pub(crate) struct OperationalSnapshot {
    pub(crate) catalogue: RemoteService,
    pub(crate) metadata: RemoteService,
}

#[derive(Serialize)]
pub(crate) struct RemoteService {
    available: bool,
    latency_ms: u64,
    data: Option<Value>,
}

impl Default for OperationalServices {
    fn default() -> Self {
        Self::from_values(|_| None).expect("default local operational service URLs are valid")
    }
}

impl OperationalServices {
    pub fn from_env() -> Result<Self, String> {
        Self::from_values(|name| std::env::var(name).ok())
    }

    fn from_values(mut value: impl FnMut(&str) -> Option<String>) -> Result<Self, String> {
        let catalog_url = parse_url("BOKHEIM_ADMIN_CATALOG_URL", value("BOKHEIM_ADMIN_CATALOG_URL"), DEFAULT_CATALOG_URL)?;
        let metadata_url = parse_url("BOKHEIM_ADMIN_METADATA_URL", value("BOKHEIM_ADMIN_METADATA_URL"), DEFAULT_METADATA_URL)?;
        let client = Client::builder().redirect(Policy::none()).timeout(Duration::from_secs(5)).build().map_err(|error| format!("could not build operations HTTP client: {error}"))?;
        Ok(Self { client, catalog_url, metadata_url })
    }

    pub(crate) async fn snapshot(&self) -> OperationalSnapshot {
        let (catalogue, metadata) = tokio::join!(self.fetch(self.catalog_url.as_ref(), "internal/operations"), self.fetch(self.metadata_url.as_ref(), "internal/operations"));
        OperationalSnapshot { catalogue, metadata }
    }

    async fn fetch(&self, base: Option<&Url>, path: &str) -> RemoteService {
        let started = Instant::now();
        let result = async {
            let base = base.ok_or("service disabled")?;
            let url = base.join(path).map_err(|_| "invalid service endpoint")?;
            let response = self.client.get(url).header(reqwest::header::ACCEPT, "application/json").send().await.map_err(|_| "service request failed")?;
            if !response.status().is_success() {
                return Err("service returned an error");
            }
            if response.content_length().is_some_and(|length| length > RESPONSE_LIMIT as u64) {
                return Err("service response is too large");
            }
            let bytes = response.bytes().await.map_err(|_| "service response failed")?;
            if bytes.len() > RESPONSE_LIMIT {
                return Err("service response is too large");
            }
            serde_json::from_slice(&bytes).map_err(|_| "service returned invalid JSON")
        }
        .await;
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok(data) => RemoteService { available: true, latency_ms, data: Some(data) },
            Err(reason) => {
                tracing::debug!(service_path = path, reason, "administrator service status collection failed");
                RemoteService { available: false, latency_ms, data: None }
            }
        }
    }
}

fn parse_url(name: &str, configured: Option<String>, default: &str) -> Result<Option<Url>, String> {
    let value = configured.unwrap_or_else(|| default.to_owned());
    if value.is_empty() {
        return Ok(None);
    }
    let mut url = Url::parse(&value).map_err(|error| format!("invalid {name}: {error}"))?;
    let address = url.host_str().and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok()).ok_or_else(|| format!("{name} must use a literal loopback IP address"))?;
    if url.scheme() != "http" || !address.is_loopback() || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
        return Err(format!("{name} must be an HTTP loopback base URL without credentials, query, or fragment"));
    }
    url.set_path("/");
    Ok(Some(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_targets_are_loopback_only_and_may_be_disabled() {
        assert!(parse_url("test", Some("http://127.0.0.1:8090".to_owned()), DEFAULT_CATALOG_URL).unwrap().is_some());
        assert!(parse_url("test", Some("http://[::1]:8090".to_owned()), DEFAULT_CATALOG_URL).unwrap().is_some());
        assert!(parse_url("test", Some(String::new()), DEFAULT_CATALOG_URL).unwrap().is_none());
        assert!(parse_url("test", Some("http://example.com".to_owned()), DEFAULT_CATALOG_URL).is_err());
        assert!(parse_url("test", Some("https://127.0.0.1".to_owned()), DEFAULT_CATALOG_URL).is_err());
        assert!(parse_url("test", Some("http://127.0.0.1@203.0.113.1".to_owned()), DEFAULT_CATALOG_URL).is_err());
    }
}
