use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wire::{self as wire, WireError};

fn content_type(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok())
}

fn accepts_wire(headers: &HeaderMap) -> bool {
    headers.get_all(header::ACCEPT).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(|candidate| {
        let disabled = candidate
            .split(';')
            .skip(1)
            .any(|parameter| parameter.trim().split_once('=').filter(|(name, _)| name.trim().eq_ignore_ascii_case("q")).and_then(|(_, quality)| quality.trim().parse::<f32>().ok()).is_some_and(|quality| quality <= 0.0));
        wire::is_current_media_type(candidate) && !disabled
    })
}

fn content_encoding_supported(headers: &HeaderMap) -> bool {
    match headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).map(str::trim) {
        None | Some("") => true,
        Some(value) if value.eq_ignore_ascii_case("identity") => true,
        Some(_) => false,
    }
}

fn wire_decode_status(error: WireError) -> StatusCode {
    match error {
        WireError::UnsupportedVersion(_) => StatusCode::UPGRADE_REQUIRED,
        WireError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        WireError::Encode(_) => StatusCode::INTERNAL_SERVER_ERROR,
        WireError::Decode(_) | WireError::InvalidHeader => StatusCode::BAD_REQUEST,
    }
}

pub fn decode_request<T: DeserializeOwned>(headers: &HeaderMap, body: &[u8], decoded_limit: usize) -> Result<T, StatusCode> {
    decode_request_with(headers, body, decoded_limit, wire::decode)
}

pub fn decode_request_byte_fields<T: DeserializeOwned>(headers: &HeaderMap, body: &[u8], decoded_limit: usize) -> Result<T, StatusCode> {
    decode_request_with(headers, body, decoded_limit, wire::decode_byte_fields)
}

fn decode_request_with<T>(headers: &HeaderMap, body: &[u8], decoded_limit: usize, decode: fn(&[u8], usize) -> Result<T, WireError>) -> Result<T, StatusCode> {
    if !content_encoding_supported(headers) {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    match content_type(headers) {
        Some(value) if wire::is_current_media_type(value) => decode(body, decoded_limit).map_err(wire_decode_status),
        Some(value) if value.split(';').next().is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case(wire::MEDIA_TYPE_BASE)) => Err(StatusCode::UPGRADE_REQUIRED),
        _ => Err(StatusCode::UNSUPPORTED_MEDIA_TYPE),
    }
}

pub fn decode_optional_request<T: DeserializeOwned>(headers: &HeaderMap, body: &[u8], decoded_limit: usize) -> Result<Option<T>, StatusCode> {
    if body.is_empty() {
        Ok(None)
    } else {
        decode_request(headers, body, decoded_limit).map(Some)
    }
}

pub fn response<T: Serialize>(request_headers: &HeaderMap, value: T) -> Result<Response, StatusCode> {
    if !accepts_wire(request_headers) {
        return Err(StatusCode::NOT_ACCEPTABLE);
    }

    let encoded = wire::encode(&value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut response = encoded.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(wire::MEDIA_TYPE));
    response.headers_mut().insert(header::VARY, HeaderValue::from_static("Accept"));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Payload {
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    }

    #[test]
    fn compact_request_decode_keeps_http_contract_checks() {
        let payload = Payload { bytes: (0..=255).collect() };
        let encoded = wire::encode(&payload).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(wire::MEDIA_TYPE));
        assert_eq!(decode_request_byte_fields::<Payload>(&headers, &encoded, 4096).unwrap(), payload);
        assert_eq!(decode_request_byte_fields::<Payload>(&headers, &encoded, 1), Err(StatusCode::PAYLOAD_TOO_LARGE));
        let mut old_body = encoded.clone();
        old_body[1] = 28;
        assert_eq!(decode_request_byte_fields::<Payload>(&headers, &old_body, 4096), Err(StatusCode::UPGRADE_REQUIRED));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/vnd.bokheim+protobuf; version=28"));
        assert_eq!(decode_request_byte_fields::<Payload>(&headers, &encoded, 4096), Err(StatusCode::UPGRADE_REQUIRED));
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(wire::MEDIA_TYPE));
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert_eq!(decode_request_byte_fields::<Payload>(&headers, &encoded, 4096), Err(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    }
}
