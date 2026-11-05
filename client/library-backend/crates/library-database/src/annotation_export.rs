//! A consistent, read-only snapshot for local annotation exports.
use crate::{Database, DatabaseError};
use library_model::{AnnotationExportBook, AnnotationExportPlacement};
use rusqlite::{OptionalExtension, params};
use sync_common::{BookId, DirId};

impl Database {
    pub fn annotation_export_snapshot(&self) -> Result<Vec<AnnotationExportBook>, DatabaseError> {
        self.with_browse_snapshot(|| {
            let books = self.annotation_export_book_ids()?;
            self.annotation_export_books_in_snapshot(&books)
        })
    }

    /// Identities included in the export snapshot.
    pub fn annotation_export_book_ids(&self) -> Result<Vec<BookId>, DatabaseError> {
        let mut query = self.connection.prepare(
            "SELECT b.content_hash FROM book b WHERE b.deleted_at IS NULL
             AND EXISTS(SELECT 1 FROM library_book_presence p WHERE p.book_row_id=b.row_id)
             AND (EXISTS(SELECT 1 FROM annotation a WHERE a.book_row_id=b.row_id AND a.id NOT LIKE 'bookmark:%')
                  OR EXISTS(SELECT 1 FROM pdf_reader_metadata p WHERE p.content_hash=b.content_hash)) ORDER BY b.content_hash",
        )?;
        Ok(query.query_map([], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?)
    }

    /// Load annotations, metadata, and navigation for the requested books.
    pub fn annotation_export_books(&self, selected: &[BookId]) -> Result<Vec<AnnotationExportBook>, DatabaseError> {
        self.with_browse_snapshot(|| self.annotation_export_books_in_snapshot(selected))
    }

    fn annotation_export_books_in_snapshot(&self, selected: &[BookId]) -> Result<Vec<AnnotationExportBook>, DatabaseError> {
        let mut query = self.connection.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM annotation a WHERE a.book_row_id=b.row_id AND a.id NOT LIKE 'bookmark:%')
                 FROM book b WHERE b.content_hash=?1 AND b.deleted_at IS NULL
                 AND EXISTS(SELECT 1 FROM library_book_presence p WHERE p.book_row_id=b.row_id)
                 AND (EXISTS(SELECT 1 FROM annotation a WHERE a.book_row_id=b.row_id AND a.id NOT LIKE 'bookmark:%')
                      OR EXISTS(SELECT 1 FROM pdf_reader_metadata p WHERE p.content_hash=b.content_hash))
                 ",
        )?;
        let mut result = Vec::new();
        for &book_id in selected {
            let Some(has_history) = query.query_row([book_id], |row| row.get::<_, bool>(0)).optional()? else { continue };
            let state = self.book_transfer_state_in_snapshot(book_id)?;
            let mut annotations = state.annotations;
            annotations.retain(|annotation| !annotation.is_bookmark());
            if let Some(pdf) = self.pdf_metadata(&book_id)? {
                let history = self.annotation_history(book_id)?;
                annotations.extend(
                    pdf.annotations
                        .into_iter()
                        .filter(|value| {
                            // Cached foreign marks are presentation snapshots. Once
                            // adopted, their live row or tombstone owns that location.
                            !history.iter().any(|annotation| crate::annotations::imported_pdf_covers(annotation, &value))
                        })
                        .filter_map(|value| library_model::stored_pdf_annotation(book_id, &value)),
                );
            }
            // Retain tombstone-only books so exports can remove deleted
            // marks from PDFs and clear previous Markdown/JSON contents.
            if annotations.is_empty() && !has_history {
                continue;
            }
            let rows = self
                .connection
                .prepare("SELECT bd.dir_id,bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=?1 AND bd.deleted_at IS NULL ORDER BY bd.dir_id,bd.file_name")?
                .query_map(params![book_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut placements = Vec::new();
            for (directory, name, downloaded) in rows {
                let directory_id = DirId::parse_str(&directory).map_err(DatabaseError::operation)?;
                if let Some(relative_path) = self.native_book_path(directory_id, &name)? {
                    placements.push(AnnotationExportPlacement { directory_id, relative_path: relative_path.to_string(), downloaded, local_hash: self.local_content_hash(book_id, directory_id)? });
                }
            }
            if placements.is_empty() {
                continue;
            }
            let mut metadata = state.addition.record.metadata;
            if metadata.title.trim().is_empty() {
                metadata.title = std::path::Path::new(&placements[0].relative_path).file_stem().and_then(|name| name.to_str()).unwrap_or("Untitled").to_owned();
            }
            let format = state.addition.facts.format;
            result.push(AnnotationExportBook {
                book_id,
                format,
                metadata,
                annotations,
                toc: state.addition.navigation.map(|navigation| navigation.entries).unwrap_or_default(),
                canonical_hash: self.canonical_content_hash(book_id)?,
                placements,
            });
        }
        Ok(result)
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn annotation_export_snapshot() -> Vec<AnnotationExportBook>;
    arguments () = ();
    execute database, arguments => database.annotation_export_snapshot();

    fn annotation_export_book_ids() -> Vec<BookId>;
    arguments () = ();
    execute database, arguments => database.annotation_export_book_ids();

    fn annotation_export_books(selected: &[BookId]) -> Vec<AnnotationExportBook>;
    arguments (Vec<BookId>,) = (selected.to_vec(),);
    execute database, arguments => database.annotation_export_books(&arguments.0);
}
