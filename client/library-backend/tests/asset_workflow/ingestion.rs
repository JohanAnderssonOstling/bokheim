#![cfg(not(target_arch = "wasm32"))]

use std::io::Cursor;
use library_placement as asset_workflow;
use std::sync::Arc;

use client_platform_runtime::executor::BoxedBackendFuture;
use cpu_host::{BackendError, BookTask, BoxedCpuReader, CpuHost, CpuRequest, CpuResponse, ThumbnailBytes};
use library_database::{Database, ScanSnapshot};
use library_files::{asset_store::ImportOptions, AssetStore};
use sync_common::{BookId, ROOT_DIR_ID};

struct InspectHost;

impl CpuHost for InspectHost {
    fn book(&self, task: BookTask, mut reader: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async move {
            use std::io::{Read as _, Seek as _};
            let BookTask::Inspect { content_hash, .. } = &task else { panic!("unexpected task") };
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).unwrap();
            assert_eq!(*content_hash, Some(BookId::from(blake3::hash(&bytes))));
            reader.rewind().unwrap();
            cpu_host::execute_book_with_mobi(task, reader, |_, _| Err("subject page inspection is not supported for this format".into()))
        })
    }

    fn request(&self, _: CpuRequest) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async { panic!("unexpected request") })
    }

    fn generate_thumbnail(&self, _: String, _: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailBytes>, BackendError>> {
        Box::pin(async { panic!("unexpected thumbnail") })
    }
}

fn empty_snapshot() -> ScanSnapshot {
    ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] }
}

fn inspection_facts(inspection: &book_inspection::InspectedBook) -> serde_json::Value {
    let mut value = serde_json::to_value(inspection).unwrap();
    // Inspection assigns fresh database IDs; compare the extracted facts.
    fn remove_generated_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                fields.remove("contributor_id");
                for value in fields.values_mut() {
                    remove_generated_ids(value);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    remove_generated_ids(value);
                }
            }
            _ => {}
        }
    }
    remove_generated_ids(&mut value);
    value
}

fn pdf_bytes() -> Vec<u8> {
    use lopdf::{dictionary, Document, Object};
    let mut document = Document::with_version("1.5");
    let pages = document.new_object_id();
    let page = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()] });
    document.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
    let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

#[tokio::test]
async fn copied_imports_and_scanned_files_have_identical_ingestion_results() {
    let fixtures: Vec<(&str, &[u8])> =
        vec![("book.epub", include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub")), ("book.m4b", include_bytes!("../../crates/library-files/tests/fixtures/embedded-cover.m4b"))];
    let pdf = pdf_bytes();
    for (name, bytes) in fixtures.into_iter().chain(std::iter::once(("book.pdf", pdf.as_slice()))) {
        for direct in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let db = Database::open(root.path().join("library.db")).unwrap();
            db.initialize_library().unwrap();
            let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
            let host: Arc<dyn CpuHost> = Arc::new(InspectHost);
            let (name, copied) = asset_workflow::stage_import(&store, &db, ROOT_DIR_ID, name.into(), Box::new(Cursor::new(bytes.to_vec())), ImportOptions { direct }).await.unwrap();
            assert_eq!(std::fs::read(copied.path()).unwrap(), bytes, "copy must leave ingestion to the shared function");
            let ingested = filesystem_scanner::ingestion::ingest_file(copied.path().to_owned(), name.clone(), &*host).await.unwrap();
            let expected_hash = ingested.content_hash;
            let expected_size = ingested.size_bytes;
            let expected_metadata = inspection_facts(&ingested.inspection);
            let staged = copied.into_staged(expected_hash, expected_size);
            let published = asset_workflow::publish_import(&store, &db, ROOT_DIR_ID, &name, staged, false).await.unwrap();
            let published_bytes = std::fs::read(root.path().join(published.relative_path.trim_start_matches('/'))).unwrap();
            assert_eq!(expected_hash, BookId::from(blake3::hash(&published_bytes)));
            assert_eq!(expected_size, published_bytes.len() as u64);
            let snapshot = empty_snapshot();
            let scan = filesystem_scanner::filesystem_scan::discover(root.path(), &snapshot);
            let inspected = filesystem_scanner::filesystem_scan::inspect(root.path(), &scan, &snapshot, &host).await;
            assert!(inspected.readable);
            assert_eq!(inspected.books.len(), 1);
            let scanned = &inspected.books[0];
            assert_eq!(scanned.content_hash, expected_hash);
            assert_eq!(scanned.size_bytes, expected_size);
            assert_eq!(inspection_facts(&scanned.inspection), expected_metadata);
            assert_eq!(std::fs::read(root.path().join(published.relative_path.trim_start_matches('/'))).unwrap(), published_bytes);
        }
    }
}

#[tokio::test]
async fn failed_ingestion_cleans_up_copied_files() {
    for direct in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open(root.path().join("library.db")).unwrap();
        db.initialize_library().unwrap();
        let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let (name, copied) = asset_workflow::stage_import(&store, &db, ROOT_DIR_ID, "invalid.epub".into(), Box::new(Cursor::new(b"invalid epub")), ImportOptions { direct }).await.unwrap();
        let path = copied.path().to_owned();
        assert!(filesystem_scanner::ingestion::ingest_file(path.clone(), name, &InspectHost).await.is_err());
        drop(copied);
        assert!(!path.exists());
    }
}
