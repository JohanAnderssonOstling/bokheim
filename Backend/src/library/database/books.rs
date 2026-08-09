//! Typed book-card queries used by library browsing.

use super::*;

pub(super) fn read_book_card_row(row: &rusqlite::Row<'_>) -> Result<BookCardRow, rusqlite::Error> {
    let raw_book_id: String = row.get(0)?;
    let book_id = BookId::parse(&raw_book_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
    let title: Option<String> = row.get(1).ok();
    let title = title.unwrap_or_else(|| format!("Untitled ({raw_book_id})"));
    let author: Option<String> = row.get(2).ok();
    let description: Option<String> = row.get(3)?;
    let progress: Option<f32> = row.get(4)?;
    let duration_ms: Option<i64> = row.get(8)?;
    let chapter_count: Option<i64> = row.get(9)?;
    Ok(BookCardRow {
        book_id,
        title,
        author: author.unwrap_or_default(),
        description: description.unwrap_or_default(),
        progress: progress.unwrap_or_default(),
        downloaded: row.get(5)?,
        download_requested: row.get(6)?,
        audiobook: row.get(7)?,
        audiobook_duration_ms: duration_ms.and_then(|value| u64::try_from(value).ok()),
        audiobook_chapter_count: chapter_count.and_then(|value| usize::try_from(value).ok()),
    })
}

pub(crate) fn get_book_cards_in_dir_scoped(conn: &Connection, dir_id: &str, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    conn.get_book_cards_in_dir(dir_id, i32::from(downloaded_only), |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}

/// A book placed in several folders yields one row per placement, so results
/// are collapsed to one card per book.
pub(crate) fn get_book_cards_by_author_scoped(conn: &Connection, author_id: AuthorId, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    let mut seen = std::collections::HashSet::new();
    conn.get_book_cards_by_author(&author_id.to_string(), i32::from(downloaded_only), |row| {
        let card = read_book_card_row(row)?;
        if seen.insert(card.book_id) {
            cards.push(card);
        }
        Ok(())
    })?;
    Ok(cards)
}
