#![cfg(not(target_arch = "wasm32"))]
use book_inspection::{InspectedBook, PdfSourceIdentity};
use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookDetails, BookFormat, BookRecord, BookTocEntry, PdfAnnotationAnchor, PdfAnnotationRect};
use content_address::{BookId, ContentHash, hash_file};
use library_annotation_export::{export, library_destination};
use library_database::{DatabaseClient, ImportCommit};
use library_files::AssetStore;
use library_model::*;
use lopdf::{Document, Object, decode_text_string, dictionary};
use md5::{Digest, Md5};
use std::{
    fs::{self, File},
    io::{Cursor, Write},
    path::PathBuf,
};
use sync_common::{LibraryId, ROOT_DIR_ID};
use zip::{ZipWriter, write::SimpleFileOptions};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    destination: PathBuf,
    db: DatabaseClient,
    assets: AssetStore,
    book: BookId,
    canonical: ContentHash,
    library: LibraryId,
    note: Annotation,
}
impl Fixture {
    async fn new(uploaded: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        fs::create_dir(&root).unwrap();
        let destination = library_destination(&temp.path().join("exports"), "My library");
        let library = LibraryId::new_v4();
        let mut document = Document::with_version("1.7");
        let pages = document.new_object_id();
        let external =
            document.add_object(dictionary! {"Type"=>"Annot","Subtype"=>"Text","Rect"=>vec![10.into(),10.into(),30.into(),30.into()],"NM"=>Object::string_literal("some-other-reader"),"Contents"=>Object::string_literal("Keep me")});
        let page = document.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"Annots"=>vec![Object::Reference(external)]});
        document.objects.insert(
            pages,
            dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1,"MediaBox"=>vec![0.into(),0.into(),600.into(),800.into()],"CropBox"=>vec![20.into(),30.into(),580.into(),770.into()],"Rotate"=>90}.into(),
        );
        let catalog = document.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        document.trailer.set("Root", catalog);
        document.save(root.join("Original.pdf")).unwrap();
        let canonical = hash_file(&mut File::open(root.join("Original.pdf")).unwrap()).unwrap();
        let size = fs::metadata(root.join("Original.pdf")).unwrap().len();
        let book = BookId::for_local_content(library.as_bytes(), canonical);
        let db = DatabaseClient::open(temp.path().join("metadata.sqlite")).unwrap();
        db.initialize_library_for(library).await.unwrap();
        db.commit_import(ImportCommit {
            source_identity: None,
            byte_checksum: Some(canonical),
            audiobook_content_hash: None,
            parent_id: ROOT_DIR_ID,
            file_name: "Original.pdf".into(),
            format: BookFormat::Pdf,
            book_id: book,
            size_bytes: size,
            inspection_version: 1,
            inspection: Some(InspectedBook {
                metadata: BookRecord { metadata: BookDetails { title: "A Book".into(), ..Default::default() }, description: String::new().await },
                pdf: None,
                audiobook: None,
                toc: vec![BookTocEntry { title: "Part I".into(), target: "pdf-page:0".into(), children: vec![BookTocEntry { title: "Chapter One".into(), target: "pdf-page:0".into(), children: vec![] }] }],
            }),
            published: PublishedStagedBook { name: "Original.pdf".into(), relative_path: "/Original.pdf".into(), published: true, fingerprint: Some("first".into()) },
            request_thumbnail: false,
            restore_paths: vec![],
        })
        .await
        .unwrap();
        let note = Annotation {
            id: "test-note".into(),
            book_id: book,
            anchor: AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(0, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.1)], None).await),
            exact_text: "Quoted text\n\nSecond line".into(),
            style: AnnotationStyle::Highlight,
            color: 0xf6c945,
            note: "My note: åäö".into(),
            created_at: 1,
            toc_ordinal: Some(1),
            progress: Some(10.0),
            modified_at: 2,
            deleted: false,
        };
        db.upsert_annotation(&note).await.unwrap();
        if uploaded {
            db.confirm_uploaded_content_with_pdf(book, canonical, Some(size), None).await.unwrap();
        }
        let assets = AssetStore::open(root.to_str().unwrap()).unwrap();
        Self { _temp: temp, root, destination, db, assets, book, canonical, library, note }
    }
    fn options(&self, formats: Vec<AnnotationExportFormat>) -> AnnotationExportOptions {
        AnnotationExportOptions { destination: self.destination.clone(), formats, install_readers: false, conflict: AnnotationExportConflict::KeepBoth, overrides: vec![] }
    }
    fn outbox(&self) -> i64 {
        rusqlite::Connection::open(self._temp.path().join("metadata.sqlite")).unwrap().query_row("SELECT count(*) FROM sync_outbox", [], |row| row.get(0)).unwrap()
    }
    async fn run(&self, formats: Vec<AnnotationExportFormat>) -> AnnotationExportReport {
        export(&self.db, &self.assets, self.library, self.options(formats)).await.unwrap()
    }
}

#[tokio::test]
async fn pdf_and_markdown_are_local_preserve_identity_and_describe_the_resulting_bytes() {
    let f = Fixture::new(true).await;
    let outbox = f.outbox();
    let report = f.run(vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Pdf]).await;
    assert_eq!(report.results.len(), 2);
    assert!(report.results.iter().all(|r| matches!(r.outcome, AnnotationExportOutcome::Written)), "{report:?}");
    let changed = hash_file(&mut File::open(f.root.join("Original.pdf")).unwrap()).unwrap();
    assert_ne!(changed, f.canonical);
    assert_eq!(f.db.canonical_content_hash(f.book).await.unwrap(), Some(f.canonical));
    assert_eq!(f.db.book_for_local_content(changed).await.unwrap(), Some(f.book));
    assert_eq!(f.db.annotations(f.book).await.unwrap(), vec![f.note.clone()]);
    assert_eq!(f.outbox(), outbox, "exports must not author synchronized state");
    let text = fs::read_to_string(f.destination.join("A Book.md")).unwrap();
    assert!(text.starts_with("---\ntitle: \"A Book\""));
    assert!(text.contains(&format!("local_content_hash: \"{changed}\"")));
    assert!(text.contains(&format!("canonical_content_hash: \"{}\"", f.canonical)));
    assert!(text.contains("## Part I\n\n### Chapter One\n\n> *Quoted text*\n>\n> *Second line*\n\nMy note: åäö"));
    assert!(text.contains("[A Book — p. 1](bokheim://open?"));
    let pdf = Document::load(f.root.join("Original.pdf")).unwrap();
    assert_eq!(PdfSourceIdentity::from_document(&pdf), Some(PdfSourceIdentity { book_id: f.book, canonical_hash: Some(f.canonical) }));
    let page = *pdf.get_pages().values().next().unwrap();
    let annots = pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap().as_array().unwrap();
    assert_eq!(annots.len(), 2);
    let marks = annots.iter().map(|v| pdf.get_object(v.as_reference().unwrap()).unwrap().as_dict().unwrap()).collect::<Vec<_>>();
    assert!(marks.iter().any(|a| a.get(b"NM").unwrap().as_str().unwrap() == b"some-other-reader"));
    let own = marks.iter().find(|a| a.get(b"NM").unwrap().as_str().unwrap() == b"bokheim:test-note").unwrap();
    // Inherited crop and 90-degree rotation: screen (0.1,0.2) -> (132,104).
    let quad = own.get(b"QuadPoints").unwrap().as_array().unwrap();
    assert!((quad[0].as_float().unwrap() - 132.0).abs() < 0.001);
    assert!((quad[1].as_float().unwrap() - 104.0).abs() < 0.001);
    assert!(own.get(b"AP").is_ok());
    let repeat = f.run(vec![AnnotationExportFormat::Pdf]).await;
    assert!(matches!(repeat.results[0].outcome, AnnotationExportOutcome::Written));
    let pdf = Document::load(f.root.join("Original.pdf")).unwrap();
    assert_eq!(pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap().as_array().unwrap().len(), 2);
    f.db.delete_annotation(&f.note.id, 3).await.unwrap();
    f.run(vec![AnnotationExportFormat::Pdf]).await;
    let pdf = Document::load(f.root.join("Original.pdf")).unwrap();
    assert_eq!(pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap().as_array().unwrap().len(), 1, "deleted Bokheim marks are removed, external marks retained");
}

#[tokio::test]
async fn a_pre_upload_pdf_revision_is_queued_as_first_canonical_content() {
    let f = Fixture::new(false).await;
    let outbox = f.outbox();
    f.run(vec![AnnotationExportFormat::Pdf]).await;
    let changed = f.db.local_content_hash(f.book, ROOT_DIR_ID).await.unwrap().unwrap();
    assert_ne!(changed, f.canonical);
    assert_eq!(f.db.canonical_content_hash(f.book).await.unwrap(), None);
    let connection = rusqlite::Connection::open(f._temp.path().join("metadata.sqlite")).unwrap();
    let queued: ContentHash = connection.query_row("SELECT upload_checksum FROM book_transfer WHERE content_hash=?1", [f.book], |row| row.get(0)).unwrap();
    assert_eq!(queued, changed);
    assert_eq!(f.outbox(), outbox);
    f.db.confirm_uploaded_content_with_pdf(f.book, changed, None, None).await.unwrap();
    assert_eq!(f.db.canonical_content_hash(f.book).await.unwrap(), Some(changed));
}

#[tokio::test]
async fn failures_preserve_originals_and_other_outputs_can_finish() {
    let f = Fixture::new(true).await;
    let original = fs::read(f.root.join("Original.pdf")).unwrap();
    let mut invalid = f.note.clone();
    invalid.anchor = AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(30, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.1)], None).await);
    invalid.modified_at = 3;
    f.db.upsert_annotation(&invalid).await.unwrap();
    let report = f.run(vec![AnnotationExportFormat::Pdf, AnnotationExportFormat::Markdown]).await;
    assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Failed(_)));
    assert!(matches!(report.results[1].outcome, AnnotationExportOutcome::Written));
    assert_eq!(fs::read(f.root.join("Original.pdf")).unwrap(), original);
}

#[tokio::test]
async fn output_conflicts_are_explicit_and_exports_cannot_enter_the_library() {
    let f = Fixture::new(true).await;
    f.run(vec![AnnotationExportFormat::Markdown]).await;
    fs::write(f.destination.join("A Book.md"), "user work").unwrap();
    f.run(vec![AnnotationExportFormat::Markdown]).await;
    assert_eq!(fs::read_to_string(f.destination.join("A Book.md")).unwrap(), "user work");
    assert!(f.destination.join("A Book (2).md").exists());
    let mut options = f.options(vec![AnnotationExportFormat::Markdown]);
    options.conflict = AnnotationExportConflict::Skip;
    let report = export(&f.db, &f.assets, f.library, options.clone()).await.unwrap();
    assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Skipped));
    options.conflict = AnnotationExportConflict::Replace;
    export(&f.db, &f.assets, f.library, options.clone()).await.unwrap();
    assert!(fs::read_to_string(f.destination.join("A Book.md")).unwrap().starts_with("---"));
    options.destination = f.root.join("exports");
    assert!(export(&f.db, &f.assets, f.library, options).await.is_err());
    assert!(!f.root.join("exports").exists());
    #[cfg(unix)]
    {
        let link = f._temp.path().join("disguised");
        std::os::unix::fs::symlink(&f.root, &link).unwrap();
        let mut options = f.options(vec![AnnotationExportFormat::Markdown]);
        options.destination = link.join("exports");
        assert!(export(&f.db, &f.assets, f.library, options).await.is_err());
    }
}

#[tokio::test]
async fn foliate_export_uses_the_original_epub_identifier() {
    let bytes = include_bytes!("../../../tests/fixtures/cloud-cover.epub");
    let publication = book_content::Book::open_reader(book_content::BookFormat::Epub, Cursor::new(bytes.to_vec()).await).unwrap();
    let identifier = publication.publication_identifier().expect("fixture publication identifier");
    assert_foliate_export(bytes, &identifier).await;
}

#[tokio::test]
async fn foliate_export_uses_the_file_identifier_when_the_epub_has_none() {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new().await).await).await;
    archive.start_file("book.opf", SimpleFileOptions::default()).unwrap();
    archive.write_all(br#"<package><metadata/><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#).unwrap();
    archive.start_file("chapter.xhtml", SimpleFileOptions::default()).unwrap();
    archive.write_all(b"<html><body><p>A chapter</p></body></html>").unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let identifier = format!("foliate:{:x}", Md5::digest(&bytes));
    assert_foliate_export(&bytes, &identifier).await;
}

async fn assert_foliate_export(bytes: &[u8], identifier: &str) {
    let f = Fixture::new(true).await;
    fs::write(f.root.join("Book.epub"), bytes).unwrap();
    let checksum = ContentHash::from(blake3::hash(bytes));
    let book = BookId::for_local_content(f.library.as_bytes(), checksum);
    f.db.commit_import(ImportCommit {
        source_identity: None,
        byte_checksum: Some(checksum),
        audiobook_content_hash: None,
        parent_id: ROOT_DIR_ID,
        file_name: "Book.epub".into(),
        format: BookFormat::Epub,
        book_id: book,
        size_bytes: bytes.len() as u64,
        inspection_version: 1,
        inspection: Some(InspectedBook { metadata: BookRecord { metadata: BookDetails { title: "EPUB Book".into(), ..Default::default() }, description: String::new().await }, pdf: None, audiobook: None, toc: vec![] }),
        published: PublishedStagedBook { name: "Book.epub".into(), relative_path: "/Book.epub".into(), published: true, fingerprint: Some("epub".into()) },
        request_thumbnail: false,
        restore_paths: vec![],
    })
    .await
    .unwrap();
    let publication = book_content::Book::open_reader(book_content::BookFormat::Epub, File::open(f.root.join("Book.epub")).unwrap()).unwrap();
    let reading = publication.prepare_reading().await.unwrap();
    let base = reading.publication_cfi_bases().unwrap()[0].clone().unwrap();
    let mut note = f.note.clone();
    note.book_id = book;
    note.id = "epub-note".into();
    note.anchor = AnnotationAnchor::epub_publication_cfi(format!("epubcfi({base}!/4/2,/1:0,/1:1)"));
    f.db.upsert_annotation(&note).await.unwrap();
    let outbox = f.outbox();
    let report = f.run(vec![AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest]).await;
    assert_eq!(report.results.len(), 2);
    assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
    let data: serde_json::Value = serde_json::from_slice(&fs::read(f.destination.join("EPUB Book.foliate.json")).unwrap()).unwrap();
    assert_eq!(data["metadata"]["identifier"], identifier);
    assert_eq!(data["annotations"][0]["value"], format!("epubcfi({base}!/4/2,/1:0,/1:1)"));
    let readest_path = f.destination.join("EPUB Book.readest.json");
    let data: serde_json::Value = serde_json::from_slice(&fs::read(&readest_path).unwrap()).unwrap();
    assert_eq!(data["$format"], "readest-annotations");
    assert_eq!(data["annotations"][0]["cfi"], format!("epubcfi({base}!/4/2,/1:0,/1:1)"));
    assert_eq!(data["annotations"][0]["id"], note.id);
    assert_eq!(data["annotations"][0]["note"], note.note);
    assert_eq!(f.db.annotations(book).await.unwrap(), vec![note], "export conversion must not migrate/synchronize reader state");
    assert_eq!(f.outbox(), outbox);
    assert_eq!(fs::read(f.root.join("Book.epub")).unwrap(), bytes);

    // Once locations use full publication CFIs, Readest needs no local book or
    // device-specific hash. Legacy conversions above only touched the export.
    let mut full = f.db.annotations(book).await.unwrap().remove(0);
    full.anchor = AnnotationAnchor::epub_publication_cfi(format!("epubcfi({base}!/4/2,/1:0,/1:1)"));
    full.style = AnnotationStyle::Strikethrough;
    full.modified_at += 1;
    f.db.upsert_annotation(&full).await.unwrap();
    fs::rename(&f.root, f._temp.path().join("unmounted-library")).unwrap();
    let outbox = f.outbox();
    let mut options = f.options(vec![AnnotationExportFormat::Readest]);
    options.conflict = AnnotationExportConflict::Replace;
    let report = export(&f.db, &f.assets, f.library, options).await.unwrap();
    assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Written), "{report:?}");
    assert_eq!(report.results[0].destination.path, readest_path);
    let data: serde_json::Value = serde_json::from_slice(&fs::read(&readest_path).unwrap()).unwrap();
    assert_eq!(data["annotations"][0]["style"], "underline");
    assert_eq!(f.outbox(), outbox);
    assert_eq!(f.db.annotations(book).await.unwrap(), vec![full]);
    assert!(!f.destination.join("EPUB Book (2).readest.json").exists());
}

#[tokio::test]
async fn markdown_works_when_the_library_files_are_temporarily_unavailable() {
    let f = Fixture::new(true).await;
    fs::rename(&f.root, f._temp.path().join("unmounted-library")).unwrap();
    let report = f.run(vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Pdf]).await;
    assert!(report.results.iter().any(|result| matches!(result.outcome, AnnotationExportOutcome::Written)));
    assert!(report.results.iter().any(|result| matches!(result.outcome, AnnotationExportOutcome::Failed(_))));
    assert!(f.destination.join("A Book.md").exists());
}

#[tokio::test]
async fn writing_a_recovered_pdf_preserves_its_unconfirmed_original_hash_hint() {
    let f = Fixture::new(false).await;
    let mut document = Document::load(f.root.join("Original.pdf")).unwrap();
    PdfSourceIdentity { book_id: f.book, canonical_hash: Some(f.canonical) }.write(&mut document).unwrap();
    let mut input = Vec::new().await;
    document.save_to(&mut input).unwrap();
    let mut output = Vec::new().await;
    book_inspection::prepare_pdf_annotation_export(std::io::Cursor::new(input), f.book, &[]).unwrap().write(&mut output, PdfSourceIdentity { book_id: f.book, canonical_hash: None }, &[f.note.clone()]).unwrap();
    let document = Document::load_mem(&output).unwrap();
    assert_eq!(PdfSourceIdentity::from_document(&document).unwrap().canonical_hash, Some(f.canonical));
}

async fn add_epub(f: &Fixture) -> (BookId, Annotation) {
    let bytes = include_bytes!("../../../tests/fixtures/cloud-cover.epub");
    fs::write(f.root.join("Book.epub"), bytes).unwrap();
    let checksum = ContentHash::from(blake3::hash(bytes));
    let book = BookId::for_local_content(f.library.as_bytes(), checksum);
    f.db.commit_import(ImportCommit {
        source_identity: None,
        byte_checksum: Some(checksum),
        audiobook_content_hash: None,
        parent_id: ROOT_DIR_ID,
        file_name: "Book.epub".into(),
        format: BookFormat::Epub,
        book_id: book,
        size_bytes: bytes.len() as u64,
        inspection_version: 1,
        inspection: Some(InspectedBook { metadata: BookRecord { metadata: BookDetails { title: "EPUB Book".into(), ..Default::default() }, description: String::new().await }, pdf: None, audiobook: None, toc: vec![] }),
        published: PublishedStagedBook { name: "Book.epub".into(), relative_path: "/Book.epub".into(), published: true, fingerprint: Some("epub".into()) },
        request_thumbnail: false,
        restore_paths: vec![],
    })
    .await
    .unwrap();
    let mut note = f.note.clone();
    note.id = "epub-note".into();
    note.book_id = book;
    note.anchor = AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:1)");
    f.db.upsert_annotation(&note).await.unwrap();
    (book, note)
}

async fn replace(f: &Fixture, formats: Vec<AnnotationExportFormat>) -> AnnotationExportReport {
    let mut options = f.options(formats);
    options.conflict = AnnotationExportConflict::Replace;
    export(&f.db, &f.assets, f.library, options).await.unwrap()
}

#[tokio::test]
async fn external_annotations_stay_in_their_destinations_and_known_exports_are_updated() {
    let f = Fixture::new(true).await;
    let (book, original) = add_epub(&f).await;
    let formats = vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest];
    replace(&f, formats.clone()).await;
    let foliate_path = f.destination.join("EPUB Book.foliate.json");
    let readest_path = f.destination.join("EPUB Book.readest.json");
    let mut foliate: serde_json::Value = serde_json::from_slice(&fs::read(&foliate_path).unwrap()).unwrap();
    foliate["annotations"][0]["note"] = "Edited in Foliate".into();
    foliate["metadata"]["custom"] = "keep metadata".into();
    foliate["bookmarks"] = serde_json::json!([{"value":"epubcfi(/6/2!/4/4)","label":"keep bookmark"}]);
    foliate["custom"] = "keep envelope".into();
    foliate["annotations"].as_array_mut().unwrap().push(serde_json::json!({
        "value":"epubcfi(/6/2!/4/4,/1:0,/1:7)", "color":"#123456", "text":"Foreign Foliate quote", "note":"Foreign Foliate note",
        "created":"2026-10-01T12:00:00Z","modified":"2026-10-01T12:01:00Z", "custom":"keep annotation"
    }));
    fs::write(&foliate_path, serde_json::to_vec(&foliate).unwrap()).unwrap();
    let mut readest: serde_json::Value = serde_json::from_slice(&fs::read(&readest_path).unwrap()).unwrap();
    readest["annotations"].as_array_mut().unwrap().push(serde_json::json!({
        "id":"foreign-readest-id", "type":"annotation", "cfi":"epubcfi(/6/2!/4/6,/1:0,/1:8)",
        "color":"#abcdef","style":"underline","text":"Foreign Readest quote","note":"Foreign Readest note", "createdAt":100, "updatedAt":200
    }));
    fs::write(&readest_path, serde_json::to_vec(&readest).unwrap()).unwrap();
    let watermark = f.db.sync_outbox_watermark().await.unwrap();
    let report = replace(&f, formats.clone()).await;
    assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
    let annotations = f.db.annotations(book).await.unwrap();
    assert_eq!(annotations, vec![original.clone()]);
    assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
    let markdown = fs::read_to_string(f.destination.join("EPUB Book.md")).unwrap();
    assert!(markdown.contains(&original.note));
    assert!(!markdown.contains("Foreign Foliate quote"));
    assert!(!markdown.contains("Foreign Readest note"));
    let data: serde_json::Value = serde_json::from_slice(&fs::read(&foliate_path).unwrap()).unwrap();
    assert_eq!(data["annotations"].as_array().unwrap().len(), 2);
    assert_eq!(data["bookmarks"], foliate["bookmarks"]);
    assert_eq!(data["metadata"]["custom"], "keep metadata");
    assert_eq!(data["custom"], "keep envelope");
    assert!(data["annotations"].as_array().unwrap().iter().any(|record| record["custom"] == "keep annotation"));
    // Without ownership markers an existing highlight must not be overwritten.
    let mut stripped: serde_json::Value = serde_json::from_slice(&fs::read(&foliate_path).unwrap()).unwrap();
    stripped.as_object_mut().unwrap().remove("_bokheim_book_id");
    stripped.as_object_mut().unwrap().remove("_bokheim_annotation_ids");
    for record in stripped["annotations"].as_array_mut().unwrap() {
        record.as_object_mut().unwrap().remove("_bokheim_id");
    }
    stripped["annotations"].as_array_mut().unwrap().iter_mut().find(|record| record["note"] == original.note).unwrap()["note"] = "Round trip through Foliate".into();
    let stripped_bytes = serde_json::to_vec(&stripped).unwrap();
    fs::write(&foliate_path, &stripped_bytes).unwrap();
    let report = replace(&f, formats.clone()).await;
    assert!(report.results.iter().any(|result| result.destination.format == AnnotationExportFormat::Foliate && matches!(result.outcome, AnnotationExportOutcome::Failed(_))), "{report:?}");
    assert_eq!(fs::read(&foliate_path).unwrap(), stripped_bytes);
    fs::write(&foliate_path, serde_json::to_vec(&data).unwrap()).unwrap();
    let annotations = f.db.annotations(book).await.unwrap();
    assert_eq!(annotations.iter().find(|annotation| annotation.id == original.id).unwrap(), &original);
    let watermark = f.db.sync_outbox_watermark().await.unwrap();
    replace(&f, formats.clone()).await;
    assert_eq!(f.db.annotations(book).await.unwrap(), annotations);
    assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark, "repeat exports must not author changes");
    for annotation in &annotations {
        f.db.delete_annotation(&annotation.id, 9999).await.unwrap();
    }
    let report = replace(&f, formats).await;
    assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
    assert!(f.db.annotations(book).await.unwrap().is_empty(), "stale file copies cannot restore deletions");
    let data: serde_json::Value = serde_json::from_slice(&fs::read(&foliate_path).unwrap()).unwrap();
    assert_eq!(data["annotations"].as_array().unwrap().len(), 1);
    assert_eq!(data["annotations"][0]["note"], "Foreign Foliate note");
}

#[tokio::test]
async fn bokheim_wins_reader_edit_conflicts_and_invalid_files_are_preserved() {
    for (format, name) in [(AnnotationExportFormat::Foliate, "EPUB Book.foliate.json"), (AnnotationExportFormat::Readest, "EPUB Book.readest.json")] {
        let f = Fixture::new(true).await;
        let (book, mut note) = add_epub(&f).await;
        replace(&f, vec![format]).await;
        let path = f.destination.join(name);
        let mut data: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        data["annotations"][0]["note"] = "External edit".into();
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        note.note = "Bokheim edit".into();
        note.modified_at += 1;
        f.db.upsert_annotation(&note).await.unwrap();
        let watermark = f.db.sync_outbox_watermark().await.unwrap();
        let report = replace(&f, vec![format]).await;
        assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Written), "{report:?}");
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["annotations"][0]["note"], "Bokheim edit");
        assert_eq!(f.db.annotations(book).await.unwrap(), vec![note]);
        assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
        for invalid in [b"invalid JSON".to_vec(), {
            data["_bokheim_book_id"] = serde_json::to_value(f.book).unwrap();
            serde_json::to_vec(&data).unwrap()
        }] {
            fs::write(&path, &invalid).unwrap();
            let report = replace(&f, vec![format]).await;
            assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Failed(_)), "{report:?}");
            assert_eq!(fs::read(&path).unwrap(), invalid);
            assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
        }
    }
}

#[tokio::test]
async fn foliate_records_without_ownership_are_preserved_without_importing() {
    let f = Fixture::new(true).await;
    let (book, note) = add_epub(&f).await;
    replace(&f, vec![AnnotationExportFormat::Foliate]).await;
    let path = f.destination.join("EPUB Book.foliate.json");
    let mut data: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    data.as_object_mut().unwrap().remove("_bokheim_annotation_ids");
    data["annotations"][0].as_object_mut().unwrap().remove("_bokheim_id");
    data["annotations"][0]["value"] = "epubcfi(/6/2!/4/4,/1:0,/1:7)".into();
    data["annotations"][0]["text"] = "External moved quote".into();
    data["annotations"][0]["note"] = "External edit without a baseline".into();
    data["annotations"][0]["custom"] = "keep this reader field".into();
    data["annotations"].as_array_mut().unwrap().push(serde_json::json!({
        "value":"epubcfi(/6/2!/4/6,/1:0,/1:7)", "text":"New external quote", "note":"New external note", "color":"yellow",
        "created":"2026-10-01T12:00:00Z"
    }));
    fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(!f.db.path().with_extension("annotation-export-baselines").exists());
    let watermark = f.db.sync_outbox_watermark().await.unwrap();
    let report = replace(&f, vec![AnnotationExportFormat::Foliate]).await;
    assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Written), "{report:?}");
    let notes = f.db.annotations(book).await.unwrap();
    assert_eq!(notes, vec![note.clone()]);
    assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
    let written: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let kept = written["annotations"].as_array().unwrap().iter().find(|record| written["_bokheim_annotation_ids"][record["value"].as_str().unwrap()] == note.id).unwrap();
    assert_eq!(kept["note"], note.note);
    assert_eq!(kept["value"].as_str(), note.anchor.epub_cfi_value());
    assert!(kept.get("custom").is_none());
    for external in data["annotations"].as_array().unwrap() {
        assert!(written["annotations"].as_array().unwrap().contains(external));
    }
}

#[tokio::test]
async fn old_json_exports_keep_identities_but_discard_merge_history() {
    for (format, extension) in [(AnnotationExportFormat::Foliate, "foliate"), (AnnotationExportFormat::Readest, "readest")] {
        let f = Fixture::new(true).await;
        let (book, original) = add_epub(&f).await;
        replace(&f, vec![format]).await;
        let path = f.destination.join(format!("EPUB Book.{extension}.json"));
        let mut data: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let record = &mut data["annotations"][0];
        record.as_object_mut().unwrap().remove("_bokheim_id");
        let published = record.clone();
        record["_bokheim"] = serde_json::json!({"model":original,"published":published});
        record["note"] = "Reader edit of a known note".into();
        record["custom"] = "keep reader-specific data".into();
        let mut discovery = record.clone();
        let mut old_model = original.clone();
        old_model.id = "legacy-discovery".into();
        old_model.note = "Old embedded note that must not be imported".into();
        discovery["_bokheim"]["model"] = serde_json::to_value(&old_model).unwrap();
        if format == AnnotationExportFormat::Foliate {
            discovery["value"] = "epubcfi(/6/2!/4/6,/1:0,/1:7)".into();
            discovery["created"] = "2026-10-01T12:00:00Z".into();
        } else {
            discovery["id"] = old_model.id.clone().into();
            discovery["cfi"] = "epubcfi(/6/2!/4/6,/1:0,/1:7)".into();
        }
        discovery["note"] = "Newly found reader note".into();
        data["annotations"].as_array_mut().unwrap().push(discovery.clone());
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        let watermark = f.db.sync_outbox_watermark().await.unwrap();
        let report = replace(&f, vec![format]).await;
        assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
        let notes = f.db.annotations(book).await.unwrap();
        assert_eq!(notes, vec![original.clone()]);
        assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(written["annotations"][0].get("_bokheim").is_none());
        assert!(written["annotations"].as_array().unwrap().contains(&discovery));
        if format == AnnotationExportFormat::Foliate {
            assert!(written["_bokheim_annotation_fields"].as_object().unwrap().values().all(|fields| fields["custom"] == "keep reader-specific data"));
            assert_eq!(written["_bokheim_annotation_fields"].as_object().unwrap().len(), 1);
            assert_eq!(written["annotations"][0].as_object().unwrap().len(), 6);
        } else {
            assert!(written["annotations"].as_array().unwrap().iter().all(|note| note["custom"] == "keep reader-specific data"));
        }
        let watermark = f.db.sync_outbox_watermark().await.unwrap();
        replace(&f, vec![format]).await;
        assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
        assert_eq!(f.db.annotations(book).await.unwrap(), notes);
    }
}

#[tokio::test]
async fn pdf_exports_preserve_undiscovered_foreign_marks_without_syncing_them() {
    let f = Fixture::new(true).await;
    let path = f.root.join("Original.pdf");
    let watermark = f.db.sync_outbox_watermark().await.unwrap();
    let report = replace(&f, vec![AnnotationExportFormat::Pdf, AnnotationExportFormat::Markdown]).await;
    assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
    assert_eq!(f.db.annotations(f.book).await.unwrap(), vec![f.note.clone()]);
    assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
    assert!(!fs::read_to_string(f.destination.join("A Book.md")).unwrap().contains("Keep me"));
    assert_eq!(f.db.canonical_content_hash(f.book).await.unwrap(), Some(f.canonical));
    let mut pdf = Document::load(&path).unwrap();
    let foreign = pdf.objects.iter().find_map(|(id, value)| value.as_dict().ok().filter(|mark| mark.get(b"NM").and_then(Object::as_str).ok() == Some(b"some-other-reader")).map(|_| *id)).unwrap();
    assert_eq!(decode_text_string(pdf.get_dictionary(foreign).unwrap().get(b"Contents").unwrap()).unwrap(), "Keep me");
    pdf.get_dictionary_mut(foreign).unwrap().set("Contents", Object::string_literal("External edit"));
    pdf.save(&path).unwrap();
    let report = replace(&f, vec![AnnotationExportFormat::Pdf]).await;
    assert!(matches!(report.results[0].outcome, AnnotationExportOutcome::Written), "{report:?}");
    assert_eq!(f.db.annotations(f.book).await.unwrap(), vec![f.note.clone()]);
    assert_eq!(f.db.sync_outbox_watermark().await.unwrap(), watermark);
    let pdf = Document::load(&path).unwrap();
    assert_eq!(decode_text_string(pdf.get_dictionary(foreign).unwrap().get(b"Contents").unwrap()).unwrap(), "External edit");
}

#[tokio::test]
async fn pdf_exports_preserve_unviewed_highlights_without_extracting_page_text() {
    use lopdf::{
        Stream,
        content::{Content, Operation},
    };
    let book = BookId::from_bytes([42; 32]);
    let mut document = Document::with_version("1.7");
    let pages = document.new_object_id();
    let font = document.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let resources = document.add_object(dictionary! {"Font"=>dictionary! {"F1"=>font}});
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
            Operation::new("Td", vec![100.into(), 600.into()]),
            Operation::new("Tj", vec![Object::string_literal("Foreign selected text")]),
            Operation::new("ET", vec![]),
        ],
    }
    .encode()
    .unwrap();
    let content = document.add_object(Stream::new(dictionary! {}, content).await);
    let mark = document.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Highlight","Rect"=>vec![99.into(),597.into(),230.into(),614.into()],
        "QuadPoints"=>vec![99.into(),614.into(),230.into(),614.into(),99.into(),597.into(),230.into(),597.into()],
        "C"=>vec![1.into(),1.into(),0.into()],"NM"=>Object::string_literal("bokheim:unseen"),"Contents"=>Object::string_literal("Foreign note")
    });
    let page = document.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"Resources"=>resources,"Contents"=>content,"Annots"=>vec![Object::Reference(mark)]});
    document.objects.insert(pages, dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1,"MediaBox"=>vec![0.into(),0.into(),600.into(),800.into()]}.into());
    let catalog = document.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    document.trailer.set("Root", catalog);
    let mut bytes = Vec::new().await;
    document.save_to(&mut bytes).unwrap();
    let prepared = book_inspection::prepare_pdf_annotation_export(Cursor::new(bytes).await, book, &[]).unwrap();
    let mut exported = Vec::new().await;
    prepared.write(&mut exported, PdfSourceIdentity { book_id: book, canonical_hash: None }, &[]).unwrap();
    let document = Document::load_mem(&exported).unwrap();
    let annotation = document.get_dictionary(mark).unwrap();
    assert_eq!(annotation.get(b"Subtype").unwrap().as_name().unwrap(), b"Highlight");
    assert_eq!(decode_text_string(annotation.get(b"Contents").unwrap()).unwrap(), "Foreign note");
    assert_eq!(annotation.get(b"QuadPoints").unwrap().as_array().unwrap().len(), 8);
    assert!(document.extract_text(&[1]).unwrap().contains("Foreign selected text"));
}

#[tokio::test]
async fn obsolete_merge_baselines_cannot_block_exports() {
    let f = Fixture::new(true).await;
    add_epub(&f).await;
    let baseline_folder = f.db.path().with_extension("annotation-export-baselines");
    fs::write(&baseline_folder, "obsolete or unreadable merge history").unwrap();
    let report = replace(&f, vec![AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest, AnnotationExportFormat::Pdf]).await;
    assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
    assert_eq!(fs::read_to_string(baseline_folder).unwrap(), "obsolete or unreadable merge history");
    for format in ["foliate", "readest"] {
        let written: serde_json::Value = serde_json::from_slice(&fs::read(f.destination.join(format!("EPUB Book.{format}.json"))).unwrap()).unwrap();
        assert_eq!(written["annotations"].as_array().unwrap().len(), 1);
        assert!(written["annotations"][0].get("_bokheim").is_none());
    }
}

#[tokio::test]
async fn malformed_epub_requires_attention_without_blocking_other_outputs() {
    let f = Fixture::new(true).await;
    let (book, _) = add_epub(&f).await;
    fs::write(f.root.join("Book.epub"), b"this is no longer an EPUB archive").unwrap();
    let report = replace(&f, vec![AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest, AnnotationExportFormat::Markdown]).await;
    let foliate = report.results.iter().find(|result| result.destination.book_id == book && result.destination.format == AnnotationExportFormat::Foliate).unwrap();
    assert!(matches!(foliate.outcome, AnnotationExportOutcome::Failed(_)), "{report:?}");
    assert_eq!(foliate.failure, Some(AnnotationExportFailure { kind: AnnotationExportFailureKind::InvalidData, file_written: false }));
    for format in [AnnotationExportFormat::Markdown, AnnotationExportFormat::Readest] {
        assert!(report.results.iter().any(|result| result.destination.book_id == book && result.destination.format == format && matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
    }
    assert!(!f.destination.join("EPUB Book.foliate.json").exists());
}
