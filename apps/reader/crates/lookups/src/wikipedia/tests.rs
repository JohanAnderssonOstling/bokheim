use super::*;

#[test]
fn encodes_titles_and_uses_one_book_language_without_network_fallbacks() {
    let (url, language) = request_url(" Haus / Tür? ", &[crate::parse_language_tag("de_DE.UTF-8").unwrap(), crate::parse_language_tag("en").unwrap()]).unwrap();
    assert_eq!(language.as_str(), "de");
    assert_eq!(url.host_str(), Some("de.wikipedia.org"));
    assert!(url.path().ends_with("Haus_%2F_T%C3%BCr%3F"));
    assert!(url.query().is_none());
    assert_eq!(request_url("Earth", &[]).unwrap().1.as_str(), "en");
    assert_eq!(request_url("Jorden", &[crate::parse_language_tag("nb-NO").unwrap()]).unwrap().1.as_str(), "no");
    for query in [" ", "Earth\nMoon", &"x".repeat(161)] {
        assert!(request_url(query, &[]).is_none());
    }
}

#[test]
fn summaries_are_plain_text_and_link_only_to_the_requested_wikipedia() {
    let Some(Summary::Article(article)) = parse(br#"{"type":"standard","title":"Albert Einstein","extract":"A physicist.","content_urls":{"desktop":{"page":"https://example.com/unsafe"}}}"#, &LanguageTag::parse("de").unwrap()) else { panic!("expected an article") };
    assert_eq!(article.title, "Albert Einstein");
    assert_eq!(article.extract, "A physicist.");
    assert_eq!(article.url, "https://de.wikipedia.org/wiki/Albert_Einstein");
    for json in [br#"{"type":"standard","title":"Earth","extract":" "}"#.as_slice(), b"{", br#"{"error":"not found"}"#] {
        assert!(parse(json, &LanguageTag::parse("en").unwrap()).is_none());
    }
}

#[test]
fn ambiguity_returns_choices_without_picking_an_article() {
    assert!(matches!(parse(br#"{"type":"disambiguation","title":"Mercury","extract":"May refer to..."}"#, &LanguageTag::parse("en").unwrap()), Some(Summary::Ambiguous(title)) if title == "Mercury"));
    let choices =
        parse_choices(br#"{"pages":[{"title":"Mercury","description":"Ambiguous"},{"title":"Mercury (planet)","description":"Planet"},{"title":"Mercury (planet)"},{"title":"Mercury (element)","description":null}]}"#, &LanguageTag::parse("en").unwrap(), "Mercury")
            .unwrap();
    assert_eq!(choices.len(), 2);
    assert_eq!(choices[0].title, "Mercury (planet)");
    assert_eq!(choices[0].description, "Planet");
    assert_eq!(choices[1].description, "");
    assert!(parse_choices(br#"{"pages":[]}"#, &LanguageTag::parse("en").unwrap(), "Mercury").is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn successive_requests_reuse_a_connection() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = Url::parse(&format!("http://{}/summary", listener.local_addr().unwrap())).unwrap();
    let client = client().unwrap();
    let server = async {
        let (mut socket, _) = listener.accept().await.unwrap();
        // Both requests must arrive on this socket. A second connection
        // would leave this server waiting until the test times out.
        for _ in 0..2 {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 4096);
                request.push(socket.read_u8().await.unwrap());
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").await.unwrap();
        }
    };
    let requests = async {
        for _ in 0..2 {
            assert_eq!(fetch(&client, url.clone()).await.as_deref(), Some(b"{}".as_slice()));
        }
    };
    tokio::time::timeout(Duration::from_secs(3), async { tokio::join!(requests, server) }).await.expect("requests should share one connection");
}

#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn failed_http_requests_finish_quietly_without_retrying() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    for status in ["404 Not Found", "429 Too Many Requests", "503 Service Unavailable"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/summary", listener.local_addr().unwrap())).unwrap();
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\nRetry-After: 0\r\n\r\n").as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
            assert!(tokio::time::timeout(Duration::from_millis(100), listener.accept()).await.is_err(), "Wikipedia failures must not schedule another attempt");
        };
        let client = client().unwrap();
        let (result, ()) = tokio::join!(fetch(&client, url), server);
        assert!(result.is_none());
    }
}
