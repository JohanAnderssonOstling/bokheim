use axum::http::{header, HeaderMap, StatusCode};

// Browsers cannot attach Authorization headers to WebSocket handshakes.
// Accept the credential only on this endpoint, and never echo it as the
// negotiated subprotocol or include it in a URL.
pub(crate) fn websocket_bearer<'a>(path: &str, headers: &'a HeaderMap) -> Result<Option<&'a str>, StatusCode> {
    if path != "/api/notifications" { return Ok(None); }
    let protocols = headers.get_all(header::SEC_WEBSOCKET_PROTOCOL).iter()
        .map(|value| value.to_str().map_err(|_| StatusCode::BAD_REQUEST))
        .collect::<Result<Vec<_>, _>>()?;
    let protocols = protocols.iter().flat_map(|value| value.split(',').map(str::trim)).collect::<Vec<_>>();
    let mut tokens = protocols.iter().filter_map(|value| value.strip_prefix("bearer."));
    let token = tokens.next();
    if token.is_some() && (!protocols.contains(&"bokheim.notifications.v1") || token == Some("") || tokens.next().is_some()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(token)
}
