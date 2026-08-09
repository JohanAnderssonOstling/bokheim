use super::*;
use crate::app::BackendError;
use crate::library::LibraryBackend;

impl LibraryBackend {
    pub fn reading_position(&self, content_hash: ContentHash) -> Result<Option<String>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        Ok(get_book_reading_position(&conn, content_hash).map_err(BackendError::operation)?.map(EpubCfi::into_string))
    }

    pub fn pdf_reading_page(&self, content_hash: ContentHash) -> Result<Option<u32>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_pdf_reading_page(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn audiobook_reading_position(&self, content_hash: ContentHash) -> Result<Option<u64>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        get_audiobook_reading_position(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn update_reading_position(&self, content_hash: ContentHash, cfi: &str, progress: f32) -> Result<(), BackendError> {
        let cfi = EpubCfi::parse(cfi).map_err(BackendError::operation)?;
        let progress = ReadProgress::new(progress).map_err(BackendError::operation)?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        update_reading_position(&conn, content_hash, &cfi, progress).map_err(BackendError::operation)?;
        self.app()?.request_reading_position_sync(self.id().clone());
        Ok(())
    }

    pub fn update_pdf_reading_page(&self, content_hash: ContentHash, page: u32) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        update_pdf_reading_page(&conn, content_hash, page).map_err(BackendError::operation)?;
        self.app()?.request_reading_position_sync(self.id().clone());
        Ok(())
    }

    pub fn update_audiobook_reading_position(&self, content_hash: ContentHash, position_ms: u64, progress: f32) -> Result<(), BackendError> {
        let progress = ReadProgress::new(progress).map_err(BackendError::operation)?;
        let conn = self.db.get().map_err(BackendError::operation)?;
        update_audiobook_reading_position(&conn, content_hash, position_ms, progress).map_err(BackendError::operation)?;
        self.app()?.request_reading_position_sync(self.id().clone());
        Ok(())
    }
}

fn get_stored_reading_position(conn: &Connection, content_hash: ContentHash) -> Result<Option<sync_common::ReadingPosition>, rusqlite::Error> {
    let mut raw = None;
    conn.get_book_reading_position(content_hash.as_str(), |row| {
        raw = row.get::<_, Option<String>>(0)?;
        Ok(())
    })?;
    let Some(raw) = raw.filter(|value| value != "0" && !value.is_empty()) else {
        return Ok(None);
    };
    let position = sync_common::ReadingPosition::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?;
    Ok(Some(position))
}

pub(crate) fn get_book_reading_position(conn: &Connection, content_hash: ContentHash) -> Result<Option<EpubCfi>, rusqlite::Error> {
    get_stored_reading_position(conn, content_hash)?.and_then(|position| position.epub_cfi_value().map(EpubCfi::parse)).transpose().map_err(|error| invalid_text_column(0, error.to_string()))
}

pub(crate) fn get_pdf_reading_page(conn: &Connection, content_hash: ContentHash) -> Result<Option<u32>, rusqlite::Error> {
    Ok(match get_stored_reading_position(conn, content_hash)? {
        Some(position) => position.pdf_page_index(),
        _ => None,
    })
}

pub fn update_reading_position(conn: &Connection, content_hash: ContentHash, cfi: &EpubCfi, progress: ReadProgress) -> Result<(), rusqlite::Error> {
    conn.update_reading_position(content_hash.as_str(), cfi.as_str(), progress.value())?;
    Ok(())
}
pub fn update_pdf_reading_page(conn: &Connection, content_hash: ContentHash, page_index: u32) -> Result<(), rusqlite::Error> {
    let position = sync_common::pdf_page_reading_position(page_index);
    conn.update_pdf_reading_position(content_hash.as_str(), &position)?;
    Ok(())
}
pub(crate) fn get_audiobook_reading_position(conn: &Connection, content_hash: ContentHash) -> Result<Option<u64>, rusqlite::Error> {
    Ok(match get_stored_reading_position(conn, content_hash)? {
        Some(position) => position.audiobook_position_millis(),
        _ => None,
    })
}
pub fn update_audiobook_reading_position(conn: &Connection, content_hash: ContentHash, position_ms: u64, progress: ReadProgress) -> Result<(), rusqlite::Error> {
    let position = sync_common::audiobook_reading_position(position_ms);
    conn.update_reading_position(content_hash.as_str(), &position, progress.value())?;
    Ok(())
}
