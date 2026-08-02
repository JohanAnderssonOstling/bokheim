use crate::extract::{auth_error, AuthUser};
use crate::state::AppState;
use crate::ClientAddressSource;
use axum::extract::{ConnectInfo, Json, State};
use axum::http::{HeaderMap, HeaderName, StatusCode};
use std::net::{IpAddr, SocketAddr};
use sync_common::api::auth::{AuthResponse, LoginRequest, MeResponse, SignupRequest};

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

fn attempt_key(route: &str, address: IpAddr, username: &str) -> String {
    format!("{route}:{address}:{}", username.trim().to_lowercase())
}

fn client_address(source: ClientAddressSource, headers: &HeaderMap, remote: Option<ConnectInfo<SocketAddr>>) -> Result<IpAddr, StatusCode> {
    match source {
        ClientAddressSource::DirectPeer => remote.map(|ConnectInfo(address)| address.ip()).ok_or(StatusCode::INTERNAL_SERVER_ERROR),
        ClientAddressSource::TrustedForwardedFor => {
            let mut values = headers.get_all(X_FORWARDED_FOR).iter();
            let value = values.next().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
            if values.next().is_some() {
                return Err(StatusCode::BAD_REQUEST);
            }
            let value = value.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
            if value.contains(',') {
                return Err(StatusCode::BAD_REQUEST);
            }
            value.trim().parse().map_err(|_| StatusCode::BAD_REQUEST)
        }
    }
}

pub(super) async fn post_login(State(state): State<AppState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, Json(request): Json<LoginRequest>) -> Result<Json<AuthResponse>, StatusCode> {
    let key = attempt_key("login", client_address(state.client_address_source, &headers, remote)?, &request.username);
    if !state.auth_rate_limiter.begin(key.clone()) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let issued = state.auth.login(&request.username, &request.password).await.map_err(auth_error)?;
    state.auth_rate_limiter.succeeded(&key);
    Ok(Json(AuthResponse { token: issued.token, expires_at: issued.expires_at.timestamp(), user_id: issued.user.user_id, username: issued.user.username }))
}

pub(super) async fn post_logout(State(state): State<AppState>, user: AuthUser) -> Result<StatusCode, StatusCode> {
    state.auth.logout(&user.bearer_token).await.map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn post_signup(State(state): State<AppState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, Json(request): Json<SignupRequest>) -> Result<Json<AuthResponse>, StatusCode> {
    let key = attempt_key("signup", client_address(state.client_address_source, &headers, remote)?, &request.username);
    if !state.auth_rate_limiter.begin(key.clone()) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let issued = state.auth.signup(&request.invite, &request.username, &request.password).await.map_err(auth_error)?;
    state.auth_rate_limiter.succeeded(&key);
    Ok(Json(AuthResponse { token: issued.token, expires_at: issued.expires_at.timestamp(), user_id: issued.user.user_id, username: issued.user.username }))
}

pub(super) async fn get_me(user: AuthUser) -> Json<MeResponse> {
    Json(MeResponse { user_id: user.user_id, username: user.username })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn direct_peer_addresses_ignore_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("203.0.113.9"));
        let peer = "127.0.0.1:1234".parse().unwrap();
        assert_eq!(client_address(ClientAddressSource::DirectPeer, &headers, Some(ConnectInfo(peer))).unwrap(), "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_proxy_addresses_require_one_canonical_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("203.0.113.9"));
        assert_eq!(client_address(ClientAddressSource::TrustedForwardedFor, &headers, None).unwrap(), "203.0.113.9".parse::<IpAddr>().unwrap());

        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("203.0.113.9, 127.0.0.1"));
        assert_eq!(client_address(ClientAddressSource::TrustedForwardedFor, &headers, None), Err(StatusCode::BAD_REQUEST));
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("not-an-ip"));
        assert_eq!(client_address(ClientAddressSource::TrustedForwardedFor, &headers, None), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn trusted_proxy_addresses_fail_closed_when_missing() {
        assert_eq!(client_address(ClientAddressSource::TrustedForwardedFor, &HeaderMap::new(), None), Err(StatusCode::INTERNAL_SERVER_ERROR));
    }
}
