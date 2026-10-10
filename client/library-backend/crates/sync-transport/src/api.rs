use crate::{PerformanceTrace, SyncRequestError};
use reqwest::{header, RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sync_common::transport as wire;

pub(crate) async fn send<T: Serialize + ?Sized, R: DeserializeOwned>(request: RequestBuilder, value: &T) -> Result<R, SyncRequestError> {
    let mut trace = PerformanceTrace::new("binary_request", "encode_request_response_decode");
    send_with_trace(request, value, &mut trace).await
}

pub(crate) async fn send_with_trace<T: Serialize + ?Sized, R: DeserializeOwned>(request: RequestBuilder, value: &T, trace: &mut PerformanceTrace) -> Result<R, SyncRequestError> {
    let result = async {
        let response = request.header("x-bokheim-trace-id", trace.id()).header(header::ACCEPT, wire::MEDIA_TYPE).header(header::CONTENT_TYPE, wire::MEDIA_TYPE).body(wire::encode(value)?).send().await?.error_for_status()?;
        Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
    }
    .await;
    trace.finish(result.is_ok());
    result
}

#[derive(Clone, Debug)]
pub struct SyncCredentials {
    server_url: Url,
    access_token: String,
}

impl SyncCredentials {
    pub fn new(server_url: Url, access_token: impl Into<String>) -> Self {
        Self { server_url, access_token: access_token.into() }
    }

    pub fn server_url(&self) -> &Url {
        &self.server_url
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::Path,
        http::{HeaderMap, StatusCode},
        routing::post,
        Router,
    };

    #[tokio::test]
    async fn sync_requests_preserve_recovery_statuses_and_codec_errors() {
        let app = Router::new().route(
            "/:status",
            post(|Path(status): Path<u16>, headers: HeaderMap, body: Bytes| async move {
                assert_eq!(headers.get(header::ACCEPT).unwrap(), wire::MEDIA_TYPE);
                assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), wire::MEDIA_TYPE);
                assert!(headers.contains_key("x-bokheim-trace-id"));
                wire::decode::<()>(&body, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
                (StatusCode::from_u16(status).unwrap(), "service rejected request")
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();

        for status in [401, 403, 410, 404, 500, 200] {
            let error = send::<_, ()>(client.post(origin.join(&status.to_string()).unwrap()), &()).await.unwrap_err();
            match status {
                401 | 403 => assert!(matches!(error, SyncRequestError::AuthenticationRequired)),
                410 => assert!(matches!(error, SyncRequestError::CursorRejected)),
                200 => assert!(matches!(error, SyncRequestError::Wire(_))),
                _ => assert!(matches!(error, SyncRequestError::Http(error) if error.status().unwrap().as_u16() == status)),
            }
        }
        server.abort();
    }
}
