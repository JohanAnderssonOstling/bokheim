use crate::{PerformanceTrace, SyncRequestError};
use reqwest::{header, RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sync_common::transport as wire;

pub(crate) async fn send<T: Serialize + ?Sized, R: DeserializeOwned>(request: RequestBuilder, value: &T) -> Result<R, SyncRequestError> {
    let mut trace = PerformanceTrace::new("binary_request", "encode_request_response_decode");
    send_decoded(request, value, &mut trace, wire::decode).await
}

pub(crate) fn encoded_request<T: Serialize + ?Sized>(request: RequestBuilder, value: &T) -> Result<RequestBuilder, wire::WireError> {
    Ok(request.header(header::ACCEPT, wire::MEDIA_TYPE).header(header::CONTENT_TYPE, wire::MEDIA_TYPE).body(wire::encode(value)?))
}

pub(crate) async fn send_with_trace_byte_fields<T: Serialize + ?Sized, R: DeserializeOwned>(request: RequestBuilder, value: &T, trace: &mut PerformanceTrace) -> Result<R, SyncRequestError> {
    send_decoded(request, value, trace, wire::decode_byte_fields).await
}

async fn send_decoded<T: Serialize + ?Sized, R>(request: RequestBuilder, value: &T, trace: &mut PerformanceTrace, decode: fn(&[u8], usize) -> Result<R, wire::WireError>) -> Result<R, SyncRequestError> {
    let result = async {
        let response = encoded_request(request.header("x-bokheim-trace-id", trace.id()), value)?.send().await?;
        let status = response.status();
        if matches!(status, reqwest::StatusCode::TOO_MANY_REQUESTS | reqwest::StatusCode::SERVICE_UNAVAILABLE) {
            if let Some(delay) = response.headers().get(header::RETRY_AFTER).and_then(|value| value.to_str().ok()).and_then(crate::state::retry_after) {
                return Err(SyncRequestError::RetryAfter { status, delay });
            }
        }
        let response = response.error_for_status()?;
        let bytes = crate::body::read(response, wire::MAX_DECODED_RESPONSE_BYTES).await.map_err(SyncRequestError::from)?;
        Ok(decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES)?)
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

    #[tokio::test]
    async fn sync_exchange_transports_compact_payloads_and_book_hashes() {
        use crate::exchange_changes;
        use sync_common::{ConflictRank, ContentHash, LibraryId, LibraryRevision, OpaquePublication, OpaqueStateEntry, PullStateResponse, PushMutationsResponse, ReplicaId, ReplicaSeq, StateAddress, StateDelivery, SyncCursor, SyncExchangeRequest, SyncExchangeResponse, VersionKey, Versioned};

        let app = Router::new().route("/api/sync/exchange", post(|headers: HeaderMap, body: Bytes| async move {
            assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), wire::MEDIA_TYPE);
            let request: SyncExchangeRequest = wire::decode_byte_fields(&body, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
            assert_eq!(request.mutations.len(), 1);
            let publication = request.mutations.into_iter().next().unwrap();
            assert_eq!(publication.state.value.bytes, (0..128 * 1024).map(|byte| byte as u8).collect::<Vec<_>>());
            assert_eq!(publication.state.value.address.entity_key.book(), publication.state.value.book);
            wire::encode(&SyncExchangeResponse {
                push: PushMutationsResponse { rejected: Vec::new() },
                pull: PullStateResponse {
                    mutations: vec![StateDelivery { state: publication.state, revision: LibraryRevision::new(1).unwrap() }],
                    next_cursor: SyncCursor { generation: Some(LibraryId::from_u128(2)), field_revision: Some(LibraryRevision::new(1).unwrap()), ..SyncCursor::default() },
                    has_more: false,
                },
            }).unwrap()
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let credentials = SyncCredentials::new(Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap(), "test-token");
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let book = ContentHash::from_bytes([0xab; 32]);
        let state = Versioned {
            value: OpaqueStateEntry { address: StateAddress { kind: "reading_position".into(), entity_key: book.into(), entity_subkey: "epub".into() }, book: Some(book), bytes: (0..128 * 1024).map(|byte| byte as u8).collect() },
            version: VersionKey::new(1, ConflictRank::Update, ReplicaId::from_u128(1), ReplicaSeq::new(1).unwrap()),
        };
        let response = exchange_changes(&reqwest::Client::new(), &credentials, LibraryId::from_u128(3), ReplicaId::from_u128(1), vec![OpaquePublication { publisher_seq: ReplicaSeq::new(1).unwrap(), state: state.clone() }], SyncCursor::default()).await.unwrap();
        assert_eq!(response.pull.mutations[0].state, state);
        assert_eq!(response.pull.next_cursor.generation, Some(LibraryId::from_u128(2)));
        server.abort();
    }
}
