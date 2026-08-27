use crate::session::AuthError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sync_common::transport as wire;

pub(crate) fn headers(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.header(reqwest::header::ACCEPT, wire::MEDIA_TYPE)
}

pub(crate) fn body<T: Serialize + ?Sized>(request: reqwest::RequestBuilder, value: &T) -> Result<reqwest::RequestBuilder, AuthError> {
    let encoded = wire::encode(value).map_err(|error| AuthError::InvalidData(error.to_string()))?;
    Ok(headers(request).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(encoded))
}

pub(crate) async fn send<T: Serialize + ?Sized, R: DeserializeOwned>(request: reqwest::RequestBuilder, value: &T) -> Result<R, AuthError> {
    decode(body(request, value)?.send().await?).await
}

pub(crate) async fn send_status<T: Serialize + ?Sized>(request: reqwest::RequestBuilder, value: &T) -> Result<(), AuthError> {
    ensure_ok(body(request, value)?.send().await?).await?;
    Ok(())
}

pub(crate) async fn receive<R: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<R, AuthError> {
    decode(headers(request).send().await?).await
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, AuthError> {
    let response = ensure_ok(response).await?;
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_owned();
    if response.headers().get(reqwest::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).map(str::trim).is_some_and(|value| !value.is_empty() && !value.eq_ignore_ascii_case("identity")) {
        return Err(AuthError::InvalidData("compressed API responses are not supported".to_owned()));
    }
    let body = response.bytes().await?;
    if !wire::is_current_media_type(&content_type) {
        return Err(AuthError::InvalidData(format!("unsupported API response content type: {content_type}")));
    }
    wire::decode(&body, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| AuthError::InvalidData(error.to_string()))
}

pub(crate) async fn ensure_ok(response: reqwest::Response) -> Result<reqwest::Response, AuthError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(AuthError::BadStatus(status, body))
}
