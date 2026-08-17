use crate::Session;
use futures_util::{SinkExt, StreamExt};
use sync_common::api::notifications::NotificationMessage;
use sync_common::transport as wire;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub struct NotificationStream {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

#[derive(Debug)]
pub struct NotificationError(String);

impl std::fmt::Display for NotificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for NotificationError {}

fn notification_url(session: &Session) -> Result<String, NotificationError> {
    let mut url = session.server_url().endpoint("api/notifications");
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        _ => return Err(NotificationError("notification endpoint requires HTTP or HTTPS".to_owned())),
    };
    url.set_scheme(scheme).map_err(|_| NotificationError("could not construct notification URL".to_owned()))?;
    Ok(url.into())
}

pub async fn connect_notifications(session: &Session) -> Result<NotificationStream, NotificationError> {
    let mut request = notification_url(session)?.into_client_request().map_err(|error| NotificationError(error.to_string()))?;
    let authorization = HeaderValue::from_str(&format!("Bearer {}", session.token())).map_err(|_| NotificationError("invalid notification authorization header".to_owned()))?;
    request.headers_mut().insert(AUTHORIZATION, authorization);
    let (socket, _) = tokio_tungstenite::connect_async(request).await.map_err(|error| NotificationError(error.to_string()))?;
    Ok(NotificationStream { socket })
}

impl NotificationStream {
    pub async fn next(&mut self) -> Result<Option<NotificationMessage>, NotificationError> {
        loop {
            let Some(message) = self.socket.next().await else { return Ok(None) };
            match message.map_err(|error| NotificationError(error.to_string()))? {
                Message::Binary(bytes) => {
                    let notification = wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| NotificationError(format!("invalid notification message: {error}")))?;
                    return Ok(Some(notification));
                }
                Message::Text(_) => return Err(NotificationError("notification server sent unsupported JSON text".to_owned())),
                Message::Ping(payload) => self.socket.send(Message::Pong(payload)).await.map_err(|error| NotificationError(error.to_string()))?,
                Message::Pong(_) | Message::Frame(_) => {}
                Message::Close(_) => return Ok(None),
            }
        }
    }

    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerUrl;
    use axum::extract::ws::Message as AxumMessage;
    use axum::extract::WebSocketUpgrade;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::get;
    use axum::Router;

    #[test]
    fn notification_urls_follow_the_authenticated_server_origin() {
        let token = "a".repeat(43);
        let remote = Session::try_new(ServerUrl::parse("https://api.example.com").unwrap(), "user-1".into(), "reader".into(), token.clone()).unwrap();
        let local = Session::try_new(ServerUrl::parse("http://127.0.0.1:8080").unwrap(), "user-1".into(), "reader".into(), token).unwrap();
        assert_eq!(notification_url(&remote).unwrap(), "wss://api.example.com/api/notifications");
        assert_eq!(notification_url(&local).unwrap(), "ws://127.0.0.1:8080/api/notifications");
    }

    #[tokio::test]
    async fn authenticated_notification_stream_decodes_server_hints() {
        async fn notifications(headers: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
            if headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()) != Some(&format!("Bearer {}", "a".repeat(43))) {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            upgrade.on_upgrade(|mut socket| async move {
                let message = NotificationMessage::LibraryChanged { library_id: test_support::fixture_library_id("library-1") };
                socket.send(AxumMessage::Binary(wire::encode(&message).unwrap().into())).await.unwrap();
            })
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, Router::new().route("/api/notifications", get(notifications))).await.unwrap() });
        let session = Session::try_new(ServerUrl::parse(&format!("http://{address}")).unwrap(), "user-1".into(), "reader".into(), "a".repeat(43)).unwrap();
        let mut stream = connect_notifications(&session).await.unwrap();
        assert_eq!(stream.next().await.unwrap(), Some(NotificationMessage::LibraryChanged { library_id: test_support::fixture_library_id("library-1") }));
        server.abort();
    }
}
