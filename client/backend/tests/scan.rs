use bokheim_backend::{AppBackend, AppDataLocation, BackendContext};
use library_model::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;
use std::sync::Mutex;
use sync_common::{ContentHash, ROOT_DIR_ID};

// AppBackend initialization reaches process-global native PDFium state. Keep
// these integration cases isolated even when Rust's test harness is parallel.
static SCAN_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
pub fn test_scan() {
    let _guard = SCAN_TEST_LOCK.lock().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().to_path_buf())).unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test_folder");
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("library".to_owned(), Some(fixture)).unwrap();
    let library = app.library(entry.library_id()).unwrap();
    assert_eq!(library.id(), entry.library_id());
}

fn write_annotated_pdf(path: &Path) {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let content_id = document.add_object(Stream::new(dictionary! {}, b"BT ET".to_vec()));
    let existing_annotation = document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Highlight",
        "Rect" => vec![60.into(), 600.into(), 240.into(), 632.into()],
        "QuadPoints" => vec![60.into(), 632.into(), 240.into(), 632.into(), 60.into(), 600.into(), 240.into(), 600.into()],
        "C" => vec![Object::Real(1.0), Object::Real(0.0), Object::Real(0.0), Object::Real(0.0)],
        "CA" => Object::Real(0.55),
        "Contents" => Object::string_literal("Existing interoperable comment"),
        "T" => Object::string_literal("Other reader"),
        "NM" => Object::string_literal("other-reader-annotation"),
    });
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
        "Contents" => content_id,
        "Resources" => dictionary! {},
        "Annots" => vec![existing_annotation.into()],
    });
    document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document.save(path).unwrap();
}

fn raw_hash(path: &Path) -> ContentHash {
    ContentHash::new(blake3::hash(&std::fs::read(path).unwrap()).to_hex().as_str())
}

#[test]
fn pdf_annotations_preserve_a_pristine_base_and_use_standard_color_fields() {
    let _guard = SCAN_TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let app_data = temp.path().join("app");
    let library_root = temp.path().join("library");
    let source = temp.path().join("source.pdf");
    std::fs::create_dir_all(&library_root).unwrap();
    write_annotated_pdf(&source);

    let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data)).unwrap();
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("PDF annotations".to_owned(), Some(library_root.clone())).unwrap();
    let library = app.library(entry.library_id()).unwrap();
    let content_hash = library.import_book(&ROOT_DIR_ID, &source).unwrap();
    let working_path = Path::new(&library.book_asset_path(content_hash.clone()).unwrap()).to_path_buf();

    let imported = library.annotations(content_hash.clone()).unwrap();
    assert_eq!(imported.len(), 1);
    assert!(imported[0].is_external_pdf_annotation());
    assert_eq!(imported[0].color, "#00ffff", "CMYK /C must be converted for the UI");
    assert_eq!(imported[0].note, "Existing interoperable comment");

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

    let base_path = library_root.join(".bokheim/staging/pdf-bases").join(format!("{}.pdf", content_hash.as_str()));
    assert!(base_path.is_file());
    assert_eq!(raw_hash(&base_path), content_hash);
    assert_ne!(raw_hash(&working_path), content_hash, "the annotated working PDF must be separate from its base blob");

    let document = Document::load(&working_path).unwrap();
    let root_id = document.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let marker = document.get_object(root_id).unwrap().as_dict().unwrap().get(b"BokheimContentHash").unwrap().as_str().unwrap();
    assert_eq!(marker, content_hash.as_str().as_bytes());
    let page_id = *document.get_pages().values().next().unwrap();
    let page = document.get_object(page_id).unwrap().as_dict().unwrap();
    let annotations = page.get_deref(b"Annots", &document).unwrap().as_array().unwrap();
    let bokheim = annotations
        .iter()
        .filter_map(|object| document.dereference(object).ok().map(|(_, object)| object))
        .filter_map(|object| object.as_dict().ok())
        .find(|dictionary| dictionary.get(b"NM").ok().and_then(|value| value.as_str().ok()) == Some(b"bokheim:bokheim-annotation".as_slice()))
        .unwrap();
    assert_eq!(bokheim.get(b"Subtype").unwrap().as_name().unwrap(), b"Highlight");
    let color = bokheim.get(b"C").unwrap().as_array().unwrap().iter().map(|value| value.as_float().unwrap()).collect::<Vec<_>>();
    assert!((color[0] - 0x60 as f32 / 255.0).abs() < 0.001);
    assert!((color[1] - 0xa5 as f32 / 255.0).abs() < 0.001);
    assert!((color[2] - 0xfa as f32 / 255.0).abs() < 0.001);
    assert!((bokheim.get(b"CA").unwrap().as_float().unwrap() - 0.38).abs() < 0.001);

    let merged = library.annotations(content_hash).unwrap();
    assert_eq!(merged.iter().filter(|annotation| annotation.is_external_pdf_annotation()).count(), 1);
    assert_eq!(merged.iter().filter(|annotation| annotation.id == "bokheim-annotation").count(), 1);
}

#[test]
fn read_only_pdf_uses_a_sqlite_overlay_without_modifying_the_file() {
    let _guard = SCAN_TEST_LOCK.lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let app_data = temp.path().join("app");
    let library_root = temp.path().join("library");
    let source = temp.path().join("source.pdf");
    std::fs::create_dir_all(&library_root).unwrap();
    write_annotated_pdf(&source);

    let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data)).unwrap();
    let app = AppBackend::new(context).unwrap();
    let entry = app.create_library("Read-only PDF".to_owned(), Some(library_root.clone())).unwrap();
    let library = app.library(entry.library_id()).unwrap();
    let content_hash = library.import_book(&ROOT_DIR_ID, &source).unwrap();
    let working_path = Path::new(&library.book_asset_path(content_hash.clone()).unwrap()).to_path_buf();
    let mut permissions = std::fs::metadata(&working_path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&working_path, permissions).unwrap();

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

    assert_eq!(raw_hash(&working_path), content_hash);
    assert!(!library_root.join(".bokheim/staging/pdf-bases").join(format!("{}.pdf", content_hash.as_str())).exists());
    let document = Document::load(&working_path).unwrap();
    let root_id = document.trailer.get(b"Root").unwrap().as_reference().unwrap();
    assert!(!document.get_object(root_id).unwrap().as_dict().unwrap().has(b"BokheimContentHash"));
    let annotations = library.annotations(content_hash).unwrap();
    assert_eq!(annotations.iter().filter(|candidate| candidate.id == "read-only-overlay").count(), 1);
}
