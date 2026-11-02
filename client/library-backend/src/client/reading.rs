use super::LibraryClient;
use crate::library::{LibraryCommand, PendingCommand, ReadyCommand};
use crate::{BackendError, BookId};

impl LibraryClient {
    pub async fn text_search(&self, query: library_model::NotebookQuery, request: library_model::NotebookRequest) -> Result<Option<library_model::BookTextSearchPage>, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::TextSearch { query, request })).await
    }
    pub async fn text_search_matches(&self, query: library_model::NotebookQuery, revision: String, book: BookId, offset: usize, limit: usize) -> Result<Option<library_model::BookTextMatchPage>, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::TextSearchMatches { query, revision, book, offset, limit })).await
    }
    pub async fn notebook(&self, query: library_model::NotebookQuery, request: library_model::NotebookRequest) -> Result<Option<library_model::NotebookPage>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::Notebook { query, request })).await
    }
    pub async fn notebook_annotations(&self, query: library_model::NotebookQuery, revision: String, book: BookId, offset: usize) -> Result<Option<library_model::NotebookAnnotationPage>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::NotebookAnnotations { query, revision, book, offset })).await
    }
    /// Portable snapshot used by both native exporters and browser hosts.
    pub async fn annotation_export_snapshot(&self) -> Result<Vec<library_model::AnnotationExportBook>,BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::AnnotationExportSnapshot)).await
    }

    pub async fn audiobook_playback_metadata(&self, content_hash: BookId) -> Result<Option<library_model::PlaybackMetadata>, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::AudiobookPlaybackMetadata { content_hash })).await
    }
    pub async fn reading_position(&self, content_hash: BookId) -> Result<Option<String>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::ReadingPosition { content_hash })).await
    }
    pub async fn pdf_reading_position(&self, content_hash: BookId) -> Result<Option<(u32, f32)>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::PdfReadingPosition { content_hash })).await
    }
    pub async fn update_reading_position(&self, content_hash: BookId, position: String, progress: Option<f32>, entry: Option<String>) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::UpdateReadingPosition { content_hash, position, progress, entry })).await
    }
    /// Marks the whole book read or unread, leaving the reading position where it is.
    pub async fn set_book_read(&self, content_hash: BookId, read: bool) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::SetBookRead { content_hash, read })).await
    }
    pub async fn reset_chapter_read_status(&self, content_hash: BookId) -> Result<book_model::ChapterReadStatus, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::ResetChapterReadStatus { content_hash, target: None })).await
    }
    pub async fn reset_chapter_read_status_at(&self, content_hash: BookId, target: String) -> Result<book_model::ChapterReadStatus, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::ResetChapterReadStatus { content_hash, target: Some(target) })).await
    }
    pub async fn annotations(&self, content_hash: BookId) -> Result<Vec<book_model::Annotation>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::Annotations { content_hash })).await
    }
    pub async fn annotation(&self, content_hash: BookId, annotation_id: String) -> Result<Option<book_model::Annotation>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::Annotation { content_hash, annotation_id })).await
    }
    pub async fn library_recurring_annotations(&self, excluding: BookId) -> Result<Vec<library_model::RecurringAnnotation>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::LibraryRecurringAnnotations { excluding })).await
    }
    pub async fn migrate_epub_annotation(&self, annotation_id: String, legacy_cfi: String, publication_cfi: String) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::MigrateEpubAnnotation { annotation_id, legacy_cfi, publication_cfi })).await
    }
    pub async fn upsert_annotation(&self, annotation: book_model::Annotation) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::UpsertAnnotation { annotation })).await
    }
    pub async fn delete_annotation(&self, annotation_id: String, modified_at: i64) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::DeleteAnnotation { annotation_id, modified_at })).await
    }
}
