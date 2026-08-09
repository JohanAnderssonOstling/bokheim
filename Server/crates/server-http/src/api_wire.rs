pub(crate) fn decode_request<T: serde::de::DeserializeOwned>(headers: &axum::http::HeaderMap, body: &[u8]) -> Result<T, axum::http::StatusCode> {
    server_wire_http::decode_request(headers, body, bokheim_wire::MAX_DECODED_REQUEST_BYTES)
}

pub(crate) use server_wire_http::response;
