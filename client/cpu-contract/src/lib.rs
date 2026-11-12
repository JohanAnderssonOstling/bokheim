//! Portable CPU worker messages, host interfaces, and typed submissions.
//! Execution and book/image parsing belong to `cpu-host`, not this contract.
use book_inspection_contract::{CachedPdfReader, InspectedBook};
use book_model::{BookFormat, BookRecord, BookTextIndex, BookTextLocation, BookTextMatch};
use client_platform_runtime::executor::{spawn_detached, BackendSend, BoxedBackendFuture};
pub use client_protocol::BackendError;
use content_address::BookId;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek};
use std::sync::Arc;
pub use update_protocol::Artifact;

/// Byte reader handed to CPU work. A single trait keeps the object type legal;
/// holders of book-access readers convert them with [`as_cpu_reader`].
pub trait CpuReader: Read + Seek + Send + Sync + 'static {}
impl<T: Read + Seek + Send + Sync + 'static> CpuReader for T {}
pub type BoxedCpuReader = Box<dyn CpuReader>;

/// Adapt an owned book reader (for example a book-access reader) for CPU work.
/// The opened reader owns its bytes; callers must not hold placement leases
/// across the conversion.
pub fn as_cpu_reader<R: Read + Seek + Send + Sync + 'static>(reader: R) -> BoxedCpuReader {
    Box::new(reader)
}

/// Metadata and cover parsers request ranges from the storage owner.
#[derive(Serialize, Deserialize)]
pub enum BookTask {
    Inspect {
        source_name: String,
        format: BookFormat,
        #[serde(rename = "content_hash")]
        book_id: Option<BookId>,
        #[serde(default)]
        cached_pdf: Option<CachedPdfReader>,
        #[serde(default)]
        declared_pdf: Option<BookRecord>,
    },
    InspectEpubMetadata {
        source_name: String,
    },
    HashAudiobookContent {},
    Thumbnail {
        extension: String,
    },
    TextMatches {
        format: BookFormat,
        locations: Vec<BookTextLocation>,
        query: String,
    },
    IndexText {
        format: BookFormat,
    },
}

#[derive(Serialize, Deserialize)]
pub enum CpuResponse {
    Done,
    Inspection(InspectedBook),
    Hash(BookId),
    Thumbnail(Option<ThumbnailBytes>),
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
    TextMatches(Vec<BookTextMatch>),
    TextIndex(BookTextIndex),
}

pub use thumbnail_contract::ThumbnailVersions as ThumbnailBytes;

/// Callers select an operation; only this boundary decodes the wire variant.
pub trait Operation: BackendSend + 'static {
    type Reply;
    fn request(self) -> CpuRequest;
    fn decode(response: CpuResponse) -> Result<Self::Reply, BackendError>;
}
pub trait BookOperation: Operation {
    fn task(self) -> BookTask;
}
macro_rules! operations {
    ($($name:ident { $($(#[$attribute:meta])* $field:ident: $ty:ty),* } => $reply:ty, $pattern:pat => $value:expr;)*) => {
        #[derive(Serialize, Deserialize)]
        pub enum CpuRequest {
            /// `taxonomy` names the snapshot a browser CPU worker loads before
            /// inspecting. None selects the web app's bundled reference;
            /// native hosts share the process taxonomy and also send none.
            Book { task: BookTask, taxonomy: Option<Artifact> },
            $($name { $($(#[$attribute])* $field: $ty),* }),*
        }
        $(pub struct $name { $(pub $field: $ty),* }
        impl Operation for $name {
            type Reply = $reply;
            fn request(self) -> CpuRequest { let Self { $($field),* } = self; CpuRequest::$name { $($field),* } }
            fn decode(response: CpuResponse) -> Result<Self::Reply, BackendError> {
                match response { $pattern => Ok($value), _ => Err(concat!("invalid CPU reply for ", stringify!($name)).into()) }
            }
        }
    )*};
}
operations! {
    HashStart { key: String } => (), CpuResponse::Done => ();
    HashUpdate { key: String, #[serde(with = "serde_bytes")] bytes: Vec<u8> } => (), CpuResponse::Done => ();
    HashFinish { key: String } => BookId, CpuResponse::Hash(hash) => hash;
    HashAbort { key: String } => (), CpuResponse::Done => ();
    Resize { width: u32, #[serde(with = "serde_bytes")] bytes: Vec<u8> } => Vec<u8>, CpuResponse::Bytes(bytes) => bytes;
    Cover { #[serde(with = "serde_bytes")] bytes: Vec<u8> } => ThumbnailBytes, CpuResponse::Thumbnail(Some(versions)) => versions;
}
macro_rules! book_operations {
    ($($name:ident { $($field:ident: $ty:ty),* } => $reply:ty, $pattern:pat => $value:expr;)*) => {$ (
        pub struct $name { $(pub $field: $ty),* }
        impl BookOperation for $name {
            fn task(self) -> BookTask { let Self { $($field),* } = self; BookTask::$name { $($field),* } }
        }
        impl Operation for $name {
            type Reply = $reply;
            fn request(self) -> CpuRequest { CpuRequest::Book { task: self.task(), taxonomy: None } }
            fn decode(response: CpuResponse) -> Result<Self::Reply, BackendError> {
                match response { $pattern => Ok($value), _ => Err(concat!("invalid CPU reply for ", stringify!($name)).into()) }
            }
        }
    )*};
}
book_operations! {
    HashAudiobookContent {} => BookId, CpuResponse::Hash(hash) => hash;
    IndexText { format: BookFormat } => BookTextIndex, CpuResponse::TextIndex(index) => index;
    TextMatches { format: BookFormat, locations: Vec<BookTextLocation>, query: String } => Vec<BookTextMatch>, CpuResponse::TextMatches(matches) => matches;
    Inspect { source_name: String, format: BookFormat, book_id: Option<BookId>, cached_pdf: Option<CachedPdfReader>, declared_pdf: Option<BookRecord> } => InspectedBook, CpuResponse::Inspection(inspection) => inspection;
    InspectEpubMetadata { source_name: String } => InspectedBook, CpuResponse::Inspection(inspection) => inspection;
    Thumbnail { extension: String } => Option<ThumbnailBytes>, CpuResponse::Thumbnail(versions) => versions;
}

/// Execution host. Native hosts run work on the blocking executor; browser
/// hosts forward it to the CPU worker. Application startup shares a host
/// across its libraries; typed submissions are free functions below.
pub trait CpuHost: Send + Sync + 'static {
    fn book(&self, task: BookTask, reader: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>>;
    fn request(&self, request: CpuRequest) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>>;
    fn generate_thumbnail(&self, extension: String, reader: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailBytes>, BackendError>> {
        Box::pin(async move { Thumbnail::decode(self.book(BookTask::Thumbnail { extension }, reader).await?) })
    }
}

pub async fn submit<O: Operation>(host: &dyn CpuHost, operation: O) -> Result<O::Reply, BackendError> {
    O::decode(host.request(operation.request()).await?)
}

pub async fn submit_book<B: BookOperation>(host: &dyn CpuHost, operation: B, reader: BoxedCpuReader) -> Result<B::Reply, BackendError> {
    B::decode(host.book(operation.task(), reader).await?)
}

pub async fn cover(host: &dyn CpuHost, bytes: Vec<u8>) -> Result<ThumbnailBytes, BackendError> {
    submit(host, Cover { bytes }).await
}

pub async fn resize(host: &dyn CpuHost, bytes: Vec<u8>, width: u32) -> Result<Vec<u8>, BackendError> {
    submit(host, Resize { bytes, width }).await
}

/// Remote BLAKE3 session over any host. Dropping an unfinished session
/// releases the worker-side hasher.
pub struct RemoteHashSession {
    host: Arc<dyn CpuHost>,
    key: Option<String>,
}
impl RemoteHashSession {
    pub async fn new(host: &Arc<dyn CpuHost>) -> Result<Self, BackendError> {
        let key = uuid::Uuid::new_v4().to_string();
        submit(&**host, HashStart { key: key.clone() }).await?;
        Ok(Self { host: host.clone(), key: Some(key) })
    }
    pub async fn update(&mut self, bytes: &[u8]) -> Result<(), BackendError> {
        let key = self.key.clone().ok_or("hash session finished")?;
        // Send a whole browser import chunk while leaving room for the wire
        // envelope within the CPU transport's input budget.
        for chunk in bytes.chunks(32 * 1024 * 1024) {
            submit(&*self.host, HashUpdate { key: key.clone(), bytes: chunk.to_vec() }).await?;
        }
        Ok(())
    }
    pub async fn finish(mut self) -> Result<BookId, BackendError> {
        let key = self.key.clone().ok_or("hash session finished")?;
        let result = submit(&*self.host, HashFinish { key }).await;
        self.key = None;
        result
    }
}
impl Drop for RemoteHashSession {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            let host = self.host.clone();
            spawn_detached(async move {
                let _ = submit(&*host, HashAbort { key }).await;
            });
        }
    }
}
