//! HTTP client construction and bounded body collection.
//! Callers own status, retry, timeout, and decoding policy.

mod runtime;
pub use runtime::{run, with_runtime};

use futures_util::StreamExt;
#[cfg(all(feature = "blocking", not(target_arch = "wasm32")))]
use reqwest::blocking::{Client as BlockingClient, ClientBuilder as BlockingClientBuilder};
use reqwest::{Client, ClientBuilder, Error, IntoUrl, Response};
use std::{error::Error as StdError, fmt};
#[cfg(not(target_arch = "wasm32"))]
use rustls::crypto::{ring, CryptoProvider};
#[cfg(all(feature = "bundled-roots", not(target_arch = "wasm32")))]
use rustls::{ClientConfig, RootCertStore};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Once, OnceLock};

/// Install the native provider before Reqwest's provider-free TLS path is used.
pub fn initialize_tls() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static INITIALIZE: Once = Once::new();
        INITIALIZE.call_once(|| {
            // A host may have already installed the process-wide provider.
            if CryptoProvider::get_default().is_none() {
                let _ = ring::default_provider().install_default();
            }
        });
    }
}

pub fn client_builder() -> ClientBuilder {
    initialize_tls();
    let builder = Client::builder();
    #[cfg(all(feature = "bundled-roots", not(target_arch = "wasm32")))]
    let builder = builder.tls_backend_preconfigured(bundled_tls_config());
    builder
}

#[cfg(all(feature = "bundled-roots", not(target_arch = "wasm32")))]
fn bundled_tls_config() -> ClientConfig {
    // Devices such as Kobo have no system CA store. Configure the verifier
    // explicitly so Reqwest does not try to initialize the platform verifier.
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

pub fn try_client() -> Result<Client, Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Default-policy users share the verifier, trust roots and connection pool.
        static CLIENT: OnceLock<Client> = OnceLock::new();
        if let Some(client) = CLIENT.get() {
            return Ok(client.clone());
        }
        let client = client_builder().build()?;
        Ok(CLIENT.get_or_init(|| client).clone())
    }
    #[cfg(target_arch = "wasm32")]
    client_builder().build()
}

pub fn client() -> Client {
    try_client().expect("failed to initialize the default HTTP client")
}

pub async fn get(url: impl IntoUrl) -> Result<Response, Error> {
    client().get(url).send().await
}

#[cfg(all(feature = "blocking", not(target_arch = "wasm32")))]
pub fn blocking_client_builder() -> BlockingClientBuilder {
    initialize_tls();
    let builder = BlockingClient::builder();
    #[cfg(feature = "bundled-roots")]
    let builder = builder.tls_backend_preconfigured(bundled_tls_config());
    builder
}

#[derive(Debug)]
pub enum BodyReadError {
    Http(Error),
    TooLarge { limit: usize },
}

impl fmt::Display for BodyReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => error.fmt(formatter),
            Self::TooLarge { limit } => write!(formatter, "response body exceeds {limit} bytes"),
        }
    }
}

impl StdError for BodyReadError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::TooLarge { .. } => None,
        }
    }
}

pub async fn read(response: Response, limit: usize) -> Result<Vec<u8>, BodyReadError> {
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(BodyReadError::TooLarge { limit });
    }
    let mut bytes = Vec::new();
    let stream = response.bytes_stream();
    futures_util::pin_mut!(stream);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(BodyReadError::Http)?;
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(BodyReadError::TooLarge { limit });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
