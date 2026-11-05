//! Reader annotations, notebook queries, and annotation export.

use crate::library::LibraryHandle;
use crate::{BackendError, BookId};
use library_model::LibraryUpdate;

impl LibraryHandle {
    pub async fn annotations(&self, book_id: BookId) -> Result<Vec<book_model::Annotation>, BackendError> {
        let database = self.database();
        Ok(database.annotations(book_id).await?)
    }

    pub async fn annotation(&self, book_id: BookId, annotation_id: String) -> Result<Option<book_model::Annotation>, BackendError> {
        let database = self.database();
        Ok(database.annotation(book_id, &annotation_id).await?)
    }

    pub async fn reader_annotations(&self, book_id: BookId) -> Result<(Vec<book_model::Annotation>, Vec<library_model::RecurringAnnotation>), BackendError> {
        let database = self.database();
        Ok((database.annotations(book_id).await?, database.library_recurring_annotations(book_id).await?))
    }

    pub async fn notebook(&self, query: library_model::NotebookQuery, request: library_model::NotebookRequest) -> Result<Option<library_model::NotebookPage>, BackendError> {
        let database = self.database();
        Ok(database.notebook(&query, &request, false).await?)
    }

    pub async fn notebook_annotations(&self, query: library_model::NotebookQuery, revision: String, book: BookId, offset: usize) -> Result<Option<library_model::NotebookAnnotationPage>, BackendError> {
        let database = self.database();
        Ok(database.notebook_annotations(&query, &revision, book, offset, false).await?)
    }

    pub async fn upsert_annotation(&self, annotation: book_model::Annotation) -> Result<(), BackendError> {
        let library = &self.owner.resources;
        let database = self.database();
        database.upsert_annotation(&annotation).await?;
        library.events.updates.publish(LibraryUpdate::Annotations);
        Ok(())
    }

    pub async fn delete_annotation(&self, annotation_id: String, modified_at: i64) -> Result<(), BackendError> {
        let library = &self.owner.resources;
        let database = self.database();
        database.delete_annotation(&annotation_id, modified_at).await?;
        library.events.updates.publish(LibraryUpdate::Annotations);
        Ok(())
    }

    pub async fn annotation_export_snapshot(&self) -> Result<Vec<library_model::AnnotationExportBook>, BackendError> {
        let database = self.database();
        Ok(database.annotation_export_snapshot().await?)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn export_annotations(&self, options: library_model::AnnotationExportOptions) -> Result<library_model::AnnotationExportReport, BackendError> {
        self.run_io(async {
            let library = &self.owner.resources;
            let storage = self.storage().await?;
            Ok(library_annotation_export::export(storage.database(), storage.assets(), library.id, options).await?)
        })
        .await
    }
}
