//! Reading positions, chapter status, and finished-book state.

use crate::library::{LibraryHandle, LibraryResources};
use crate::{BackendError, BookId};
use library_model::LibraryUpdate;

impl LibraryResources {
    fn reading_position_changed(&self, book_id: BookId) {
        self.event_state.lock().expect("library events").reading_position_changed(book_id, &self.events);
        // Shelf membership, recency and progress change locally, before sync.
        self.events.updates.publish(LibraryUpdate::ReadingProgress(book_id));
    }
}

impl LibraryHandle {
    pub async fn reading_position(&self, book_id: BookId) -> Result<Option<String>, BackendError> {
        let database = self.database();
        Ok(database.reading_position(&book_id).await?)
    }

    pub async fn pdf_reading_position(&self, book_id: BookId) -> Result<Option<(u32, f32)>, BackendError> {
        let database = self.database();
        Ok(database.pdf_reading_position(&book_id).await?)
    }

    pub async fn reset_chapter_read_status(&self, book_id: BookId) -> Result<book_model::ChapterReadStatus, BackendError> {
        self.reset_chapter_read_status_inner(book_id, None).await
    }

    pub async fn reset_chapter_read_status_at(&self, book_id: BookId, target: String) -> Result<book_model::ChapterReadStatus, BackendError> {
        self.reset_chapter_read_status_inner(book_id, Some(target)).await
    }

    async fn reset_chapter_read_status_inner(&self, book_id: BookId, target: Option<String>) -> Result<book_model::ChapterReadStatus, BackendError> {
        let session = &self.owner.resources;
        let database = self.database();
        if database.reset_chapter_read_status_at(&book_id, target.as_deref()).await? {
            session.reading_position_changed(book_id);
        }
        Ok(database.chapter_read_status(&book_id).await?)
    }

    pub async fn update_reading_position(&self, book_id: BookId, position: String, progress: Option<f32>, entry: Option<String>) -> Result<(), BackendError> {
        let session = &self.owner.resources;
        let database = self.database();
        if database.update_reading_position(&book_id, &position, progress, entry.as_deref()).await? {
            session.reading_position_changed(book_id);
        }
        Ok(())
    }

    pub async fn set_book_read(&self, book_id: BookId, read: bool) -> Result<(), BackendError> {
        let session = &self.owner.resources;
        let database = self.database();
        if database.set_book_read(&book_id, read).await? {
            session.reading_position_changed(book_id);
        }
        Ok(())
    }
}
