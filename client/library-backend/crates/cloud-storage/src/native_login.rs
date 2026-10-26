//! Desktop system-browser login using an ephemeral loopback callback.

use crate::{CloudError, connections::CloudConnections, oauth::{GoogleAccount, PendingGoogleLogin}};
use reqwest::Url;
use std::time::Duration;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

pub struct NativeGoogleLogin {
    listener: TcpListener,
    connections: CloudConnections,
    pending: PendingGoogleLogin,
    redirect: Url,
}

impl NativeGoogleLogin {
    pub async fn begin(connections: CloudConnections) -> Result<Self, CloudError> {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| CloudError::InvalidRequest("Could not open the local Google sign-in callback".into()))?;
        let address = listener.local_addr().map_err(|_| CloudError::InvalidRequest("Could not resolve the local Google sign-in callback".into()))?;
        let redirect = Url::parse(&format!("http://{address}/oauth/google/callback")).expect("numeric loopback callback");
        let pending = connections.begin_google_login(redirect.clone())?;
        Ok(Self { listener, connections, pending, redirect })
    }

    pub fn authorization_url(&self) -> Url { self.pending.authorization_url() }

    pub async fn finish(self) -> Result<GoogleAccount, CloudError> {
        tokio::time::timeout(Duration::from_secs(300), self.wait_callback()).await.map_err(|_| CloudError::Retryable("Google sign-in timed out; connect again".into()))?
    }

    async fn wait_callback(self) -> Result<GoogleAccount, CloudError> {
        loop {
            let (mut socket, peer) = self.listener.accept().await.map_err(|_| CloudError::Retryable("Could not receive the Google sign-in callback".into()))?;
            if !peer.ip().is_loopback() { continue; }
            let request = tokio::time::timeout(Duration::from_secs(2), async {
                let mut request = Vec::new();
                while request.len() < 8192 {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).await.ok()?;
                    if count == 0 { return None; }
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") { return String::from_utf8(request).ok(); }
                }
                None
            }).await.ok().flatten();
            let callback = request.as_deref().and_then(|request| {
                let mut line = request.lines().next()?.split_whitespace();
                if line.next()? != "GET" { return None; }
                let target = line.next()?;
                let url = self.redirect.join(target).ok()?;
                self.pending.callback_code(&url).map(|_| url.clone()).or_else(|error| {
                    if matches!(error, CloudError::PermissionDenied) { Ok(url) } else { Err(error) }
                }).ok()
            });
            let (status, body) = if callback.is_some() {
                ("200 OK", "<!doctype html><meta charset=utf-8><title>Bokheim</title><p>Return to Bokheim to finish connecting your Google Drive.</p>")
            } else {
                ("400 Bad Request", "This request does not belong to a pending Bokheim sign-in.")
            };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = socket.write_all(response.as_bytes()).await;
            drop(socket);
            if let Some(callback) = callback { return self.connections.finish_google_login(self.pending, callback).await; }
        }
    }
}
