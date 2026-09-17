use crate::{AppBackend, AppDataLocation, BackendContext, DownloadState};
use book_model::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation};
use lopdf::Document;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use sync_common::{ContentHash, ROOT_DIR_ID};

// AppBackend initialization reaches process-global native PDFium state. Keep
// these integration cases isolated even when Rust's test harness is parallel.
static SCAN_TEST_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn test_scan() {
    let _guard = SCAN_TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let fixture = temp.path().join("library");
    std::fs::create_dir(&fixture).unwrap();
    let book = fixture.join("fixture.pdf");
    write_annotated_pdf(&book);
    let original_hash = raw_hash(&book);
    let context = BackendContext::initialize(AppDataLocation::native_path(temp.path().join("app"))).unwrap();
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("library".to_owned(), Some(fixture.to_string_lossy().into_owned())).unwrap();
    let library = app.library(entry.library_id()).unwrap();
    // This harness drives local file jobs itself instead of running AppClient.
    library.run_local_jobs().await.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while library.scanner_running() || library.book_count().unwrap() == 0 {
        assert!(std::time::Instant::now() < deadline, "fixture scan did not finish");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(library.book_count().unwrap(), 1);
    assert_eq!(book_identity::identify(&mut std::fs::File::open(&book).unwrap()).unwrap(), original_hash);
    assert_ne!(raw_hash(&book), original_hash, "first scan embeds the original identity");
    let document = Document::load(&book).unwrap();
    let page = *document.get_pages().values().next().unwrap();
    assert_eq!(document.get_object(page).unwrap().as_dict().unwrap().get(b"Annots").unwrap().as_array().unwrap().len(), 1);
}

use test_support::write_annotated_pdf;

fn raw_hash(path: &Path) -> ContentHash {
    ContentHash::new(blake3::hash(&std::fs::read(path).unwrap()).to_hex().as_str())
}

#[tokio::test(flavor = "current_thread")]
async fn pdf_annotations_use_a_sqlite_overlay_and_leave_book_bytes_pristine() {
    let _guard = SCAN_TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let app_data = temp.path().join("app");
    let library_root = temp.path().join("library");
    let source = temp.path().join("source.pdf");
    std::fs::create_dir_all(&library_root).unwrap();
    write_annotated_pdf(&source);
    book_identity::ensure(&source).unwrap();

    let context = BackendContext::initialize(AppDataLocation::native_path(app_data)).unwrap();
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("PDF annotations".to_owned(), Some(library_root.to_string_lossy().into_owned())).unwrap();
    let library = app.library(entry.library_id()).unwrap();
    let pristine = std::fs::read(&source).unwrap();
    let content_hash = library.import_book_path(&ROOT_DIR_ID, "source.pdf".to_owned(), &source).await.unwrap();
    let read_bytes = || async {
        let mut bytes = Vec::new();
        library.resolve_book_for_reader(content_hash).await.unwrap().into_document().unwrap().reader.read_to_end(&mut bytes).unwrap();
        bytes
    };
    assert_eq!(read_bytes().await, pristine);
    {
        let local = library.resolve_book_for_reader(content_hash).await.unwrap().into_document().unwrap();
        let metadata = local.pdf_metadata.expect("new imports reuse metadata for their opened local file");
        assert_eq!(metadata.checksum, raw_hash(&library_root.join("source.pdf")).to_string());
        assert_eq!(metadata.annotations.len(), 1);
        assert_eq!(metadata.annotations[0].note, "Existing interoperable comment");
    }
    assert!(library.annotations(content_hash.clone()).unwrap().is_empty(), "embedded PDF annotations are book content, not synchronized Bokheim annotations");

    let annotation = ReaderAnnotation {
        id: "bokheim-annotation".to_owned(),
        content_hash: content_hash.clone(),
        anchor: AnnotationAnchor::pdf(PdfAnnotationAnchor::new(0, vec![PdfAnnotationRect::new(0.15, 0.3, 0.35, 0.04)], None)),
        exact_text: "Bokheim selection".to_owned(),
        style: AnnotationStyle::Highlight,
        color: "#60a5fa".to_owned(),
        note: "Bokheim interoperable comment".to_owned(),
        created_at: 1,
        modified_at: 1,
    };
    library.upsert_annotation(&annotation).unwrap();
    assert_eq!(read_bytes().await, pristine);
    let stored = library.annotations(content_hash).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, "bokheim-annotation");
}

#[tokio::test(flavor = "current_thread")]
async fn read_only_pdf_asset_accepts_a_sqlite_annotation_overlay() {
    let _guard = SCAN_TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let app_data = temp.path().join("app");
    let library_root = temp.path().join("library");
    let source = temp.path().join("source.pdf");
    std::fs::create_dir_all(&library_root).unwrap();
    write_annotated_pdf(&source);
    let mut source_permissions = std::fs::metadata(&source).unwrap().permissions();
    source_permissions.set_readonly(true);
    std::fs::set_permissions(&source, source_permissions).unwrap();
    assert_eq!(book_identity::ensure(&source).unwrap(), raw_hash(&source));
    assert!(book_identity::read(&mut std::fs::File::open(&source).unwrap()).unwrap().is_none());

    let context = BackendContext::initialize(AppDataLocation::native_path(app_data)).unwrap();
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("Read-only PDF".to_owned(), Some(library_root.to_string_lossy().into_owned())).unwrap();
    let library = app.library(entry.library_id()).unwrap();
    let content_hash = library.import_book_path(&ROOT_DIR_ID, "source.pdf".to_owned(), &source).await.unwrap();
    let asset_path = library_root.join("source.pdf");
    let pristine_hash = raw_hash(&asset_path);
    let mut permissions = std::fs::metadata(&asset_path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&asset_path, permissions).unwrap();

    let annotation = ReaderAnnotation {
        id: "read-only-overlay".to_owned(),
        content_hash: content_hash.clone(),
        anchor: AnnotationAnchor::pdf(PdfAnnotationAnchor::new(0, vec![PdfAnnotationRect::new(0.1, 0.1, 0.2, 0.03)], None)),
        exact_text: "Overlay selection".to_owned(),
        style: AnnotationStyle::Underline,
        color: "#4ade80".to_owned(),
        note: "Stored outside the read-only PDF".to_owned(),
        created_at: 1,
        modified_at: 1,
    };
    library.upsert_annotation(&annotation).unwrap();

    assert_eq!(raw_hash(&asset_path), pristine_hash, "annotation edits must not rewrite the PDF");
    assert_eq!(book_identity::identify(&mut std::fs::File::open(&asset_path).unwrap()).unwrap(), content_hash);
    let annotations = library.annotations(content_hash).unwrap();
    assert_eq!(annotations.iter().filter(|candidate| candidate.id == "read-only-overlay").count(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn reader_preparation_uses_local_content_or_queues_its_download() {
    let _guard = SCAN_TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let app_data = temp.path().join("app");
    let library_root = temp.path().join("library");
    let source = temp.path().join("source.pdf");
    std::fs::create_dir_all(&library_root).unwrap();
    write_annotated_pdf(&source);
    book_identity::ensure(&source).unwrap();

    let context = BackendContext::initialize(AppDataLocation::native_path(app_data)).unwrap();
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("Reader preparation".to_owned(), Some(library_root.to_string_lossy().into_owned())).unwrap();
    let library = app.library(entry.library_id()).unwrap();
    let expected = std::fs::read(&source).unwrap();
    let content_hash = library.import_book_path(&ROOT_DIR_ID, "source.pdf".to_owned(), &source).await.unwrap();

    let mut prepared = app.prepare_book_for_reader(&library, content_hash.clone()).await.unwrap();
    let mut actual = Vec::new();
    prepared.into_document().unwrap().reader.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, expected);

    assert!(matches!(library.remove_local_book_copy(content_hash.clone()).unwrap(), DownloadState::NotDownloaded));
    let error = match app.prepare_book_for_reader(&library, content_hash.clone()).await {
        Ok(_) => panic!("a missing remote book must not appear ready"),
        Err(error) => error,
    };
    assert_eq!(error.to_string(), "sign in to read this remote file");
}
