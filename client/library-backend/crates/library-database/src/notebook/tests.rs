use super::*;
use book_model::{AnnotationAnchor, AnnotationStyle, BookFormat};
use library_model::{BrowsePageRequest, LibraryFileTypeFilter};
use pdf_view_common::{PdfStoredAnnotation, PdfStoredAnnotationKind, PdfStoredPage};
use sync_common::ROOT_DIR_ID;

fn book(db: &Database, byte: u8, title: &str, format: BookFormat, downloaded: bool) -> BookId {
    let id = BookId::from_bytes([byte; 32]);
    db.connection.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,?2,?3)", params![id, title, format as i64]).unwrap();
    db.connection
        .execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,?2,'fixture',?3 FROM book WHERE content_hash=?4", params![ROOT_DIR_ID.to_string(), format!("{byte}.epub"), downloaded, id])
        .unwrap();
    id
}

fn annotation(db: &Database, book: BookId, id: &str, created: u64, text: &str, note: &str) -> Annotation {
    let annotation = Annotation {
        id: id.into(),
        content_hash: book,
        anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:0)"),
        exact_text: text.into(),
        style: AnnotationStyle::Highlight,
        color: 0xe0bb4b,
        note: note.into(),
        created_at: created,
        modified_at: created,
        toc_ordinal: None,
        progress: Some(10.0),
        deleted: false,
    };
    db.upsert_annotation(&annotation).unwrap();
    annotation
}

fn read_notebook(db: &Database, query: &NotebookQuery, offset: usize, limit: usize, downloaded_only: bool) -> NotebookPage {
    let request = NotebookRequest { page: BrowsePageRequest { section_offset: 0, section_limit: 0, book_offset: offset, book_limit: limit }, ..Default::default() };
    db.notebook(query, &request, downloaded_only).unwrap().unwrap()
}

#[test]
fn notebook_search_filters_ordering_pagination_and_deletions_share_live_membership() {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    let zed = book(&db, 11, "Zed", BookFormat::Epub, true);
    let alpha = book(&db, 12, "Alpha", BookFormat::Pdf, false);
    annotation(&db, zed, "one", 30, "A passage", "Ångström and 100% literal");
    let mut newer = annotation(&db, zed, "two", 40, "Second passage", "");
    annotation(&db, alpha, "three", 10, "A different book", "");
    db.connection.execute_batch("DROP TRIGGER notebook_revision_insert; DROP TRIGGER notebook_revision_update; DROP TRIGGER notebook_revision_delete; DROP TABLE notebook_revision; PRAGMA user_version=47;").unwrap();
    db.initialize_library().unwrap();
    let query = NotebookQuery::default();
    let first = read_notebook(&db, &query, 0, 1, false);
    assert!(first.page.book_offset + first.books.len() < first.page.book_total);
    assert_eq!(first.books[0].book.library_file(), Some(zed));
    assert_eq!(first.books[0].page.annotations.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["two", "one"]);
    let revision = first.page.result_revision.as_deref().unwrap();
    let request = BrowsePageRequest { section_offset: 0, section_limit: 0, book_offset: 1, book_limit: 1 };
    let next = db.notebook(&query, &NotebookRequest { page: request, revision: Some(revision.to_owned()), ..Default::default() }, false).unwrap().unwrap();
    assert_eq!(next.page.book_offset + next.books.len(), next.page.book_total);
    assert_eq!(next.books[0].book.library_file(), Some(alpha));
    // Inserting a newer annotation changes the order without changing totals.
    let newest = annotation(&db, alpha, "newest", 100, "New passage", "");
    assert!(db.notebook(&query, &NotebookRequest { page: request, revision: Some(revision.to_owned()), ..Default::default() }, false).unwrap().is_none(), "stale selections must refresh instead of appending duplicate or skipped books");
    let changed = read_notebook(&db, &query, 0, 1, false);
    assert_eq!(changed.books[0].book.library_file(), Some(alpha));
    let revision = changed.page.result_revision.as_deref().unwrap();
    db.delete_annotation(&newest.id, 101).unwrap();
    assert!(db.notebook(&query, &NotebookRequest { page: request, revision: Some(revision.to_owned()), ..Default::default() }, false).unwrap().is_none());
    let named = read_notebook(&db, &NotebookQuery { sort: NotebookSort::BookName, ..query.clone() }, 0, 8, false);
    assert_eq!(named.books[0].book.library_file(), Some(alpha));
    let found = read_notebook(&db, &NotebookQuery { search: "angstrom".into(), ..query.clone() }, 0, 8, false);
    assert_eq!(found.books.len(), 1);
    assert_eq!(found.books[0].page.annotations[0].id, "one");
    assert_eq!(read_notebook(&db, &NotebookQuery { search: "%".into(), ..query.clone() }, 0, 8, false).books.len(), 1);
    assert_eq!(read_notebook(&db, &NotebookQuery { search: "_".into(), ..query.clone() }, 0, 8, false).books.len(), 0);
    assert_eq!(read_notebook(&db, &query, 0, 8, true).books.len(), 1);
    let pdf = read_notebook(&db, &NotebookQuery { formats: vec![LibraryFileTypeFilter::Pdf], ..query.clone() }, 0, 8, false);
    assert_eq!(pdf.books.len(), 1);
    assert_eq!(pdf.books[0].book.library_file(), Some(alpha));
    newer.note = "Changed while notebook was open".into();
    newer.modified_at = 50;
    db.upsert_annotation(&newer).unwrap();
    assert_eq!(read_notebook(&db, &NotebookQuery { search: "Changed".into(), ..query.clone() }, 0, 8, false).books[0].page.annotations[0].id, "two");
    db.delete_annotation("two", 60).unwrap();
    assert!(read_notebook(&db, &NotebookQuery { search: "Changed".into(), ..query.clone() }, 0, 8, false).books.is_empty());
    db.connection.execute("UPDATE book SET deleted_at=70 WHERE content_hash=?1", [zed]).unwrap();
    assert_eq!(read_notebook(&db, &query, 0, 8, false).books.len(), 1);
    db.connection.execute("UPDATE book_dir SET deleted_at=80 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [alpha]).unwrap();
    assert!(read_notebook(&db, &query, 0, 8, false).books.is_empty());
    for index in 0..40 {
        let id = book(&db, 100 + index, &format!("Refresh book {index}"), BookFormat::Epub, true);
        annotation(&db, id, &format!("refresh-{index}"), index as u64, "A passage", "");
    }
    let refreshed = read_notebook(&db, &query, 0, 40, false);
    assert_eq!(refreshed.books.len(), 40, "refreshing an existing flow must not require repeated searches in 32-book chunks");
    assert_eq!(refreshed.page.book_total, refreshed.books.len());
}

#[test]
fn migration_rebuilds_existing_notes_and_imported_pdf_annotations() {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    let epub = book(&db, 21, "An existing book", BookFormat::Epub, true);
    let pdf = book(&db, 22, "An imported PDF", BookFormat::Pdf, true);
    annotation(&db, epub, "existing", 1, "Existing highlight", "Keep this note");
    annotation(&db, epub, "deleted", 2, "Deleted highlight", "");
    db.delete_annotation("deleted", 3).unwrap();
    let metadata = PdfReaderMetadata {
        version: 1,
        content_hash: pdf.to_hex(),
        pages: vec![PdfStoredPage { width: 600.0, height: 800.0, rotation: 0 }],
        skippable_pages: None,
        dependencies: None,
        annotations: vec![PdfStoredAnnotation {
            page_index: 0,
            index: 0,
            kind: PdfStoredAnnotationKind::Highlight,
            rects: vec![[0.1, 0.2, 0.4, 0.05]],
            color: "#e0bb4b".into(),
            text: "An imported passage".into(),
            note: "Embedded PDF note".into(),
        }],
    };
    db.connection.execute("INSERT INTO pdf_reader_metadata(content_hash,metadata) VALUES(?1,?2)", params![pdf, stored_document::encode(&metadata).unwrap()]).unwrap();
    db.connection.execute_batch("DROP TABLE notebook_annotation; PRAGMA user_version=45;").unwrap();
    db.initialize_library().unwrap();
    let result = read_notebook(&db, &NotebookQuery::default(), 0, 8, false);
    assert_eq!(result.books.len(), 2);
    assert_eq!(result.books.iter().flat_map(|book| &book.page.annotations).count(), 2);
    let imported = &result.books.iter().find(|book| book.book.library_file() == Some(pdf)).unwrap().page.annotations[0];
    assert!(imported.is_external_pdf_annotation());
    assert_eq!(imported.note, "Embedded PDF note");
    assert_eq!(imported.anchor.pdf_anchor().unwrap().rects[0].top, 0.2);
    db.initialize_library().unwrap();
    assert_eq!(read_notebook(&db, &NotebookQuery::default(), 0, 8, false).books.len(), 2);
}

#[test]
fn adopted_pdf_annotations_replace_cached_rows_and_deletions_stay_deleted() {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    let pdf = book(&db, 23, "Adopted PDF", BookFormat::Pdf, true);
    let metadata = PdfReaderMetadata {
        version: 1,
        content_hash: pdf.to_hex(),
        pages: vec![PdfStoredPage { width: 600.0, height: 800.0, rotation: 0 }],
        skippable_pages: None,
        dependencies: None,
        annotations: vec![PdfStoredAnnotation { page_index: 0, index: 0, kind: PdfStoredAnnotationKind::Highlight, rects: vec![[0.1, 0.2, 0.4, 0.05]], color: "#e0bb4b".into(), text: "Passage".into(), note: "Note".into() }],
    };
    db.connection.execute("INSERT INTO pdf_reader_metadata(content_hash,metadata) VALUES(?1,?2)", params![pdf, stored_document::encode(&metadata).unwrap()]).unwrap();
    let tx = db.connection.unchecked_transaction().unwrap();
    project_pdf(&tx, pdf).unwrap();
    tx.commit().unwrap();
    let mut adopted = read_notebook(&db, &NotebookQuery::default(), 0, 8, false).books[0].page.annotations[0].clone();
    adopted.id = "pdf-import:adopted".into();
    db.import_export_annotations(pdf, &[adopted.clone()]).unwrap();
    let notebook = read_notebook(&db, &NotebookQuery::default(), 0, 8, false);
    assert_eq!(notebook.books[0].page.annotations, vec![adopted.clone()], "cached source and normal annotation must appear once");
    db.delete_annotation(&adopted.id, 1).unwrap();
    assert!(read_notebook(&db, &NotebookQuery::default(), 0, 8, false).books.is_empty());
    let tx = db.connection.unchecked_transaction().unwrap();
    project_pdf(&tx, pdf).unwrap();
    tx.commit().unwrap();
    assert!(read_notebook(&db, &NotebookQuery::default(), 0, 8, false).books.is_empty(), "refreshing metadata must not restore a deleted cached copy");
}

#[test]
fn annotation_windows_are_bounded_ordered_filtered_and_migrate_from_v46() {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    let id = book(&db, 25, "Paged book", BookFormat::Epub, true);
    for index in 0..70 {
        let mut value = annotation(&db, id, &format!("paged-{index:03}"), index + 1, "Find this passage", "");
        value.toc_ordinal = Some((69 - index) as i64);
        db.upsert_annotation(&value).unwrap();
    }
    // Exercise the actual v46 upgrade rather than only fresh schema creation.
    db.connection.execute_batch("DROP TRIGGER notebook_revision_insert; DROP TRIGGER notebook_revision_update; DROP TRIGGER notebook_revision_delete; DROP TABLE notebook_revision; DROP INDEX notebook_annotation_location; ALTER TABLE notebook_annotation DROP COLUMN toc_ordinal; ALTER TABLE notebook_annotation DROP COLUMN progress; PRAGMA user_version=46;").unwrap();
    db.initialize_library().unwrap();
    let query = NotebookQuery::default();
    let first = read_notebook(&db, &query, 0, 8, false).books.remove(0).page;
    assert_eq!(first.annotations.len(), NOTEBOOK_ANNOTATION_PAGE_SIZE);
    assert!(first.has_more);
    assert_eq!(first.annotations[0].id, "paged-069");
    let next = db.notebook_annotations(&query, &first.revision, id, 32, false).unwrap().unwrap();
    assert_eq!(next.annotations.len(), 32);
    assert!(next.has_more);
    assert_eq!(next.annotations[0].id, "paged-037");
    let last = db.notebook_annotations(&query, &first.revision, id, 64, false).unwrap().unwrap();
    assert_eq!(last.annotations.len(), 6);
    assert!(!last.has_more);
    let located = NotebookQuery { sort: NotebookSort::BookName, ..query.clone() };
    let located_revision = read_notebook(&db, &located, 0, 8, false).page.result_revision.unwrap();
    assert_eq!(db.notebook_annotations(&located, &located_revision, id, 32, false).unwrap().unwrap().annotations[0].toc_ordinal, Some(32));
    let missing = NotebookQuery { search: "missing phrase".into(), ..query.clone() };
    let missing_revision = read_notebook(&db, &missing, 0, 8, false).page.result_revision.unwrap();
    assert!(db.notebook_annotations(&missing, &missing_revision, id, 0, false).unwrap().unwrap().annotations.is_empty());
    annotation(&db, id, "paged-new", 100, "Find this passage", "");
    assert!(db.notebook_annotations(&query, &first.revision, id, 32, false).unwrap().is_none(), "annotation paging must reject a changed order");
    let request = NotebookRequest { annotation_offsets: [(id, 32)].into(), ..Default::default() };
    let refreshed = db.notebook(&query, &request, false).unwrap().unwrap().books.remove(0).page;
    assert_eq!(refreshed.offset, 32, "refresh must hydrate the current window directly");
    assert_eq!(refreshed.annotations[0].id, "paged-038");
    for index in 0..65 {
        db.delete_annotation(&format!("paged-{index:03}"), 200).unwrap();
    }
    let request = NotebookRequest { annotation_offsets: [(id, 64)].into(), ..Default::default() };
    let shortened = db.notebook(&query, &request, false).unwrap().unwrap().books.remove(0).page;
    assert_eq!(shortened.offset, 0, "a deleted window clamps to the last surviving one in the same request");
    assert_eq!(shortened.annotations.len(), 6);
    assert_eq!(db.connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), crate::LIBRARY_SCHEMA_VERSION);
}
