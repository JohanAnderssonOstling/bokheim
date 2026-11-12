//! CPU execution for library book and thumbnail work.
//! Portable messages and host interfaces are owned by `cpu-contract`.
pub use cpu_contract::*;

use content_address::BookId;
#[cfg(test)]
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::io::Cursor;
use std::io::{Read, Seek};
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod text_search;
#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeHost;

pub fn execute(request: CpuRequest) -> Result<CpuResponse, BackendError> {
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
            Ok(CpuResponse::Hash(BookId::from(hasher.finalize())))
        }),
        CpuRequest::HashAbort { key } => {
            HASHERS.with(|hashers| hashers.borrow_mut().remove(&key));
            Ok(CpuResponse::Done)
        }
        CpuRequest::Cover { bytes } => {
            let versions = cover_thumbnails(&bytes).map_err(BackendError::operation)?;
            Ok(CpuResponse::Thumbnail(Some(versions)))
        }
        CpuRequest::Resize { width, bytes } => {
            // The worker accepts only the product's supported display buckets.
            if ![thumbnail::BROWSE_THUMBNAIL_WIDTH, thumbnail::THUMBNAIL_WIDTH].contains(&width) {
                return Err("unsupported thumbnail width".into());
            }
            resize_cover(&bytes, width).map(CpuResponse::Bytes).map_err(BackendError::operation)
        }
    }
}

/// Both thumbnail sizes for an uploaded cover image.
fn cover_thumbnails(bytes: &[u8]) -> thumbnail::ThumbnailResult<thumbnail::ThumbnailVersions> {
    let format = image::guess_format(bytes)?;
    thumbnail::thumbnail_versions_from_image(&thumbnail::decode_cover_image(bytes, format)?)
}

/// A card derivative cropped and transformed at one thumbnail width.
fn resize_cover(bytes: &[u8], width: u32) -> thumbnail::ThumbnailResult<Vec<u8>> {
    let format = image::guess_format(bytes)?;
    thumbnail::encode_card_thumbnail_at_width(&thumbnail::decode_cover_image(bytes, format)?, width)
}

thread_local! {
    static HASHERS: std::cell::RefCell<std::collections::HashMap<String, blake3::Hasher>> = Default::default();
}

pub fn execute_book_with_mobi<R: Read + Seek + Send + Sync + 'static>(task: BookTask, reader: R, mobi_pages: fn(&book_model::BookRecord, &str) -> Result<book_model::BookMetadata, BackendError>) -> Result<CpuResponse, BackendError> {
    match task {
        BookTask::HashAudiobookContent {} => book_inspection::audiobook_content_hash(reader).map(|hash| CpuResponse::Hash(BookId::from(hash))).map_err(BackendError::operation),
        BookTask::TextMatches { format, locations, query } => text_search::previews(format, Box::new(reader), locations, &query).map(CpuResponse::TextMatches),
        BookTask::IndexText { format } => text_search::index(format, Box::new(reader)).map(CpuResponse::TextIndex),
        BookTask::Inspect { source_name, format, book_id, cached_pdf, declared_pdf } => {
            let path = Path::new(&source_name);
            if format != book_model::BookFormat::Mobi {
                return book_inspection::inspect_book_reader_with_pdf_metadata(path, format, reader, book_id, cached_pdf, declared_pdf).map(CpuResponse::Inspection).map_err(BackendError::operation);
            }
            book_inspection::inspect_mobi_with_frontmatter(path, reader, |book, text| mobi_pages(book, text).ok()).map(CpuResponse::Inspection).map_err(BackendError::operation)
        }
        BookTask::InspectEpubMetadata { source_name } => {
            let inspected = book_inspection::inspect_epub_metadata_reader(Path::new(&source_name), reader).map_err(BackendError::operation)?;
            Ok(CpuResponse::Inspection(book_inspection::InspectedBook { metadata: inspected.metadata, toc: inspected.toc, pdf: None, audiobook: None }))
        }
        BookTask::Thumbnail { extension } => thumbnail::generate_thumbnail_versions_with_pdf_renderer(&extension, reader, thumbnail::render_pdf_cover).map(CpuResponse::Thumbnail).map_err(BackendError::operation),
    }
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
    fn reader_epub_operation_roundtrips_and_omits_page_evidence() {
        let task = InspectEpubMetadata { source_name: "book.epub".into() }.task();
        let encoded = rmp_serde::to_vec_named(&task).unwrap();
        let task = rmp_serde::from_slice(&encoded).unwrap();
        let bytes = include_bytes!("../../book-inspection/tests/fixtures/back-matter-evidence.epub");
        let response = execute_book_with_mobi(task, Cursor::new(bytes.to_vec()), |_, _| panic!("EPUB metadata must not invoke MOBI inspection")).unwrap();
        let CpuResponse::Inspection(inspection) = response else { panic!("expected EPUB metadata") };
        assert_eq!(inspection.metadata.title, "Science History");
        assert!(inspection.metadata.book.identifiers.is_empty());
        assert!(inspection.pdf.is_none());
        assert!(inspection.audiobook.is_none());
    }

    #[test]
    fn shared_thumbnail_versions_keep_the_previous_worker_encoding() {
        #[derive(Serialize, Deserialize)]
        struct PreviousThumbnailBytes {
            #[serde(with = "serde_bytes")]
            browse: Vec<u8>,
            #[serde(with = "serde_bytes")]
            card_high_density: Vec<u8>,
            #[serde(with = "serde_bytes")]
            high_density: Vec<u8>,
        }

        let previous = PreviousThumbnailBytes { browse: vec![0, 128, 255], card_high_density: vec![1, 129, 254], high_density: vec![2, 130, 253] };
        for named in [false, true] {
            let original = if named { rmp_serde::to_vec_named(&previous).unwrap() } else { rmp_serde::to_vec(&previous).unwrap() };
            let versions: ThumbnailBytes = rmp_serde::from_slice(&original).unwrap();
            assert_eq!(versions.browse, previous.browse);
            assert_eq!(versions.card_high_density, previous.card_high_density);
            assert_eq!(versions.high_density, previous.high_density);
            let encoded = if named { rmp_serde::to_vec_named(&versions).unwrap() } else { rmp_serde::to_vec(&versions).unwrap() };
            assert_eq!(encoded, original);
            let decoded: PreviousThumbnailBytes = rmp_serde::from_slice(&encoded).unwrap();
            assert_eq!(decoded.browse, previous.browse);
            assert_eq!(decoded.card_high_density, previous.card_high_density);
            assert_eq!(decoded.high_density, previous.high_density);
        }
    }

    #[test]
    fn hashing_worker_returns_digest_bytes() {
        let key = "raw-digest-test".to_owned();
        assert!(matches!(roundtrip(CpuRequest::HashStart { key: key.clone() }), CpuResponse::Done));
        roundtrip(CpuRequest::HashUpdate { key: key.clone(), bytes: b"book bytes".to_vec() });
        let response = roundtrip(CpuRequest::HashFinish { key });
        let CpuResponse::Hash(hash) = &response else { panic!("expected hash") };
        assert_eq!(*hash, BookId::from(blake3::hash(b"book bytes")));
        let encoded = rmp_serde::to_vec_named(&response).unwrap();
        assert!(encoded.windows(32).any(|bytes| bytes == hash.as_bytes()));
        assert!(!encoded.windows(64).any(|bytes| bytes == hash.to_hex().as_bytes()));
    }

    #[test]
    fn cover_worker_transports_both_adjusted_card_densities_and_the_original_master() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(400, 400, image::Rgb([120, 40, 20]))).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let CpuResponse::Thumbnail(Some(versions)) = roundtrip(CpuRequest::Cover { bytes: png.into_inner() }) else { panic!("expected thumbnail versions") };
        assert_eq!(image::load_from_memory(&versions.browse).unwrap().to_rgb8().dimensions(), (300, 387));
        assert_eq!(image::load_from_memory(&versions.card_high_density).unwrap().to_rgb8().dimensions(), (600, 774));
        assert_eq!(image::load_from_memory(&versions.high_density).unwrap().to_rgb8().dimensions(), (600, 600));
    }

    #[test]
    fn resize_transports_a_real_jpeg_at_the_requested_display_size() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(400, 600, image::Rgb([120, 40, 20]))).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let CpuResponse::Bytes(bytes) = roundtrip(CpuRequest::Resize { width: 300, bytes: png.into_inner() }) else { panic!("expected thumbnail") };
        assert_eq!(image::guess_format(&bytes).unwrap(), image::ImageFormat::Jpeg);
        assert_eq!(image::load_from_memory(&bytes).unwrap().to_rgb8().dimensions(), (300, 480));
    }
}
