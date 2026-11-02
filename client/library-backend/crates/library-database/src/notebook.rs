//! Search annotations without opening publications or decoding the entire library.
use crate::browse::options::BrowseOptions;
use crate::{Database, DatabaseError, stored_document};
use book_model::{Annotation, normalize_search_text};
use library_model::{NOTEBOOK_ANNOTATION_PAGE_SIZE, NotebookAnnotationPage, NotebookBook, NotebookPage, NotebookQuery, NotebookRequest, NotebookSort};
use pdf_view_common::PdfReaderMetadata;
use rusqlite::{OptionalExtension, Transaction, params};
use sync_common::BookId;

pub(crate) fn install(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    tx.execute_batch(include_str!("notebook/schema.sql"))?;
    let annotations = tx.prepare("SELECT body FROM annotation WHERE deleted_at IS NULL")?.query_map([], |row| row.get::<_, Vec<u8>>(0))?.collect::<Result<Vec<_>, _>>()?;
    for body in annotations {
        let annotation = Annotation::decode(&body).map_err(DatabaseError::operation)?;
        project_annotation(tx, &annotation)?;
        crate::annotations::project_recurrence(tx, &annotation)?;
    }
    let books = tx.prepare("SELECT content_hash FROM pdf_reader_metadata")?.query_map([], |row| row.get::<_, BookId>(0))?.collect::<Result<Vec<_>, _>>()?;
    for book in books {
        project_pdf(tx, book)?;
    }
    Ok(())
}

pub(crate) fn project_annotation(tx: &Transaction<'_>, annotation: &Annotation) -> Result<(), DatabaseError> {
    if annotation.deleted || annotation.is_bookmark() {
        tx.execute("DELETE FROM notebook_annotation WHERE id=?1", [&annotation.id])?;
    } else {
        let body = annotation.encode().map_err(DatabaseError::operation)?;
        let search = normalize_search_text(&format!("{}\n{}", annotation.exact_text, annotation.note));
        let created = i64::try_from(annotation.created_at).map_err(DatabaseError::operation)?;
        tx.execute(
            "INSERT INTO notebook_annotation(id,book_row_id,body,search_text,created_at,external,toc_ordinal,progress)
            SELECT ?1,row_id,?3,?4,?5,?6,?7,?8 FROM book WHERE content_hash=?2
            ON CONFLICT(id) DO UPDATE SET book_row_id=excluded.book_row_id,body=excluded.body,search_text=excluded.search_text,created_at=excluded.created_at,external=excluded.external,toc_ordinal=excluded.toc_ordinal,progress=excluded.progress",
            params![annotation.id, annotation.book_id, body, search, created, annotation.is_external_pdf_annotation(), annotation.toc_ordinal, annotation.progress],
        )?;
    }
    Ok(())
}

pub(crate) fn project_pdf(tx: &Transaction<'_>, book: BookId) -> Result<(), DatabaseError> {
    tx.execute("DELETE FROM notebook_annotation WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND external=1", [book])?;
    let bytes: Option<Vec<u8>> = tx.query_row("SELECT metadata FROM pdf_reader_metadata WHERE content_hash=?1", [book], |row| row.get(0)).optional()?;
    let Some(pdf) = bytes.and_then(|body| stored_document::decode::<PdfReaderMetadata>(&body).ok()).filter(|pdf| pdf.valid_for(&book.to_hex())) else { return Ok(()) };
    let history = tx
        .prepare("SELECT a.body FROM annotation a JOIN book b ON b.row_id=a.book_row_id WHERE b.content_hash=?1")?
        .query_map([book], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|body| Annotation::decode(&body).map_err(DatabaseError::operation))
        .collect::<Result<Vec<_>, _>>()?;
    for value in pdf.annotations {
        if history.iter().any(|annotation| crate::annotations::imported_pdf_covers(annotation, &value)) {
            continue;
        }
        let Some(annotation) = library_model::stored_pdf_annotation(book, &value) else { continue };
        project_annotation(tx, &annotation)?;
    }
    Ok(())
}

const MATCHES: &str = "FROM notebook_annotation n JOIN book b ON b.row_id=n.book_row_id
    JOIN library_book_presence p ON p.book_row_id=b.row_id
    WHERE b.deleted_at IS NULL AND (?1=0 OR p.downloaded=1)
    AND (?2=0 OR (COALESCE(b.format,1) & ?2)!=0)
    AND (?3='' OR instr(n.search_text,?3)>0 OR instr(normalize_search_text(COALESCE(b.title,'')||' '||COALESCE(b.author,'')),?3)>0)";

impl Database {
    /// Notebook is a browse source: selection, caching, ordering and revision
    /// validation all belong to the same prepared-result pipeline.
    pub(crate) fn notebook_book_ids(&self, options: &BrowseOptions, sort: NotebookSort) -> Result<Vec<i64>, DatabaseError> {
        let order = match sort {
            NotebookSort::Recent => "MAX(n.created_at) DESC, normalize_search_text(COALESCE(b.title,'')),b.row_id",
            NotebookSort::BookName => "normalize_search_text(COALESCE(b.title,'')),b.row_id",
        };
        let sql = format!("SELECT b.row_id {MATCHES} GROUP BY b.row_id ORDER BY {order}");
        Ok(self.connection.prepare(&sql)?.query_map(params![options.downloaded_only, options.file_types, options.search], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn notebook(&self, query: &NotebookQuery, request: &NotebookRequest, downloaded_only: bool) -> Result<Option<NotebookPage>, DatabaseError> {
        let browse_query = query.browse_query();
        let options = BrowseOptions::from_query(&browse_query, downloaded_only);
        self.browse_page_with(&browse_query, downloaded_only, request.revision.as_deref(), request.page, |data| {
            let page = data.page.ok_or_else(|| DatabaseError::message("notebook browse page is missing"))?;
            let revision = page.result_revision.as_deref().ok_or_else(|| DatabaseError::message("notebook revision is missing"))?;
            let mut books = Vec::with_capacity(data.contents.books.len());
            for book in data.contents.books {
                let Some(id) = book.library_file() else { continue };
                let offset = request.annotation_offsets.get(&id).copied().unwrap_or(0);
                let annotations = self.notebook_annotation_page(&options, query.sort, id, offset, revision)?;
                books.push(NotebookBook { book, page: annotations });
            }
            Ok(NotebookPage { books, page })
        })
    }

    /// Annotation windows use the same revision guard as book continuations.
    pub fn notebook_annotations(&self, query: &NotebookQuery, revision: &str, book: BookId, offset: usize, downloaded_only: bool) -> Result<Option<NotebookAnnotationPage>, DatabaseError> {
        let browse_query = query.browse_query();
        let options = BrowseOptions::from_query(&browse_query, downloaded_only);
        self.with_browse_result(&browse_query, downloaded_only, Some(revision), |_, _| self.notebook_annotation_page(&options, query.sort, book, offset, revision))
    }

    fn notebook_annotation_page(&self, options: &BrowseOptions, sort: NotebookSort, book: BookId, mut offset: usize, revision: &str) -> Result<NotebookAnnotationPage, DatabaseError> {
        let limit = NOTEBOOK_ANNOTATION_PAGE_SIZE;
        let order = match sort {
            NotebookSort::Recent => "n.created_at DESC,n.id",
            NotebookSort::BookName => "n.toc_ordinal,n.progress,n.id",
        };
        let sql = format!("SELECT n.body {MATCHES} AND b.content_hash=?4 ORDER BY {order} LIMIT ?5 OFFSET ?6");
        let read = |offset: usize| -> Result<Vec<Annotation>, DatabaseError> {
            Ok(self
                .connection
                .prepare_cached(&sql)?
                .query_map(params![options.downloaded_only, options.file_types, options.search, book, (limit + 1) as i64, offset.min(i64::MAX as usize) as i64], |row| {
                    Annotation::decode(&row.get::<_, Vec<u8>>(0)?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error)))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        };
        let mut annotations = read(offset)?;
        if annotations.is_empty() && offset > 0 {
            // A refresh can follow deletions that shortened the book. Clamp to
            // its last window in this request, without a chain of UI retries.
            let count: i64 = self.connection.query_row(&format!("SELECT COUNT(*) {MATCHES} AND b.content_hash=?4"), params![options.downloaded_only, options.file_types, options.search, book], |row| row.get(0))?;
            let count = usize::try_from(count).map_err(DatabaseError::operation)?;
            offset = count.saturating_sub(1) / limit * limit;
            annotations = read(offset)?;
        }
        let has_more = annotations.len() > limit;
        annotations.truncate(limit);
        Ok(NotebookAnnotationPage { annotations, has_more, offset, revision: revision.to_owned() })
    }
}

#[cfg(test)]
mod tests;
