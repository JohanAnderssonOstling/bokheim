//! Shared bounded buffering for typed HTTP bodies. Status policy belongs to
//! each endpoint; raw book transfers retain their streaming paths.
use futures_util::StreamExt;
use sync_common::transport::WireError;

#[derive(Debug)]
pub(crate) enum BodyReadError {
    Http(reqwest::Error),
    TooLarge { limit: usize },
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use axum::{
        body::{Body, Bytes},
        extract::Path,
        response::{IntoResponse, Response},
        routing::get,
        Router,
    };
    use std::convert::Infallible;

    #[tokio::test]
    async fn bounded_reader_accepts_exact_limits_and_rejects_known_and_chunked_excess() {
        let app = Router::new().route(
            "/:length/:mode",
            get(|Path((length, mode)): Path<(usize, String)>| async move {
                if mode == "known" {
                    return vec![0xab; length].into_response();
                }
                let chunks = vec![Ok::<_, Infallible>(Bytes::from(vec![0xab; length / 2])), Ok(Bytes::from(vec![0xab; length - length / 2]))];
                Response::new(Body::from_stream(futures_util::stream::iter(chunks)))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        for mode in ["known", "chunked"] {
            let response = client.get(format!("{origin}/32/{mode}")).send().await.unwrap();
            assert_eq!(read(response, 32).await.unwrap(), vec![0xab; 32]);
            let response = client.get(format!("{origin}/33/{mode}")).send().await.unwrap();
            if mode == "chunked" {
                assert_eq!(response.content_length(), None);
            }
            assert!(matches!(read(response, 32).await, Err(BodyReadError::TooLarge { limit: 32 })));
        }
        server.abort();
    }
}

impl From<BodyReadError> for crate::SyncRequestError {
    fn from(error: BodyReadError) -> Self {
        match error {
            BodyReadError::Http(error) => error.into(),
            BodyReadError::TooLarge { limit } => WireError::TooLarge { limit }.into(),
        }
    }
}

impl From<BodyReadError> for crate::AssetResponseError {
    fn from(error: BodyReadError) -> Self {
        match error {
            BodyReadError::Http(error) => Self::Retryable(error),
            BodyReadError::TooLarge { limit } => Self::Wire(WireError::TooLarge { limit }),
        }
    }
}

pub(crate) async fn read(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, BodyReadError> {
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
