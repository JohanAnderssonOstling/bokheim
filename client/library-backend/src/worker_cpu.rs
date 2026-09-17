//! Shared CPU operations and their typed wire contract. Host adapters own
//! execution, transport, and resource lifetimes.

use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
#[path = "worker_cpu/native.rs"]
mod host;
#[cfg(target_arch = "wasm32")]
#[path = "worker_cpu/web.rs"]
mod host;
pub(crate) use host::ThumbnailExtractor;
#[cfg(target_arch = "wasm32")]
pub(crate) use host::{RemoteHasher, submit_source};

#[derive(Clone, Copy)]
pub(crate) struct InspectionCapabilities {
    ocr: bool,
    mobi_pages: bool,
}
impl InspectionCapabilities {
    pub(crate) fn supports(self, format: book_model::BookFormat, ocr: bool) -> bool {
        use book_model::BookFormat;
        if ocr { self.ocr && matches!(format, BookFormat::Epub | BookFormat::Pdf) } else { matches!(format, BookFormat::Epub | BookFormat::Pdf) || (self.mobi_pages && format == BookFormat::Mobi) }
    }
}
pub(crate) fn inspection_capabilities() -> InspectionCapabilities {
    host::CAPABILITIES
}

/// Metadata and cover parsers request ranges from the storage owner.
#[derive(Serialize, Deserialize)]
pub enum BookTask {
    Inspect { source_name: String, format: book_model::BookFormat },
    SubjectPages { source_name: String, format: book_model::BookFormat },
    Thumbnail { extension: String },
    SubjectOcr { source_name: String, format: book_model::BookFormat },
}

#[derive(Serialize, Deserialize)]
pub enum CpuResponse {
    Done,
    Inspection(book_metadata::InspectedBook),
    SubjectEvidence(book_model::BookMetadata),
    Hash(String),
    Thumbnail(Option<ThumbnailBytes>),
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
}

#[derive(Serialize, Deserialize)]
pub struct ThumbnailBytes {
    #[serde(with = "serde_bytes")]
    pub browse: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub high_density: Vec<u8>,
}

impl From<thumbnail::ThumbnailVersions> for ThumbnailBytes {
    fn from(versions: thumbnail::ThumbnailVersions) -> Self {
        Self { browse: versions.browse, high_density: versions.high_density }
    }
}

pub fn execute(request: CpuRequest) -> Result<CpuResponse, String> {
    match request {
        CpuRequest::Book { .. } => Err("book request requires a range source".into()),
        CpuRequest::HashStart { key } => HASHERS.with(|hashers| {
            let mut hashers = hashers.borrow_mut();
            if hashers.len() >= 32 || hashers.contains_key(&key) {
                return Err("hash session unavailable; retry the job".into());
            }
            hashers.insert(key, blake3::Hasher::new());
            Ok(CpuResponse::Done)
        }),
        CpuRequest::HashUpdate { key, bytes } => HASHERS.with(|hashers| {
            let mut hashers = hashers.borrow_mut();
            let hasher = hashers.get_mut(&key).ok_or("hash session was lost; retry the job")?;
            hasher.update(&bytes);
            Ok(CpuResponse::Done)
        }),
        CpuRequest::HashFinish { key } => HASHERS.with(|hashers| {
            let hasher = hashers.borrow_mut().remove(&key).ok_or("hash session was lost; retry the job")?;
            Ok(CpuResponse::Hash(hasher.finalize().to_hex().to_string()))
        }),
        CpuRequest::HashAbort { key } => {
            HASHERS.with(|hashers| hashers.borrow_mut().remove(&key));
            Ok(CpuResponse::Done)
        }
        CpuRequest::Cover { bytes } => {
            let versions = thumbnail::generate_thumbnail_versions_from_image_bytes(&bytes).map_err(|error| error.to_string())?;
            Ok(CpuResponse::Thumbnail(Some(versions.into())))
        }
        CpuRequest::Resize { width, bytes } => {
            // The worker accepts only the product's supported display buckets.
            if ![thumbnail::BROWSE_THUMBNAIL_WIDTH, thumbnail::THUMBNAIL_WIDTH].contains(&width) {
                return Err("unsupported thumbnail width".into());
            }
            thumbnail::generate_thumbnail_at_width_from_image_bytes(&bytes, width).map(CpuResponse::Bytes).map_err(|error| error.to_string())
        }
    }
}

thread_local! {
    static HASHERS: std::cell::RefCell<std::collections::HashMap<String, blake3::Hasher>> = Default::default();
}

pub fn execute_book(task: BookTask, reader: crate::BoxedBookReader) -> Result<CpuResponse, String> {
    match task {
        BookTask::Inspect { source_name, format } => book_metadata::inspect_book_reader(std::path::Path::new(&source_name), format, reader).map(CpuResponse::Inspection).map_err(|e| e.to_string()),
        BookTask::SubjectPages { source_name, format } => {
            let path = std::path::Path::new(&source_name);
            let book = match format {
                book_model::BookFormat::Epub => book_inspection::inspect_epub_pages_reader(path, reader),
                book_model::BookFormat::Pdf => book_inspection::inspect_pdf_pages_reader(path, reader).map(|b| b.metadata),
                book_model::BookFormat::Mobi => return host::mobi_pages(path, reader).map(CpuResponse::SubjectEvidence),
                _ => return Err("subject page inspection is not supported for this format".into()),
            }
            .map_err(|e| e.to_string())?;
            Ok(CpuResponse::SubjectEvidence(book.book))
        }
        BookTask::SubjectOcr { source_name, format } => {
            let path = std::path::Path::new(&source_name);
            let book = match format {
                book_model::BookFormat::Epub => book_inspection::inspect_epub_ocr_reader(path, reader),
                book_model::BookFormat::Pdf => book_inspection::inspect_pdf_ocr_reader(path, reader).map(|b| b.metadata),
                _ => return Err("OCR is only supported for EPUB and PDF".into()),
            }
            .map_err(|e| e.to_string())?;
            Ok(CpuResponse::SubjectEvidence(book.book))
        }
        BookTask::Thumbnail { extension } => thumbnail::generate_thumbnail_versions_from_reader(&extension, reader).map(|versions| CpuResponse::Thumbnail(versions.map(ThumbnailBytes::from))).map_err(|e| e.to_string()),
    }
}

/// Callers select an operation; only this boundary decodes the wire variant.
pub(crate) trait Operation: crate::executor::BackendSend + 'static {
    type Reply;
    fn request(self) -> CpuRequest;
    fn decode(response: CpuResponse) -> Result<Self::Reply, String>;
}
pub(crate) trait BookOperation: Operation {
    fn task(self) -> BookTask;
}
macro_rules! operations {
    ($($name:ident { $($(#[$attribute:meta])* $field:ident: $ty:ty),* } => $reply:ty, $pattern:pat => $value:expr;)*) => {
        #[derive(Serialize, Deserialize)]
        pub enum CpuRequest {
            Book { task: BookTask },
            $($name { $($(#[$attribute])* $field: $ty),* }),*
        }
        $(pub(crate) struct $name { $(pub $field: $ty),* }
        impl Operation for $name {
            type Reply = $reply;
            fn request(self) -> CpuRequest { let Self { $($field),* } = self; CpuRequest::$name { $($field),* } }
            fn decode(response: CpuResponse) -> Result<Self::Reply, String> {
                match response { $pattern => Ok($value), _ => Err(concat!("invalid CPU reply for ", stringify!($name)).into()) }
            }
        }
    )*};
}
operations! {
    HashStart { key: String } => (), CpuResponse::Done => ();
    HashUpdate { key: String, #[serde(with = "serde_bytes")] bytes: Vec<u8> } => (), CpuResponse::Done => ();
    HashFinish { key: String } => String, CpuResponse::Hash(hash) => hash;
    HashAbort { key: String } => (), CpuResponse::Done => ();
    Resize { width: u32, #[serde(with = "serde_bytes")] bytes: Vec<u8> } => Vec<u8>, CpuResponse::Bytes(bytes) => bytes;
    Cover { #[serde(with = "serde_bytes")] bytes: Vec<u8> } => ThumbnailBytes, CpuResponse::Thumbnail(Some(versions)) => versions;
}
macro_rules! book_operations {
    ($($name:ident { $($field:ident: $ty:ty),* } => $reply:ty, $pattern:pat => $value:expr;)*) => {$ (
        pub(crate) struct $name { $(pub $field: $ty),* }
        impl BookOperation for $name {
            fn task(self) -> BookTask { let Self { $($field),* } = self; BookTask::$name { $($field),* } }
        }
        impl Operation for $name {
            type Reply = $reply;
            fn request(self) -> CpuRequest { CpuRequest::Book { task: self.task() } }
            fn decode(response: CpuResponse) -> Result<Self::Reply, String> {
                match response { $pattern => Ok($value), _ => Err(concat!("invalid CPU reply for ", stringify!($name)).into()) }
            }
        }
    )*};
}
book_operations! {
    SubjectPages { source_name: String, format: book_model::BookFormat } => book_model::BookMetadata, CpuResponse::SubjectEvidence(evidence) => evidence;
    SubjectOcr { source_name: String, format: book_model::BookFormat } => book_model::BookMetadata, CpuResponse::SubjectEvidence(evidence) => evidence;
    Inspect { source_name: String, format: book_model::BookFormat } => book_metadata::InspectedBook, CpuResponse::Inspection(inspection) => inspection;
    Thumbnail { extension: String } => Option<ThumbnailBytes>, CpuResponse::Thumbnail(versions) => versions;
}

pub(crate) async fn submit_book<C: BookOperation>(operation: C, reader: crate::BoxedBookReader) -> Result<C::Reply, String> {
    C::decode(host::book(operation.task(), reader).await?)
}

async fn submit<C: Operation>(operation: C) -> Result<C::Reply, String> {
    C::decode(host::request(operation.request()).await?)
}

pub(crate) async fn resize(bytes: Vec<u8>, width: u32) -> Result<Vec<u8>, String> {
    submit(Resize { bytes, width }).await
}
pub(crate) async fn cover(bytes: Vec<u8>) -> Result<ThumbnailBytes, String> {
    submit(Cover { bytes }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(request: CpuRequest) -> CpuResponse {
        let bytes = rmp_serde::to_vec_named(&request).unwrap();
        let response = execute(rmp_serde::from_slice(&bytes).unwrap()).unwrap();
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&response).unwrap()).unwrap()
    }

    #[test]
    fn generated_requests_preserve_named_fields_and_binary_payloads() {
        let update = HashUpdate { key: "stream".into(), bytes: vec![1, 2, 3] }.request();
        assert_eq!(rmp_serde::to_vec_named(&update).unwrap(), b"\x81\xaaHashUpdate\x82\xa3key\xa6stream\xa5bytes\xc4\x03\x01\x02\x03");
        let resize = Resize { width: 32, bytes: vec![4, 5] }.request();
        assert_eq!(rmp_serde::to_vec_named(&resize).unwrap(), b"\x81\xa6Resize\x82\xa5width\x20\xa5bytes\xc4\x02\x04\x05");
    }

    #[test]
    fn independent_streams_hash_their_own_bytes_and_release_sessions() {
        for key in ["first", "second"] {
            roundtrip(CpuRequest::HashStart { key: key.into() });
        }
        roundtrip(CpuRequest::HashUpdate { key: "first".into(), bytes: b"hello ".to_vec() });
        roundtrip(CpuRequest::HashUpdate { key: "second".into(), bytes: b"another book".to_vec() });
        roundtrip(CpuRequest::HashUpdate { key: "first".into(), bytes: b"world".to_vec() });
        for (key, bytes) in [("first", b"hello world".as_slice()), ("second", b"another book".as_slice())] {
            let CpuResponse::Hash(actual) = roundtrip(CpuRequest::HashFinish { key: key.into() }) else { panic!("expected digest") };
            assert_eq!(actual, blake3::hash(bytes).to_hex().as_str());
            assert!(execute(CpuRequest::HashUpdate { key: key.into(), bytes: Vec::new() }).is_err());
        }
    }

    #[test]
    fn lost_or_cancelled_hash_sessions_cannot_silently_start_again() {
        roundtrip(CpuRequest::HashStart { key: "cancel".into() });
        roundtrip(CpuRequest::HashAbort { key: "cancel".into() });
        assert!(execute(CpuRequest::HashFinish { key: "cancel".into() }).is_err());
        assert!(execute(CpuRequest::HashUpdate { key: "missing".into(), bytes: vec![1] }).is_err());
    }

    #[test]
    fn seekable_book_inspection_matches_direct_inspection() {
        let bytes = include_bytes!("../../backend/tests/fixtures/storage-contract.epub").to_vec();
        let actual = execute_book(BookTask::Inspect { source_name: "fixture.epub".into(), format: book_model::BookFormat::Epub }, Box::new(std::io::Cursor::new(bytes.clone()))).unwrap();
        let expected = CpuResponse::Inspection(book_metadata::inspect_book_bytes("fixture.epub", book_model::BookFormat::Epub, bytes).unwrap());
        assert_eq!(rmp_serde::to_vec_named(&actual).unwrap(), rmp_serde::to_vec_named(&expected).unwrap());
    }

    #[test]
    fn resize_transports_a_real_jpeg_at_the_requested_display_size() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(400, 600, image::Rgb([120, 40, 20]))).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let CpuResponse::Bytes(bytes) = roundtrip(CpuRequest::Resize { width: 300, bytes: png.into_inner() }) else { panic!("expected thumbnail") };
        assert_eq!(image::guess_format(&bytes).unwrap(), image::ImageFormat::Jpeg);
        assert_eq!(image::load_from_memory(&bytes).unwrap().to_rgb8().dimensions(), (300, 450));
    }
}

impl crate::thumbnail_jobs::ThumbnailProcessor for ThumbnailExtractor {
    fn decode(&self, bytes: Vec<u8>) -> crate::executor::BoxedBackendFuture<'_, Result<thumbnail::ThumbnailVersions, String>> {
        Box::pin(async move { cover(bytes).await.map(|v| thumbnail::ThumbnailVersions { browse: v.browse, high_density: v.high_density }) })
    }
    fn generate(&self, extension: String, reader: crate::BoxedBookReader) -> crate::executor::BoxedBackendFuture<'_, Result<Option<thumbnail::ThumbnailVersions>, String>> {
        Box::pin(async move { ThumbnailExtractor::generate(self, extension, reader).await.map(|v| v.map(|v| thumbnail::ThumbnailVersions { browse: v.browse, high_density: v.high_density })) })
    }
}
