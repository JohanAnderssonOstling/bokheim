use audiobook_archive::{archived_cover, archived_tracks, write_archive, Track};
use client_platform_runtime::transfers::{NetworkCost, POLICY};
use library_model::{CardKey, CatalogImportProgress};
use rusqlite::Connection;
use std::{
    cell::RefCell,
    fs,
    io::{Cursor, Read, Write},
    net::TcpListener,
    thread,
};

#[tokio::test]
async fn audiobook_archive_is_unchanged_and_thumbnail_source_bypasses_browsing_cache() {
    let key = CardKey([6; 32]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    std::env::set_var("BOKHEIM_CATALOG_ORIGIN", &origin);
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../library-database/tests/sql/catalog_pagination.sql")).unwrap();
    db.execute("INSERT INTO snapshot VALUES(1,?1,1)", [library_database::catalog::FORMAT_VERSION]).unwrap();
    db.execute_batch("INSERT INTO source VALUES(1,'example.org'); INSERT INTO description VALUES(1,'Source description');").unwrap();
    db.execute(
        "INSERT INTO book(row_id,key,external,title,description_id,source_id,entry_url,download_url,format) VALUES(1,?1,1,'Audio book',1,1,'https://example.org/book',?2,16)",
        rusqlite::params![&key.0[..], format!("{origin}/source.zip")],
    )
    .unwrap();
    library_database::catalog::activate(db.serialize("main").unwrap().to_vec()).unwrap();

    let path = std::env::temp_dir().join(format!("bokheim-audio-cover-test-{}.mp3", std::process::id()));
    let audio = b"unchanged audio frames";
    fs::write(&path, audio).unwrap();
    let archive = write_archive(Cursor::new(Vec::new()), &[Track { name: "chapter.mp3".into(), path: path.clone() }]).unwrap().into_inner();
    let archive_bytes = archive.len() as u64;
    fs::remove_file(path).unwrap();
    let old = b"\xff\xd8old cover";
    let selected = b"\xff\xd8selected cover";
    let server = thread::spawn(move || {
        for (url, body) in [(format!("/covers/{key}-detail.jpg"), old.as_slice()), ("/source.zip".into(), archive.as_slice()), (format!("/covers/{key}-detail.jpg"), selected.as_slice())] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let size = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with(&format!("GET {url} ")));
            if url.contains("covers") {
                assert!(request.to_lowercase().contains("cache-control: no-cache"));
            }
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(body).unwrap();
        }
    });
    assert_eq!(library_catalog::external_cover(key, true).await.unwrap(), old);
    POLICY.set_network_cost(NetworkCost::Metered);
    POLICY.set_allow_metered(false);
    let progress = RefCell::new(Vec::new());
    let downloaded = library_catalog::download_audiobook_with_progress(key, &|update| progress.borrow_mut().push(update)).await.unwrap();
    assert_eq!(progress.borrow().last(), Some(&CatalogImportProgress::Importing));
    assert!(progress.borrow().contains(&CatalogImportProgress::Downloading { downloaded_bytes: archive_bytes, total_bytes: Some(archive_bytes) }));
    assert_eq!(library_catalog::fresh_external_cover(key).await.unwrap().unwrap(), selected);
    assert!(!POLICY.allowed(), "explicit acquisitions must not enable automatic transfers");
    server.join().unwrap();
    assert_eq!(archived_cover(Cursor::new(&downloaded)).unwrap(), None);
    let track = &archived_tracks(Cursor::new(&downloaded)).unwrap()[0];
    let bytes = &downloaded[track.offset as usize..(track.offset + track.length) as usize];
    assert_eq!(bytes, audio);
    assert!(!bytes.windows(selected.len()).any(|part| part == selected));
    assert_eq!(library_catalog::external_cover(key, true).await.unwrap(), selected);
}
