//! Browser CPU transport and source/session lifetimes.
use super::*;

pub(super) async fn publication(task: PublicationTask, reader: crate::BoxedBookReader) -> Result<CpuResponse, String> {
    let server = crate::browser_adapter::publication_source::Server::new(reader)?;
    let bytes = rmp_serde::to_vec_named(&CpuRequest::Publication { task }).map_err(|e| e.to_string())?;
    let response = crate::browser_adapter::cpu_client::request_with_source(bytes, server.source())?.await?;
    rmp_serde::from_slice(&response).map_err(|e| e.to_string())
}

pub(super) async fn request(request: CpuRequest) -> Result<CpuResponse, String> {
    let bytes = rmp_serde::to_vec_named(&request).map_err(|error| error.to_string())?;
    let response = crate::browser_adapter::cpu_request(bytes)?.await?;
    rmp_serde::from_slice(&response).map_err(|error| error.to_string())
}

pub(super) const CAPABILITIES: InspectionCapabilities = InspectionCapabilities { ocr: false, mobi_pages: false };

pub(super) fn mobi_pages(_path: &std::path::Path, _reader: crate::BoxedBookReader) -> Result<library_model::PublicationMetadata, String> {
    Err("subject page inspection is not supported for this format".into())
}

pub(crate) async fn submit_source<C: PublicationOperation>(operation: C, source: wasm_bindgen::JsValue) -> Result<C::Reply, String> {
    let bytes = rmp_serde::to_vec_named(&operation.request()).map_err(|e| e.to_string())?;
    let response = crate::browser_adapter::cpu_client::request_with_source(bytes, source)?.await?;
    C::decode(rmp_serde::from_slice(&response).map_err(|e| e.to_string())?)
}

pub(crate) struct RemoteHasher(Option<String>);

impl RemoteHasher {
    pub(crate) async fn new() -> Result<Self, String> {
        let session = Self(Some(uuid::Uuid::new_v4().to_string()));
        submit(HashStart { key: session.0.clone().unwrap() }).await?;
        Ok(session)
    }

    pub(crate) async fn update(&mut self, bytes: &[u8]) -> Result<(), String> {
        for chunk in bytes.chunks(256 * 1024) {
            submit(HashUpdate { key: self.0.clone().ok_or("hash session finished")?, bytes: chunk.to_vec() }).await?;
        }
        Ok(())
    }

    pub(crate) async fn finish(mut self) -> Result<String, String> {
        let result = submit(HashFinish { key: self.0.clone().ok_or("hash session finished")? }).await?;
        self.0 = None;
        Ok(result)
    }
}

impl Drop for RemoteHasher {
    fn drop(&mut self) {
        if let Some(key) = self.0.take() {
            wasm_bindgen_futures::spawn_local(async move {
                let _ = submit(HashAbort { key }).await;
            });
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ThumbnailExtractor;
impl ThumbnailExtractor {
    pub(crate) async fn generate(&self, extension: String, reader: crate::BoxedBookReader) -> Result<Option<ThumbnailBytes>, String> {
        submit_publication(Thumbnail { extension }, reader).await
    }
}
