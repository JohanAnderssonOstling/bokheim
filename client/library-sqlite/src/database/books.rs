//! Typed book-card queries used by library browsing.

use super::*;

pub fn read_book_card_row(row: &rusqlite::Row<'_>) -> Result<BookCardRow, rusqlite::Error> {
    let raw_content_hash: String = row.get(0)?;
    let content_hash = ContentHash::new(&raw_content_hash.clone());
    let title: Option<String> = row.get(1).ok();
    let title = title.unwrap_or_else(|| format!("Untitled ({raw_content_hash})"));
    let author: Option<String> = row.get(2).ok();
    let description: Option<String> = row.get(3)?;
    let progress: Option<f32> = row.get(4)?;
    let duration_ms: Option<i64> = row.get(8)?;
    let chapter_count: Option<i64> = row.get(9)?;
    Ok(BookCardRow {
        content_hash,
        title,
        author: author.unwrap_or_default(),
        description: description.unwrap_or_default(),
        progress: progress.unwrap_or_default(),
        downloaded: row.get(5)?,
        download_requested: row.get(6)?,
        audiobook: row.get(7)?,
        audiobook_duration_ms: duration_ms.and_then(|value| u64::try_from(value).ok()),
        audiobook_chapter_count: chapter_count.and_then(|value| usize::try_from(value).ok()),
        added_at: row.get(10).unwrap_or_default(),
        source_directory: row.get::<_, Option<String>>(11).ok().flatten().and_then(|value| DirId::parse_str(&value).ok()),
    })
}

pub fn get_book_cards_in_dir_scoped(conn: &Connection, dir_id: &str, downloaded_only: bool, normalized_query: &str, file_type: library_model::LibraryFileTypeFilter) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    get_book_cards_in_dir_sorted_scoped(conn, dir_id, downloaded_only, normalized_query, file_type, library_model::BrowseBookSort::Alphabetical)
}

pub fn get_book_cards_in_dir_sorted_scoped(
    conn: &Connection, dir_id: &str, downloaded_only: bool, normalized_query: &str, file_type: library_model::LibraryFileTypeFilter, book_sort: library_model::BrowseBookSort,
) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    get_book_cards_in_dir_filtered_sorted_scoped(conn, dir_id, downloaded_only, normalized_query, &[file_type], book_sort, &[])
}

pub fn get_book_cards_in_dir_filtered_sorted_scoped(
    conn: &Connection, dir_id: &str, downloaded_only: bool, normalized_query: &str, file_types: &[library_model::LibraryFileTypeFilter], book_sort: library_model::BrowseBookSort, languages: &[&str],
) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    let languages = super::browse::languages_query_value(languages);
    conn.get_book_cards_in_dir(dir_id, i32::from(downloaded_only), normalized_query, super::browse::file_types_query_value(file_types), super::browse::book_sort_query_value(book_sort), &languages, |row| {
        cards.push(read_book_card_row(row)?);
        Ok(())
    })?;
    Ok(cards)
}

/// A book placed in several folders yields one row per placement, so results
/// are collapsed to one card per book.
pub fn get_book_cards_by_author_scoped(conn: &Connection, author_id: AuthorId, downloaded_only: bool) -> Result<Vec<BookCardRow>, rusqlite::Error> {
    let mut cards = Vec::new();
    let mut seen = std::collections::HashSet::new();
    conn.get_book_cards_by_author(&author_id.to_string(), i32::from(downloaded_only), |row| {
        let card = read_book_card_row(row)?;
        if seen.insert(card.content_hash.clone()) {
            cards.push(card);
        }
        Ok(())
    })?;
    Ok(cards)
}
