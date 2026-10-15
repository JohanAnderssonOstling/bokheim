#![cfg(unix)]

use base64::{engine::general_purpose::STANDARD, Engine};
use library_model::OpdsCredentials;
use library_opds::{browser::{self, Request, Response}, feed_title};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::thread;

const FEED: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Verified feed</title></feed>"#;

#[tokio::test]
async fn challenges_try_browser_once_then_report_friendly_errors_and_only_401_requests_sign_in() {
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let other_url = format!("http://{}", other.local_addr().unwrap());
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let root = format!("http://{}", server.local_addr().unwrap());
    let redirect = other_url.clone();
    let server_thread = thread::spawn(move || {
        for _ in 0..9 {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert_ne!(count, 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).unwrap();
            let (status, headers, body) = match request.split_whitespace().nth(1).unwrap() {
                "/public" => ("200 OK", String::new(), FEED),
                "/private" => ("401 Unauthorized", String::new(), ""),
                "/denied" => ("403 Forbidden", String::new(), ""),
                "/redirect" => ("302 Found", format!("Location: {redirect}/challenge\r\n"), ""),
                "/challenge" => ("403 Forbidden", "cf-mitigated: challenge\r\n".into(), ""),
                path => panic!("unexpected request {path}"),
            };
            write!(stream, "HTTP/1.1 {status}\r\n{headers}Content-Type: application/atom+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let other_thread = thread::spawn(move || {
        let (mut stream, _) = other.accept().unwrap();
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(!String::from_utf8_lossy(&buffer[..count]).to_ascii_lowercase().contains("authorization:"));
        stream.write_all(b"HTTP/1.1 403 Forbidden\r\ncf-mitigated: challenge\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let temp = tempfile::tempdir().unwrap();
    let helper = temp.path().join("browser");
    let record = temp.path().join("browser.request");
    let calls = temp.path().join("browser.calls");
    let response_file = temp.path().join("browser.response");
    std::fs::write(&helper, "#!/bin/sh\ncat > \"$0.request\"\nprintf 'attempt\\n' >> \"$0.calls\"\ncat \"$0.response\"\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::env::set_var("BOKHEIM_OPDS_WEBVIEW_HELPER", &helper);
    browser::configure(temp.path().join("profile"));
    let success = Response::Fetched { status: 200, content_type: Some("application/atom+xml".into()), url: format!("{root}/challenge"), body: STANDARD.encode(FEED) };
    std::fs::write(&response_file, serde_json::to_vec(&success).unwrap()).unwrap();
    assert_eq!(feed_title(&format!("{root}/public"), None).await.unwrap().as_deref(), Some("Verified feed"));
    assert_eq!(feed_title(&format!("{root}/private"), None).await.unwrap(), None);
    assert!(feed_title(&format!("{root}/denied"), None).await.unwrap_err().contains("blocking access from Bokheim"));
    assert!(!calls.exists());
    assert_eq!(feed_title(&format!("{root}/challenge"), None).await.unwrap().as_deref(), Some("Verified feed"));
    let anonymous: Request = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    assert!(anonymous.authorization.is_none());
    assert_eq!(anonymous.navigation_url.as_deref(), Some(format!("{root}/challenge").as_str()));
    let credentials = OpdsCredentials { username: "reader".into(), password: "password".into() };
    feed_title(&format!("{root}/challenge"), Some(&credentials)).await.unwrap();
    let authenticated: Request = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    assert_eq!(authenticated.authorization.as_deref(), Some("Basic cmVhZGVyOnBhc3N3b3Jk"));
    assert!(authenticated.navigation_url.is_none());
    feed_title(&format!("{root}/redirect"), Some(&credentials)).await.unwrap();
    let redirected: Request = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    assert!(redirected.authorization.is_none());
    assert!(redirected.navigation_url.is_none());
    assert_eq!(redirected.url, format!("{other_url}/challenge"));
    assert_eq!(redirected.data_directory, anonymous.data_directory, "verification reuses the browser profile across origins");
    assert_eq!(anonymous.data_directory, browser::shared_profile(&temp.path().join("profile")));
    let expected = "Browser verification could not be completed for this catalog. Please try another catalog.";
    std::fs::write(&response_file, serde_json::to_vec(&Response::Error { message: "verification was rejected".into() }).unwrap()).unwrap();
    assert_eq!(feed_title(&format!("{root}/challenge"), None).await.unwrap_err(), expected);
    assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 4, "a failed browser attempt must not launch another helper");
    for status in [403, 401] {
        let response = Response::Fetched { status, content_type: None, url: format!("{root}/challenge"), body: String::new() };
        std::fs::write(&response_file, serde_json::to_vec(&response).unwrap()).unwrap();
        let result = feed_title(&format!("{root}/challenge"), None).await;
        if status == 401 { assert_eq!(result.unwrap(), None); }
        else { assert_eq!(result.unwrap_err(), expected); }
    }
    assert_eq!(std::fs::read_to_string(calls).unwrap().lines().count(), 6);
    server_thread.join().unwrap();
    other_thread.join().unwrap();
}
