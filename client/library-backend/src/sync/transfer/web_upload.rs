//! Browser integration contract for the shared OPFS uploader.
#[cfg(feature = "storage-contract-tests")]
use asset_transfer::books::web_upload::upload;

#[cfg(feature = "storage-contract-tests")]
pub(crate) async fn contract(server: &str) -> Result<(), String> {
    use super::TransferError::*;
    use library_replica::BlobKind;
    use sync_common::ContentHash;
    let store = crate::asset_store::open("upload-contract").map_err(|e| e.to_string())?;
    let bytes = vec![42u8; 2 * 1024 * 1024];
    let hash = ContentHash::new(blake3::hash(&bytes).to_hex().as_str());
    let mut writer = store.begin_stream_write(BlobKind::Book, &hash).await.map_err(|e| e.to_string())?;
    writer.write_all(&bytes).await.map_err(|e| e.to_string())?;
    writer.commit().await.map_err(|e| e.to_string())?;
    let book = store.verified_book_reader(hash).await.map_err(|e| e.to_string())?.unwrap();
    store.remove(BlobKind::Book, &hash).map_err(|e| e.to_string())?;
    // Deleting the logical entry must not remove the pinned upload file.
    // Verification is complete. Any additional JS/WASM byte read is a regression.
    js_sys::eval("FileSystemSyncAccessHandle.prototype.read = function() { throw Error('upload reread'); }; Request.prototype.arrayBuffer = Blob.prototype.arrayBuffer = function() { throw Error('upload buffered'); };")
        .map_err(|e| format!("{e:?}"))?;
    for status in [204, 401, 507, 408, 429, 404, 500] {
        let url = reqwest::Url::parse(&format!("{server}/upload/{status}")).unwrap();
        let result = upload(&book, &url, "contract-token").await;
        assert!(match (status, result) {
            (204, Ok(())) | (401, Err(AuthenticationRequired)) | (507 | 408 | 429 | 500, Err(Retryable(_))) | (404, Err(Rejected(_))) => true,
            (_, result) => panic!("unexpected upload result: {result:?}"),
        });
    }
    // A dropped upload must abort even before response headers arrive.
    let started = js_sys::eval("globalThis.savedUploadFetch = globalThis.fetch; new Promise(started => { globalThis.fetch = (_, init) => { globalThis.uploadSignal = init.signal; started(); return new Promise(() => {}); }; })")
        .map_err(|e| format!("{e:?}"))?;
    let url = reqwest::Url::parse(server).unwrap();
    match futures_util::future::select(Box::pin(upload(&book, &url, "contract-token")), Box::pin(wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(started)))).await {
        futures_util::future::Either::Right((_, pending)) => drop(pending),
        futures_util::future::Either::Left(_) => panic!("upload completed before cancellation"),
    }
    assert_eq!(js_sys::eval("globalThis.fetch = globalThis.savedUploadFetch; globalThis.uploadSignal.aborted").map_err(|e| format!("{e:?}"))?.as_bool(), Some(true));
    Ok(())
}
