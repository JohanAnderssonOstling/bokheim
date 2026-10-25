use super::LibraryClient;
use crate::{BackendError, ContentHash};
use crate::library::{LibraryCommand, PendingCommand, ReadyCommand};

impl LibraryClient {
    pub async fn audiobook_playback_metadata(&self, content_hash: ContentHash) -> Result<Option<library_model::PlaybackMetadata>, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::AudiobookPlaybackMetadata { content_hash })).await
    }
    pub async fn reading_position(&self, content_hash: ContentHash) -> Result<Option<String>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::ReadingPosition { content_hash })).await
    }
    pub async fn pdf_reading_position(&self, content_hash: ContentHash) -> Result<Option<(u32, f32)>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::PdfReadingPosition { content_hash })).await
    }
    pub async fn update_reading_position(&self, content_hash: ContentHash, position: String, progress: Option<f32>, entry: Option<String>) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::UpdateReadingPosition { content_hash, position, progress, entry })).await
    }
    pub async fn reset_chapter_read_status(&self, content_hash: ContentHash) -> Result<book_model::ChapterReadStatus, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::ResetChapterReadStatus { content_hash, target: None })).await
    }
    pub async fn reset_chapter_read_status_at(&self, content_hash: ContentHash, target: String) -> Result<book_model::ChapterReadStatus, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::ResetChapterReadStatus { content_hash, target: Some(target) })).await
    }
    pub async fn annotations(&self, content_hash: ContentHash) -> Result<Vec<book_model::Annotation>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::Annotations { content_hash })).await
    }
    pub async fn upsert_annotation(&self, annotation: book_model::Annotation) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::UpsertAnnotation { annotation })).await
    }
    pub async fn delete_annotation(&self, annotation_id: String, modified_at: i64) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::DeleteAnnotation { annotation_id, modified_at })).await
    }
}
