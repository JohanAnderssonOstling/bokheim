#![cfg(not(target_arch = "wasm32"))]

use client_platform_runtime::transfers::{NetworkCost, POLICY};
use library_model::{BookSource, OpdsSource, OpdsSourceId};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

#[tokio::test]
async fn explicit_opds_acquisitions_bypass_the_automatic_transfer_restriction() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = OpdsSource { id: OpdsSourceId::from_u128(51), name: "Books".into(), url: format!("http://{}", listener.local_addr().unwrap()) };
    let feed = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Books</title><entry><id>book</id><title>Novel</title><link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/book.epub"/></entry></feed>"#;
    let server = thread::spawn(move || {
        for (path, body) in [("/", feed.as_bytes()), ("/book.epub", b"book bytes".as_slice())] {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).starts_with(&format!("GET {path} ")));
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/atom+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            socket.write_all(body).unwrap();
        }
    });
    POLICY.set_network_cost(NetworkCost::Metered);
    POLICY.set_allow_metered(false);
    let (contents, _) = library_opds::browse(&source, None, None, "").await.unwrap();
    let BookSource::Opds { entry, .. } = contents.books[0].source else { panic!("expected OPDS book") };
    let (name, bytes) = tokio::time::timeout(Duration::from_secs(5), library_opds::download(&source, None, entry)).await.expect("explicit OPDS download must not wait for unmetered Wi-Fi").unwrap();
    assert_eq!(name, "Novel.epub");
    assert_eq!(bytes, b"book bytes");
    assert!(!POLICY.allowed(), "the exemption must not enable automatic transfers");
    server.join().unwrap();
}
