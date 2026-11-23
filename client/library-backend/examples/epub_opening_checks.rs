//! Browser fixtures for production remote ZIP readers and shared range batching.
#![cfg(target_arch = "wasm32")]

use book_access::remote_file;
use book_content::{Book, BookFormat, comic::ComicBook};
use book_model::BookFormat as ContainerFormat;
use library_backend::{
    BackendError, BookId, BookSource, ResolvedBookData,
    library::{
        DirectPlayback, PlaybackSource, RemoteFile,
        web_reader::{BookTransport, resolve_book},
    },
};
use wasm_bindgen::prelude::*;

#[derive(Clone)]
struct Transport {
    client: reqwest::Client,
    url: reqwest::Url,
    format: ContainerFormat,
}

impl BookTransport for Transport {
    async fn open_remote_audio(&self, _: BookId) -> Result<Option<PlaybackSource>, BackendError> {
        Ok(None)
    }

    async fn renew_remote_audio(&self, _: BookId) -> Result<DirectPlayback, BackendError> {
        Err(BackendError::message("browser ZIP source does not provide audio"))
    }

    async fn read_book_range(&self, _: String, _: u64, _: usize) -> Result<Vec<u8>, BackendError> {
        Err(BackendError::message("browser ZIP source does not provide local file capabilities"))
    }

    async fn read_file_range(&self, source: RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, BackendError> {
        remote_file::fetch_range(&self.client, self.url.clone(), &source, "fixture", offset, length).await.map_err(BackendError::operation)
    }

    async fn read_file_ranges(&self, source: RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, BackendError> {
        remote_file::fetch_ranges(&self.client, self.url.clone(), &source, "fixture", &ranges).await.map_err(BackendError::operation)
    }

    async fn subscribe_book(&self, hash: BookId) -> Result<(async_channel::Receiver<()>, ResolvedBookData), BackendError> {
        let (source, tail) = remote_file::describe_tail(&self.client, self.url.clone(), "fixture", hash).await.map_err(BackendError::operation)?;
        let resolved = ResolvedBookData { format: self.format, pdf_metadata: None, source: BookSource::RemoteArchive { source, tail } };
        // Exercise the worker wire representation as well as the reader path.
        let resolved = serde_json::from_slice(&serde_json::to_vec(&resolved).map_err(BackendError::operation)?).map_err(BackendError::operation)?;
        let (_, lease) = async_channel::unbounded();
        Ok((lease, resolved))
    }
}

#[wasm_bindgen(js_name = checkEpubOpening)]
pub async fn check(url: String) -> Result<String, JsValue> {
    let result = async {
        let transport = Transport { client: http_response::client(), url: url.parse().map_err(BackendError::operation)?, format: ContainerFormat::Epub };
        let book = resolve_book(&transport, BookId::new(&"d".repeat(64))).await?.into_document()?;
        let library_backend::library::DocumentSource::Async(source) = book.source else { return Err(BackendError::message("expected asynchronous EPUB")) };
        let book = Book::open(BookFormat::Epub, move || source.reader()).await.map_err(BackendError::operation)?.prepare_reading().await.map_err(BackendError::operation)?;
        assert_eq!(book.document_uris().len(), 1);
        assert_eq!(book.resources.read_bytes(&book.document_uris()[0]).unwrap(), b"<html><body>Exact remote chapter</body></html>");
        assert_eq!(book.resources.read_bytes("notes.xhtml").unwrap(), b"<html><body>Auxiliary notes</body></html>");
        assert_eq!(book.resources.read_bytes("style.css").unwrap(), b"body { color: black; }");
        Ok::<_, BackendError>(serde_json::json!({"documents": book.document_uris(), "eager_text": true}).to_string())
    }
    .await;
    result.map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(js_name = checkCbzOpening)]
pub async fn check_cbz(url: String, deleted: bool) -> Result<String, JsValue> {
    let result = async {
        let transport = Transport { client: http_response::client(), url: url.parse().map_err(BackendError::operation)?, format: ContainerFormat::Cbz };
        let book = resolve_book(&transport, BookId::new(&"d".repeat(64))).await?.into_document()?;
        let library_backend::library::DocumentSource::Async(source) = book.source else { return Err(BackendError::message("expected asynchronous CBZ")) };
        assert!(source.supports_prefetch());
        let prefetch = source.clone();
        let book = ComicBook::open_with_prefetch(
            move || source.reader(),
            1,
            move |ranges| {
                let source = prefetch.clone();
                async move { source.prefetch(ranges).await }
            },
        )
        .await
        .map_err(BackendError::operation)?;
        assert_eq!(book.metadata.title, "Remote comic");
        let reader = book.into_reader().await.map_err(BackendError::operation)?;
        let expected = if deleted { 10 } else { 2 };
        for _ in 0..2 {
            let bytes = reader.read_page(1).await.map_err(BackendError::operation)?;
            assert_eq!(bytes, vec![expected; 512 * 1024]);
        }
        Ok::<_, BackendError>(serde_json::json!({"resumed_page": 1, "pages": reader.pages.len(), "cached_reread": true}).to_string())
    }
    .await;
    result.map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(inline_js = "
export async function waitForPrivateAssets(library, expected) {
    const deadline = performance.now() + 5000;
    let count = 0;
    do {
        try {
            let directory = await navigator.storage.getDirectory();
            for (const part of ['__libraries', library, 'book']) directory = await directory.getDirectoryHandle(part);
            count = 0;
            for await (const entry of directory.values()) if (entry.kind === 'file') count++;
        } catch (error) { if (error.name === 'NotFoundError') count = 0; else throw error; }
        if (count === expected) return;
        await new Promise(resolve => setTimeout(resolve, 10));
    } while (performance.now() < deadline);
    throw Error(`Expected ${expected} private assets, found ${count}`);
}
")]
extern "C" {
    #[wasm_bindgen(catch, js_name = waitForPrivateAssets)]
    async fn wait_for_private_assets(library: &str, expected: u32) -> Result<(), JsValue>;
}

/// Exercise the same OPFS source/output ownership used by catalog repacking.
#[wasm_bindgen(js_name = checkOpfsRepacking)]
pub async fn check_opfs_repacking(bytes: Vec<u8>) -> Result<String, JsValue> {
    use client_platform_web::web_storage;
    use std::io::{self, Read, Seek, SeekFrom, Write};
    let js_error = |error: &dyn std::fmt::Display| JsValue::from_str(&error.to_string());
    client_platform_runtime::executor::initialize_storage_executor().map_err(|error| js_error(&error))?;
    web_storage::initialize().await.map_err(|error| js_error(&error))?;
    let library = uuid::Uuid::new_v4().to_string();
    let name = format!("assets/{library}/book/repack");
    let mut source = web_storage::begin_write(&name).await.map_err(|error| js_error(&error))?;
    for chunk in bytes.chunks(64 * 1024) {
        source.write_all(chunk).await.map_err(|error| js_error(&error))?;
    }
    let reader = source.open_reader().await.map_err(|error| js_error(&error))?;
    // A separate reader must retain the source even after the repack closure exits.
    let source_pin = source.open_reader().await.map_err(|error| js_error(&error))?;
    let started = web_time::Instant::now();
    let mut output = web_storage::stage_file_with(&name, move |writer| {
        let _source = source;
        audiobook_archive::repack_zip_into(reader, writer, None).map(|_| ())
    })
    .await
    .map_err(|error| js_error(&error))?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    wait_for_private_assets(&library, 2).await?;
    drop(source_pin);
    wait_for_private_assets(&library, 1).await?;
    assert!(!web_storage::file_exists(&name).map_err(|error| js_error(&error))?);
    assert_eq!(web_storage::total_file_bytes(&format!("assets/{library}/")).await.map_err(|error| js_error(&error))?, 0);
    for (index, (name, byte)) in [("track2.mp3", 2u8), ("track10.mp3", 10u8)].into_iter().enumerate() {
        let track = audiobook_archive::archived_track(&mut output, index).map_err(|error| js_error(&error))?;
        assert_eq!(track.name, name);
        assert_eq!(track.length, 2 * 1024 * 1024);
        output.seek(SeekFrom::Start(track.offset)).map_err(|error| js_error(&error))?;
        let mut remaining = track.length;
        let mut buffer = [0; 64 * 1024];
        while remaining != 0 {
            let length = remaining.min(buffer.len() as u64) as usize;
            output.read_exact(&mut buffer[..length]).map_err(|error| js_error(&error))?;
            assert!(buffer[..length].iter().all(|value| *value == byte));
            remaining -= length as u64;
        }
    }
    // A failed output must clean up only its own generation, preserving the live reader.
    let failed = web_storage::stage_file_with(&name, |writer| {
        writer.write_all(b"partial archive")?;
        Err(io::Error::other("injected repack failure"))
    })
    .await;
    assert!(failed.is_err());
    wait_for_private_assets(&library, 1).await?;
    assert_eq!(audiobook_archive::archived_track(&mut output, 0).map_err(|error| js_error(&error))?.name, "track2.mp3");
    drop(output);
    wait_for_private_assets(&library, 0).await?;
    Ok(serde_json::json!({"name":"opfs-repack", "tracks":2, "payload_bytes":4 * 1024 * 1024, "repack_ms":elapsed_ms, "source_pin":true, "unpublished":true, "failure_cleanup":true, "final_reader_cleanup":true}).to_string())
}


/// Drop an acquisition while its HTTP response is stalled after a persisted chunk.
#[wasm_bindgen(js_name = checkOpfsCancellation)]
pub async fn check_opfs_cancellation(url: String) -> Result<String, JsValue> {
    use client_platform_web::web_storage;
    use futures_util::StreamExt;
    let js_error = |error: &dyn std::fmt::Display| JsValue::from_str(&error.to_string());
    client_platform_runtime::executor::initialize_storage_executor().map_err(|error| js_error(&error))?;
    web_storage::initialize().await.map_err(|error| js_error(&error))?;
    let library = uuid::Uuid::new_v4().to_string();
    let name = format!("assets/{library}/book/cancelled");
    let (written, persisted) = tokio::sync::oneshot::channel();
    let mut acquisition = Box::pin(async {
        let mut source = web_storage::begin_write(&name).await.map_err(|error| js_error(&error))?;
        let response = http_response::get(&url).await.map_err(|error| js_error(&error))?;
        let mut stream = response.bytes_stream();
        let chunk = stream.next().await.expect("server must send an initial chunk").map_err(|error| js_error(&error))?;
        assert!(!chunk.is_empty());
        source.write_all(&chunk).await.map_err(|error| js_error(&error))?;
        written.send(source.length()).unwrap();
        // The endpoint never finishes normally. Cancellation must drop source and response.
        while let Some(chunk) = stream.next().await {
            source.write_all(&chunk.map_err(|error| js_error(&error))?).await.map_err(|error| js_error(&error))?;
        }
        Ok::<(), JsValue>(())
    });
    let bytes = tokio::select! {
        result = &mut acquisition => return Err(JsValue::from_str(&format!("acquisition ended before cancellation: {result:?}"))),
        result = persisted => result.map_err(|error| js_error(&error))?,
    };
    wait_for_private_assets(&library, 1).await?;
    drop(acquisition);
    wait_for_private_assets(&library, 0).await?;
    assert!(!web_storage::file_exists(&name).map_err(|error| js_error(&error))?);
    Ok(serde_json::json!({"name":"opfs-cancellation", "persisted_bytes":bytes, "partial_source_cleanup":true, "unpublished":true}).to_string())
}

/// Keep a partial download alive until the harness terminates this worker.
#[wasm_bindgen(js_name = prepareOpfsWorkerTermination)]
pub async fn prepare_opfs_worker_termination(url: String, library: String, ready: js_sys::Function) -> Result<(), JsValue> {
    use client_platform_web::web_storage;
    use futures_util::StreamExt;
    let js_error = |error: &dyn std::fmt::Display| JsValue::from_str(&error.to_string());
    client_platform_runtime::executor::initialize_storage_executor().map_err(|error| js_error(&error))?;
    web_storage::initialize().await.map_err(|error| js_error(&error))?;
    web_storage::write_file(&format!("assets/{library}/book/committed"), b"published bytes survive worker termination").await.map_err(|error| js_error(&error))?;
    let mut source = web_storage::begin_write(&format!("assets/{library}/book/interrupted")).await.map_err(|error| js_error(&error))?;
    let response = http_response::get(&url).await.map_err(|error| js_error(&error))?;
    let mut stream = response.bytes_stream();
    let chunk = stream.next().await.expect("server must send an initial chunk").map_err(|error| js_error(&error))?;
    assert!(!chunk.is_empty());
    source.write_all(&chunk).await.map_err(|error| js_error(&error))?;
    // Flush the file before reporting readiness, while retaining its writer pin.
    let _reader = source.open_reader().await.map_err(|error| js_error(&error))?;
    wait_for_private_assets(&library, 2).await?;
    ready.call1(&JsValue::NULL, &JsValue::from_f64(source.length() as f64))?;
    while let Some(chunk) = stream.next().await {
        source.write_all(&chunk.map_err(|error| js_error(&error))?).await.map_err(|error| js_error(&error))?;
    }
    Err(JsValue::from_str("download finished before worker termination"))
}

/// Startup must reclaim the orphan without removing the committed generation.
#[wasm_bindgen(js_name = checkOpfsWorkerRecovery)]
pub async fn check_opfs_worker_recovery(library: String) -> Result<String, JsValue> {
    use client_platform_web::web_storage;
    let js_error = |error: &dyn std::fmt::Display| JsValue::from_str(&error.to_string());
    let started = web_time::Instant::now();
    client_platform_runtime::executor::initialize_storage_executor().map_err(|error| js_error(&error))?;
    web_storage::initialize().await.map_err(|error| js_error(&error))?;
    wait_for_private_assets(&library, 1).await?;
    assert!(!web_storage::file_exists(&format!("assets/{library}/book/interrupted")).map_err(|error| js_error(&error))?);
    assert_eq!(web_storage::read_file(&format!("assets/{library}/book/committed")).await.map_err(|error| js_error(&error))?.unwrap(), b"published bytes survive worker termination");
    let recovery_ms = started.elapsed().as_secs_f64() * 1000.0;
    web_storage::purge_library(library.parse().map_err(|error| js_error(&error))?).await.map_err(|error| js_error(&error))?;
    wait_for_private_assets(&library, 0).await?;
    Ok(serde_json::json!({"name":"opfs-worker-recovery", "orphan_reclaimed":true, "published_bytes_preserved":true, "recovery_ms":recovery_ms}).to_string())
}
