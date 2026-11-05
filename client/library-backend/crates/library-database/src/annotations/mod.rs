//! Reader annotations: highlights, notes, and their anchors.

use crate::SqlParam;
use crate::query_methods::{blob_column, query_last, query_rows};
use crate::sync::apply::registers::record_local;
use crate::{Database, DatabaseError};
use book_model::{Annotation, RecurringScope};
use include_sqlite_sql::include_sql;
use library_model::RecurringAnnotation;
use library_replica::LibraryStateEntry;
use pdf_view_common::PdfStoredAnnotation;
use rusqlite::{Row, Transaction, params, types::Type};
use sync_common::BookId;

impl Database {
    pub fn annotations(&self, book_id: BookId) -> Result<Vec<Annotation>, DatabaseError> {
        Ok(query_rows(|read| self.connection.get_annotations(&book_id, read), read_annotation)?)
    }

    /// Resolve a live annotation without loading the book's other annotations.
    pub fn annotation(&self, book_id: BookId, id: &str) -> Result<Option<Annotation>, DatabaseError> {
        let annotation = query_last(|read| self.connection.get_annotation_by_id(id, read), read_annotation)?;
        Ok(annotation.filter(|annotation| annotation.book_id == book_id && !annotation.deleted))
    }

    /// Only explicit recurring annotations from live books in this library. Derived inline
    /// matches are never stored or returned as another book's annotations.
    pub fn library_recurring_annotations(&self, excluding: BookId) -> Result<Vec<RecurringAnnotation>, DatabaseError> {
        let mut query = self.connection.prepare(
            "SELECT a.body,COALESCE(b.title,'Untitled') FROM annotation_recurrence r
             JOIN annotation a ON a.id=r.annotation_id JOIN book b ON b.row_id=a.book_row_id
             WHERE r.scope=3 AND a.deleted_at IS NULL AND b.deleted_at IS NULL AND b.content_hash<>?1
             AND EXISTS(SELECT 1 FROM library_book_presence p WHERE p.book_row_id=b.row_id)
             ORDER BY b.title,b.content_hash,a.modified_at,a.id",
        )?;
        Ok(query.query_map([excluding], |row| Ok(RecurringAnnotation { annotation: read_annotation(row)?, book_title: row.get(1)? }))?.collect::<Result<Vec<_>, _>>()?)
    }

    /// Includes tombstones so an old export cannot restore a deleted note.
    pub fn annotation_history(&self, book_id: BookId) -> Result<Vec<Annotation>, DatabaseError> {
        let mut query = self.connection.prepare("SELECT a.body FROM annotation a JOIN book b ON b.row_id=a.book_row_id WHERE b.content_hash=?1 ORDER BY a.id")?;
        Ok(query.query_map([book_id], read_annotation)?.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn upsert_annotation(&self, value: &Annotation) -> Result<(), DatabaseError> {
        value.validate().map_err(DatabaseError::operation)?;
        if value.is_bookmark() && value.toc_ordinal.is_none() {
            return Err(DatabaseError::message("bookmarks require a TOC ordinal"));
        }
        self.write(|transaction| {
            let mut value = value.clone();
            if let Some(existing) = existing_annotation(transaction, &value.id)? {
                value.created_at = existing.created_at;
            }
            value.deleted = false;
            if !transaction.annotation_book_exists(&(&value.book_id), |row| row.get::<_, bool>(0))? {
                return Err(DatabaseError::message("annotation book not found"));
            }
            record_local(transaction, LibraryStateEntry::Annotation { value })?;
            Ok(())
        })
    }

    pub fn delete_annotation(&self, id: &str, modified_at: i64) -> Result<(), DatabaseError> {
        u64::try_from(modified_at).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let mut value = existing_annotation(transaction, id)?.ok_or_else(|| DatabaseError::message("annotation not found"))?;
            value.modified_at = modified_at as u64;
            value.deleted = true;
            record_local(transaction, LibraryStateEntry::Annotation { value })?;
            Ok(())
        })
    }
}

pub(crate) fn project_recurrence(transaction: &Transaction<'_>, value: &Annotation) -> Result<(), DatabaseError> {
    let scope = match value.anchor.recurring_scope().filter(|_| !value.deleted) {
        Some(RecurringScope::Chapter) => 1,
        Some(RecurringScope::Book) => 2,
        Some(RecurringScope::Library) => 3,
        None => {
            transaction.execute("DELETE FROM annotation_recurrence WHERE annotation_id=?1", [&value.id])?;
            return Ok(());
        }
    };
    transaction.execute("INSERT INTO annotation_recurrence(annotation_id,scope) VALUES(?1,?2) ON CONFLICT(annotation_id) DO UPDATE SET scope=excluded.scope", params![value.id, scope])?;
    Ok(())
}

fn existing_annotation(transaction: &Transaction<'_>, id: &str) -> Result<Option<Annotation>, DatabaseError> {
    Ok(query_last(|read| transaction.get_annotation_by_id(id, read), read_annotation)?)
}

fn read_annotation(row: &Row<'_>) -> rusqlite::Result<Annotation> {
    Annotation::decode(blob_column(row, 0)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Blob, Box::new(error)))
}

pub(crate) fn imported_pdf_covers(annotation: &Annotation, stored: &PdfStoredAnnotation) -> bool {
    annotation.id.starts_with("pdf-import:")
        && annotation.anchor.pdf_anchor().is_some_and(|anchor| {
            anchor.page_index as usize == stored.page_index
                && anchor.rects.len() == stored.rects.len()
                && anchor.rects.iter().zip(&stored.rects).all(|(a, b)| [a.left - b[0], a.top - b[1], a.width - b[2], a.height - b[3]].iter().all(|delta| delta.abs() < 0.001))
        })
}

include_sql!("src/annotations/sql/schema/tables.sql");
include_sql!("src/annotations/sql/schema/index.sql");
include_sql!("src/annotations/sql/annotations.sql");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_fixtures::FixturesSql;
    use book_model::{AnnotationAnchor, AnnotationStyle};

    fn export_note(book: BookId) -> Annotation {
        Annotation {
            id: "foreign-note".into(),
            book_id: book,
            anchor: AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2/1:3)"),
            exact_text: "Quote".into(),
            style: AnnotationStyle::Highlight,
            color: 0xffcc00,
            note: "Original".into(),
            created_at: 1,
            toc_ordinal: None,
            progress: None,
            modified_at: 2,
            deleted: false,
        }
    }

    #[test]
    fn annotation_lookup_is_scoped_live_and_does_not_decode_other_annotations() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let book = BookId::from_bytes([7; 32]);
        db.connection.fixtures_insert_book_7(&book).unwrap();
        let annotation = export_note(book);
        db.upsert_annotation(&annotation).unwrap();
        let mut unrelated = annotation.clone();
        unrelated.id = "unrelated-note".into();
        db.upsert_annotation(&unrelated).unwrap();
        // An unrelated corrupt body must not affect resolving this link.
        db.connection.execute("UPDATE annotation SET body=x'00' WHERE id=?1", [&unrelated.id]).unwrap();

        assert_eq!(db.annotation(book, &annotation.id).unwrap(), Some(annotation.clone()));
        assert_eq!(db.annotation(book, "missing").unwrap(), None);
        assert_eq!(db.annotation(BookId::from_bytes([8; 32]), &annotation.id).unwrap(), None);
        db.delete_annotation(&annotation.id, 3).unwrap();
        assert_eq!(db.annotation(book, &annotation.id).unwrap(), None);
    }

    #[test]
    fn recurring_rules_are_inherited_only_at_library_scope_and_export_from_the_origin() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let origin = BookId::from_bytes([7; 32]);
        let other = BookId::from_bytes([8; 32]);
        db.connection.fixtures_insert_book_7(&origin).unwrap();
        db.connection.fixtures_insert_book_7(&other).unwrap();
        for (book, name) in [(origin, "Origin.epub"), (other, "Other.epub")] {
            db.connection
                .execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,?2,'fixture',1 FROM book WHERE content_hash=?3", params![sync_common::ROOT_DIR_ID.to_string(), name, book])
                .unwrap();
        }
        let anchor = AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:9)");
        let mut annotation = Annotation {
            id: "recurring-name".into(),
            book_id: origin,
            anchor: anchor.clone(),
            exact_text: "Elizabeth".into(),
            style: AnnotationStyle::Highlight,
            color: 0xffcc00,
            note: "One shared note".into(),
            created_at: 1,
            toc_ordinal: Some(2),
            progress: Some(25.0),
            modified_at: 2,
            deleted: false,
        };
        for scope in [RecurringScope::Chapter, RecurringScope::Book, RecurringScope::Library] {
            annotation.anchor = anchor.with_recurring_scope(Some(scope));
            db.upsert_annotation(&annotation).unwrap();
            assert_eq!(db.annotations(origin).unwrap(), vec![annotation.clone()]);
            assert!(db.annotations(other).unwrap().is_empty());
            assert!(db.library_recurring_annotations(origin).unwrap().is_empty());
            let inherited = db.library_recurring_annotations(other).unwrap();
            assert_eq!(inherited.len(), usize::from(scope == RecurringScope::Library));
            if let Some(inherited) = inherited.first() {
                assert_eq!(inherited.annotation, annotation);
            }
        }
        annotation.note = "Edited from another book".into();
        annotation.modified_at = 3;
        db.upsert_annotation(&annotation).unwrap();
        assert_eq!(db.annotations(origin).unwrap()[0].note, annotation.note);
        assert_eq!(db.annotations(origin).unwrap()[0].anchor.origin(), &anchor);
        annotation.anchor = annotation.anchor.with_recurring_scope(Some(RecurringScope::Book));
        db.upsert_annotation(&annotation).unwrap();
        assert!(db.library_recurring_annotations(other).unwrap().is_empty());
        annotation.anchor = annotation.anchor.with_recurring_scope(Some(RecurringScope::Library));
        db.upsert_annotation(&annotation).unwrap();
        db.delete_annotation(&annotation.id, 4).unwrap();
        assert!(db.annotations(origin).unwrap().is_empty());
        assert!(db.library_recurring_annotations(other).unwrap().is_empty());
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn annotations(book_id: BookId) -> Vec<Annotation>;
    arguments (BookId,) = (book_id,);
    execute database, arguments => database.annotations(arguments.0);

    fn annotation(book_id: BookId, id: &str) -> Option<Annotation>;
    arguments (BookId, String,) = (book_id, id.to_owned(),);
    execute database, arguments => database.annotation(arguments.0, &arguments.1);

    fn library_recurring_annotations(excluding: BookId) -> Vec<RecurringAnnotation>;
    arguments (BookId,) = (excluding,);
    execute database, arguments => database.library_recurring_annotations(arguments.0);

    fn annotation_history(book_id: BookId) -> Vec<Annotation>;
    arguments (BookId,) = (book_id,);
    execute database, arguments => database.annotation_history(arguments.0);

    fn upsert_annotation(value: &Annotation) -> ();
    arguments (Annotation,) = (value.clone(),);
    execute database, arguments => database.upsert_annotation(&arguments.0);

    fn delete_annotation(id: &str, modified_at: i64) -> ();
    arguments (String, i64,) = (id.to_owned(), modified_at,);
    execute database, arguments => database.delete_annotation(&arguments.0, arguments.1);
}
