use content_address::BookId;
use library_model::CardKey;
use rusqlite::Connection;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

#[tokio::test]
async fn retained_external_cover_does_not_authorize_book_downloads() {
    let key = CardKey([4; 32]);
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../library-database/tests/sql/catalog_pagination.sql")).unwrap();
    db.execute("INSERT INTO snapshot VALUES(1,?1,1)", [library_database::catalog::FORMAT_VERSION]).unwrap();
    db.execute_batch("INSERT INTO source VALUES(1,'example.org'); INSERT INTO description VALUES(1,'Source description');").unwrap();
    db.execute("INSERT INTO book(row_id,key,external,title,description_id,source_id,entry_url,format) VALUES(1,?1,1,'Linked book',1,1,'https://example.org/book',1)", [&key.0[..]]).unwrap();
    library_database::catalog::activate(db.serialize("main").unwrap().to_vec()).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    std::env::set_var("BOKHEIM_CATALOG_ORIGIN", format!("http://{}", listener.local_addr().unwrap()));
    let body = b"retained image bytes";
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 2048];
        let size = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..size]).starts_with(&format!("GET /covers/{key}-card.jpg ")));
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        stream.write_all(body).unwrap();
    });
    assert_eq!(library_catalog::external_cover(key, false).await.as_deref(), Some(body.as_slice()));
    server.join().unwrap();
    // Served from cache after the origin is gone.
    assert_eq!(library_catalog::external_cover(key, false).await.as_deref(), Some(body.as_slice()));
    assert!(library_catalog::external_cover(CardKey([5; 32]), false).await.is_none());
    let synthetic_hash: BookId = key.to_string().parse().unwrap();
    assert_eq!(library_catalog::cover(synthetic_hash, false).await.as_deref(), Some(body.as_slice()));
    assert_eq!(library_catalog::download_book(synthetic_hash).await.unwrap_err().failure(), Some(client_runtime::Failure::NotFound));
    assert!(library_database::catalog::entry(key).unwrap().unwrap().hash.is_none());
}
