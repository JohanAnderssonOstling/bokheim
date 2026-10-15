use super::*;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

#[tokio::test]
async fn streamed_download_reports_actual_bytes_with_and_without_a_total() {
    for known_total in [true, false] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/book", listener.local_addr().unwrap());
        let content = vec![42; 96 * 1024];
        let expected = content.clone();
        let total = content.len() as u64;
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).unwrap();
            let size_header = if known_total { format!("Content-Length: {total}") } else { "Transfer-Encoding: chunked".into() };
            write!(socket, "HTTP/1.1 200 OK\r\n{size_header}\r\nConnection: close\r\n\r\n").unwrap();
            for chunk in content.chunks(32 * 1024) {
                // Ensure the body takes more than one reporting interval, so
                // an implementation buffering the entire response fails.
                thread::sleep(Duration::from_millis(130));
                if !known_total {
                    write!(socket, "{:x}\r\n", chunk.len()).unwrap();
                }
                socket.write_all(chunk).unwrap();
                if !known_total {
                    socket.write_all(b"\r\n").unwrap();
                }
                socket.flush().unwrap();
            }
            if !known_total {
                socket.write_all(b"0\r\n\r\n").unwrap();
            }
        });
        let observations = RefCell::new(Vec::new());
        let downloaded = download_with_progress(&url, &|progress| observations.borrow_mut().push(progress)).await.unwrap();
        server.join().unwrap();
        assert_eq!(downloaded, expected);
        let observations = observations.into_inner();
        let expected_total = known_total.then_some(total);
        assert_eq!(observations.last(), Some(&CatalogImportProgress::Downloading { downloaded_bytes: total, total_bytes: expected_total }));
        assert!(observations.iter().any(|progress| matches!(progress, CatalogImportProgress::Downloading { downloaded_bytes, total_bytes } if *downloaded_bytes > 0 && *downloaded_bytes < total && *total_bytes == expected_total)));
        let counts = observations
            .iter()
            .filter_map(|progress| match progress {
                CatalogImportProgress::Downloading { downloaded_bytes, .. } => Some(*downloaded_bytes),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(counts.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}

#[tokio::test]
async fn interrupted_download_returns_an_error_without_reporting_completion() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/book", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        socket.read(&mut request).unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\nConnection: close\r\n\r\nincomplete").unwrap();
    });
    let observations = RefCell::new(Vec::new());
    assert!(download_with_progress(&url, &|progress| observations.borrow_mut().push(progress)).await.is_err());
    server.join().unwrap();
    assert!(!observations.borrow().iter().any(|progress| matches!(progress, CatalogImportProgress::Downloading { downloaded_bytes: 4096, .. } | CatalogImportProgress::Importing)));
}
