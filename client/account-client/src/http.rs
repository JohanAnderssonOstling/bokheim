use crate::{AuthError, Session, Url};
use reqwest::{header, Client, Method, RequestBuilder, Response};
use serde::{de::DeserializeOwned, Serialize};
use sync_common::transport as wire;

pub(crate) fn request(server_url: &Url, method: Method, endpoint: &str) -> Result<RequestBuilder, AuthError> {
    Ok(Client::builder().build()?.request(method, server_url.join(endpoint)?).header(header::ACCEPT, wire::MEDIA_TYPE))
}

pub(crate) fn authenticated_request(session: &Session, method: Method, endpoint: &str) -> Result<RequestBuilder, AuthError> {
    Ok(request(session.server_url(), method, endpoint)?.bearer_auth(session.token()))
}

pub(crate) fn with_body(request: RequestBuilder, body: &impl Serialize) -> Result<RequestBuilder, AuthError> {
    Ok(request.header(header::CONTENT_TYPE, wire::MEDIA_TYPE).body(wire::encode(body)?))
}

pub(crate) async fn send(request: RequestBuilder) -> Result<Response, AuthError> {
    Ok(request.send().await?.error_for_status()?)
}

pub(crate) async fn post_empty(server_url: &Url, endpoint: &str, body: &impl Serialize) -> Result<(), AuthError> {
    send(with_body(request(server_url, Method::POST, endpoint)?, body)?).await.map(|_| ())
}

pub(crate) async fn decode_response<T: DeserializeOwned>(request: RequestBuilder) -> Result<T, AuthError> {
    let response = send(request).await?;
    Ok(wire::decode(&response.bytes().await?, wire::MAX_DECODED_RESPONSE_BYTES)?)
}
