//! HTTP client construction and bounded body collection.
//! Callers own status, retry, timeout, and decoding policy.

use futures_util::StreamExt;
#[cfg(all(feature = "blocking", not(target_arch = "wasm32")))]
use reqwest::blocking::{Client as BlockingClient, ClientBuilder as BlockingClientBuilder};
use reqwest::{Client, ClientBuilder, Error, IntoUrl, Response};
#[cfg(not(target_arch = "wasm32"))]
use rustls::crypto::{ring, CryptoProvider};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Once;

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
    Client::builder()
}

pub fn client() -> Client {
    initialize_tls();
    Client::new()
}

pub async fn get(url: impl IntoUrl) -> Result<Response, Error> {
    client().get(url).send().await
}

#[cfg(all(feature = "blocking", not(target_arch = "wasm32")))]
pub fn blocking_client_builder() -> BlockingClientBuilder {
    initialize_tls();
    BlockingClient::builder()
}

#[derive(Debug)]
pub enum BodyReadError {
    Http(Error),
    TooLarge { limit: usize },
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
