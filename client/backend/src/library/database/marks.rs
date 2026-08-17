//! Persistent reader annotations and bookmarks for a library.

use crate::app::BackendError;
use crate::library::LibraryBackend;
use crate::ContentHash;
pub use app_contract::reader_state::{AnnotationAnchor, AnnotationStyle, PdfAnnotationAnchor, PdfAnnotationRect, ReaderAnnotation, ReaderBookmark};
use include_sqlite_sql::{impl_sql, include_sql};
use rusqlite::{types::Type, Connection, Row};

include_sql!("src/library/database/sql/annotations.sql");
include_sql!("src/library/database/sql/bookmarks.sql");

impl LibraryBackend {
    pub fn annotations(&self, content_hash: ContentHash) -> Result<Vec<ReaderAnnotation>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        load_annotations(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn upsert_annotation(&self, annotation: &ReaderAnnotation) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        store_annotation(&conn, annotation).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn delete_annotation(&self, annotation_id: &str, modified_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        remove_annotation(&conn, annotation_id, modified_at).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn bookmarks(&self, content_hash: ContentHash) -> Result<Vec<ReaderBookmark>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        load_bookmarks(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn create_bookmark(&self, bookmark: &ReaderBookmark) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        insert_bookmark(&conn, bookmark).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn delete_bookmark(&self, bookmark_id: &str, deleted_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        remove_bookmark(&conn, bookmark_id, deleted_at).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }
}

fn load_annotations(conn: &Connection, content_hash: ContentHash) -> Result<Vec<ReaderAnnotation>, Box<dyn std::error::Error>> {
    let mut values = Vec::new();
    conn.get_annotations(content_hash.as_str(), |row| {
        values.push(ReaderAnnotation {
            id: row.get(0)?,
            content_hash: ContentHash::new(&row.get::<_, String>(1)?),
            anchor: annotation_anchor(row, 2)?,
            exact_text: row.get(7)?,
            style: AnnotationStyle::parse_storage(&row.get::<_, String>(8)?).ok_or(rusqlite::Error::InvalidQuery)?,
            color: row.get(9)?,
            note: row.get(10)?,
            created_at: row.get(11)?,
            modified_at: row.get(12)?,
        });
        Ok(())
    })?;
    Ok(values)
}

fn store_annotation(conn: &Connection, value: &ReaderAnnotation) -> Result<(), Box<dyn std::error::Error>> {
    value.validate(false)?;
    let anchor = store_annotation_anchor(&value.anchor)?;
    conn.upsert_annotation(
        &value.id,
        value.content_hash.as_str(),
        anchor.kind,
        anchor.cfi.as_deref(),
        anchor.page,
        anchor.rects.as_deref(),
        anchor.fallback_cfi.as_deref(),
        &value.exact_text,
        value.style.storage(),
        &value.color,
        &value.note,
        value.created_at,
        value.modified_at,
    )?;
    Ok(())
}

fn remove_annotation(conn: &Connection, id: &str, modified_at: i64) -> Result<(), Box<dyn std::error::Error>> {
    let mut deleted = annotation_by_id(conn, id)?.ok_or("annotation not found")?;
    deleted.modified_at = modified_at;
    deleted.validate(true)?;
    conn.delete_annotation(id, modified_at)?;
    Ok(())
}

fn load_bookmarks(conn: &Connection, content_hash: ContentHash) -> Result<Vec<ReaderBookmark>, Box<dyn std::error::Error>> {
    let mut bookmarks = Vec::new();
    conn.get_bookmarks(content_hash.as_str(), |row| {
        bookmarks.push(ReaderBookmark { id: row.get(0)?, content_hash: ContentHash::new(&row.get::<_, String>(1)?), cfi: row.get(2)?, label: row.get(3)?, created_at: row.get(4)?, modified_at: row.get(5)? });
        Ok(())
    })?;
    Ok(bookmarks)
}

fn insert_bookmark(conn: &Connection, value: &ReaderBookmark) -> Result<(), Box<dyn std::error::Error>> {
    value.validate(false)?;
    if value.modified_at != value.created_at {
        return Err("a new bookmark's modified_at must equal created_at".into());
    }
    conn.create_bookmark(&value.id, value.content_hash.as_str(), &value.cfi, &value.label, value.created_at)?;
    Ok(())
}

fn remove_bookmark(conn: &Connection, id: &str, deleted_at: i64) -> Result<(), Box<dyn std::error::Error>> {
    let mut existing = None;
    conn.get_live_bookmark(id, |row| {
        existing = Some((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?));
        Ok(())
    })?;
    let existing = existing.ok_or("bookmark not found")?;
    ReaderBookmark { id: id.to_owned(), content_hash: ContentHash::new(&existing.0), cfi: existing.1, label: existing.2, created_at: existing.3, modified_at: deleted_at }.validate(true)?;
    conn.delete_bookmark(id, deleted_at)?;
    Ok(())
}

fn annotation_anchor(row: &Row<'_>, index: usize) -> rusqlite::Result<AnnotationAnchor> {
    let kind = row.get::<_, String>(index)?;
    match kind.as_str() {
        "epub_cfi" => Ok(AnnotationAnchor::epub_cfi(row.get::<_, String>(index + 1)?)),
        "pdf" => {
            let page_index = u32::try_from(row.get::<_, i64>(index + 2)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(index + 2, Type::Integer, Box::new(error)))?;
            let encoded = row.get::<_, Vec<u8>>(index + 3)?;
            let rects: Vec<PdfAnnotationRect> = sync_common::wire::decode(&encoded, 1024 * 1024).map_err(|error| rusqlite::Error::FromSqlConversionFailure(index + 3, Type::Blob, Box::new(error)))?;
            let fallback_cfi = row.get::<_, Option<String>>(index + 4)?;
            let anchor = PdfAnnotationAnchor::new(page_index, rects, fallback_cfi);
            Ok(AnnotationAnchor::pdf(anchor))
        }
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

pub(crate) struct StoredAnchor {
    pub(crate) kind: &'static str,
    pub(crate) cfi: Option<String>,
    pub(crate) page: Option<i64>,
    pub(crate) rects: Option<Vec<u8>>,
    pub(crate) fallback_cfi: Option<String>,
}

pub(crate) fn store_annotation_anchor(anchor: &AnnotationAnchor) -> Result<StoredAnchor, Box<dyn std::error::Error>> {
    match anchor {
        AnnotationAnchor::EpubCfi { cfi } => Ok(StoredAnchor { kind: "epub_cfi", cfi: Some(cfi.clone()), page: None, rects: None, fallback_cfi: None }),
        AnnotationAnchor::Pdf { page_index, rects, fallback_cfi } => Ok(StoredAnchor { kind: "pdf", cfi: None, page: Some(i64::from(*page_index)), rects: Some(sync_common::wire::encode(rects)?), fallback_cfi: fallback_cfi.clone() }),
    }
}

fn annotation_by_id(conn: &Connection, id: &str) -> rusqlite::Result<Option<ReaderAnnotation>> {
    let mut value = None;
    conn.get_annotation_by_id(id, |row| {
        value = Some(ReaderAnnotation {
            id: row.get(0)?,
            content_hash: ContentHash::new(&row.get::<_, String>(1)?),
            anchor: annotation_anchor(row, 2)?,
            exact_text: row.get(7)?,
            style: AnnotationStyle::parse_storage(&row.get::<_, String>(8)?).ok_or(rusqlite::Error::InvalidQuery)?,
            color: row.get(9)?,
            note: row.get(10)?,
            created_at: row.get(11)?,
            modified_at: row.get(12)?,
        });
        Ok(())
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::fixture_content_hash;

    fn database() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::library::database::init_db(&connection).unwrap();
        connection.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES(?1, 'Reader state', 1, 'epub')", [fixture_content_hash(1).as_str()]).unwrap();
        connection
    }

    #[test]
    fn annotation_and_bookmark_mutations_create_syncable_tombstones() {
        let connection = database();
        let annotation = ReaderAnnotation {
            id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(),
            content_hash: fixture_content_hash(1),
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4,/1:0,/1:4)"),
            exact_text: "Text".to_owned(),
            style: AnnotationStyle::Highlight,
            color: "#f6c945".to_owned(),
            note: "Note".to_owned(),
            created_at: 10,
            modified_at: 10,
        };
        store_annotation(&connection, &annotation).unwrap();
        assert_eq!(load_annotations(&connection, fixture_content_hash(1)).unwrap().len(), 1);
        let body: Vec<u8> = connection.query_row("SELECT body FROM sync_outbox WHERE state_kind='annotation' AND state_key=?1", [&annotation.id], |row| row.get(0)).unwrap();
        let synced: library_replica::MutationBody = sync_common::wire::decode(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap();
        let library_replica::MutationBody::Annotation { value: synced, .. } = synced else { panic!("expected annotation mutation") };
        assert_eq!(synced.anchor, annotation.anchor.clone().into());
        remove_annotation(&connection, &annotation.id, 11).unwrap();
        assert!(load_annotations(&connection, fixture_content_hash(1)).unwrap().is_empty());

        let bookmark = ReaderBookmark { id: "91b27f15-50c5-47b6-af88-88136253f9aa".to_owned(), content_hash: fixture_content_hash(1), cfi: "epubcfi(/6/2!/4/1:0)".to_owned(), label: "Start".to_owned(), created_at: 12, modified_at: 12 };
        insert_bookmark(&connection, &bookmark).unwrap();
        remove_bookmark(&connection, &bookmark.id, 13).unwrap();

        let events: i64 = connection.query_row("SELECT count(*) FROM sync_outbox WHERE state_kind IN ('annotation','bookmark')", [], |row| row.get(0)).unwrap();
        assert_eq!(events, 2);
        let body: Vec<u8> = connection.query_row("SELECT body FROM sync_outbox WHERE state_kind='annotation' AND state_key=?1", [&annotation.id], |row| row.get(0)).unwrap();
        let synced: library_replica::MutationBody = sync_common::wire::decode(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap();
        assert!(matches!(synced, library_replica::MutationBody::Annotation { value, .. } if value.deleted));
    }

    #[test]
    fn storage_round_trips_typed_epub_and_pdf_anchors() {
        let connection = database();
        let epub = ReaderAnnotation {
            id: "6fc87cb1-8030-41b1-8313-3b9e654f403a".to_owned(),
            content_hash: fixture_content_hash(1),
            anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4,/1:0,/1:4)"),
            exact_text: "EPUB text".to_owned(),
            style: AnnotationStyle::Highlight,
            color: "#f6c945".to_owned(),
            note: String::new(),
            created_at: 1,
            modified_at: 1,
        };
        store_annotation(&connection, &epub).unwrap();

        let pdf_anchor = PdfAnnotationAnchor::new(2, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.04)], None);
        let pdf = ReaderAnnotation {
            id: "8c1fe446-709c-4fdf-9065-d3a0938291df".to_owned(),
            content_hash: fixture_content_hash(1),
            anchor: AnnotationAnchor::pdf(pdf_anchor.clone()),
            exact_text: "PDF text".to_owned(),
            style: AnnotationStyle::Underline,
            color: "#f6c945".to_owned(),
            note: String::new(),
            created_at: 2,
            modified_at: 2,
        };
        store_annotation(&connection, &pdf).unwrap();
        let loaded = load_annotations(&connection, fixture_content_hash(1)).unwrap();
        assert_eq!(loaded.iter().find(|item| item.id == epub.id).unwrap().anchor, epub.anchor);
        assert_eq!(loaded.iter().find(|item| item.id == pdf.id).unwrap().pdf_anchor(), Some(pdf_anchor));
    }
}
