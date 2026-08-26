use super::*;

pub fn get_stored_reading_position(conn: &Connection, content_hash: ContentHash) -> Result<Option<library_model::ReadingPosition>, rusqlite::Error> {
    let mut raw = None;
    conn.get_book_reading_position(content_hash.as_str(), |row| {
        raw = row.get::<_, Option<String>>(0)?;
        Ok(())
    })?;
    let Some(raw) = raw.filter(|value| value != "0" && !value.is_empty()) else {
        return Ok(None);
    };
    let position = library_model::ReadingPosition::parse(&raw).map_err(|error| invalid_text_column(0, error.to_string()))?;
    Ok(Some(position))
}

pub fn get_book_reading_position(conn: &Connection, content_hash: ContentHash) -> Result<Option<EpubCfi>, rusqlite::Error> {
    get_stored_reading_position(conn, content_hash)?.and_then(|position| position.epub_cfi_value().map(EpubCfi::parse)).transpose().map_err(|error| invalid_text_column(0, error.to_string()))
}

pub fn get_pdf_reading_page(conn: &Connection, content_hash: ContentHash) -> Result<Option<u32>, rusqlite::Error> {
    Ok(match get_stored_reading_position(conn, content_hash)? {
        Some(position) => position.pdf_page_index(),
        _ => None,
    })
}

pub fn get_pdf_reading_position(conn: &Connection, content_hash: ContentHash) -> Result<Option<(u32, f32)>, rusqlite::Error> {
    Ok(match get_stored_reading_position(conn, content_hash)? {
        Some(position) => position.pdf_page_index().map(|page_index| (page_index, position.pdf_page_position().unwrap_or(0.0))),
        _ => None,
    })
}

/// Stores the one synchronized reading-position register used by every format.
/// `position` is validated and canonicalized before SQLite sees it. Progress is
/// optional because not every reader calculates a publication-wide fraction.
pub fn update_reading_position(conn: &Connection, content_hash: ContentHash, position: &str, progress: Option<ReadProgress>) -> Result<(), rusqlite::Error> {
    let position = library_model::ReadingPosition::parse(position).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?.as_str();
    conn.set_reading_position(content_hash.as_str(), &position, progress.map(ReadProgress::value))?;
    Ok(())
}
pub fn get_audiobook_reading_position(conn: &Connection, content_hash: ContentHash) -> Result<Option<u64>, rusqlite::Error> {
    Ok(match get_stored_reading_position(conn, content_hash)? {
        Some(position) => position.audiobook_position_millis(),
        _ => None,
    })
}
