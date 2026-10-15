use library_model::CardKey;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::{sync::mpsc, thread};
use tokio::sync::oneshot;

#[tokio::test]
async fn eviction_rejects_an_old_response_and_allows_fresh_covers_after_reading() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    std::env::set_var("BOKHEIM_CATALOG_ORIGIN", format!("http://{}", listener.local_addr().unwrap()));
    let key = CardKey([93; 32]);
    let (started, received) = oneshot::channel();
    let (resume, wait) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut started = Some(started);
        for body in [b"old".as_slice(), b"fresh", b"after reading"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
            }
            assert!(String::from_utf8_lossy(&request).starts_with(&format!("GET /covers/{key}-card.jpg ")));
            if let Some(started) = started.take() {
                started.send(()).unwrap();
                wait.recv().unwrap();
            }
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(body).unwrap();
        }
    });

    let (old, ()) = tokio::join!(library_catalog::external_cover(key, false), async {
        received.await.unwrap();
        library_catalog::evict_cover_caches();
        resume.send(()).unwrap();
    });
    assert_eq!(old.as_deref(), Some(b"old".as_slice()));
    assert_eq!(library_catalog::external_cover(key, false).await.as_deref(), Some(b"fresh".as_slice()));
    assert_eq!(library_catalog::external_cover(key, false).await.as_deref(), Some(b"fresh".as_slice()), "a new request can cache normally");
    library_catalog::evict_cover_caches();
    assert_eq!(library_catalog::external_cover(key, false).await.as_deref(), Some(b"after reading".as_slice()));
    server.join().unwrap();
}
