//! Stored navigation documents.
//!
//! Inspection hands every format the same hierarchical form — title, target,
//! children — so what a format called its navigation is settled before it
//! reaches storage. What a target *means* remains the format's business and is
//! recorded in the target itself (`pdf-page:`, `audiobook-position-ms:`, or a
//! publisher's href).
//!
//! What is left here is the storage contract: one navigation document per
//! book, whatever the format, including the recorded fact that a book has none.
//! The row is keyed by the book's identity rather than by the bytes it was read
//! from, because an identity is defined to survive a revision and the targets
//! are meaningful for every revision it survives: a PDF carries its identity in
//! the file so its pagination does not move, and an audiobook derives one from
//! the audio its offsets are offsets into.

use crate::storage_values::invalid_integer_column;
use crate::storage_values::invalid_text_column;
use include_sqlite_sql::include_sql;
use library_model::BookTocEntry;
use rusqlite::OptionalExtension;
use sync_common::ContentHash;

fn count(entries: &[BookTocEntry]) -> usize {
    entries.iter().map(|entry| 1 + count(&entry.children)).sum()
}

/// Records a book's navigation, including the fact that it has none. Written in
/// the caller's transaction, beside the metadata it was inspected with.
///
/// An empty tree is a result rather than an absence: it records that the file
/// was inspected and carries no navigation, so nothing parses it again hoping
/// for a different answer. That is the row existing with no entries, which is
/// why the read distinguishes `Some(vec![])` from `None`.
pub fn upsert_book_toc(conn: &rusqlite::Transaction<'_>, content_hash: &ContentHash, entries: &[BookTocEntry]) -> Result<(), rusqlite::Error> {
    let entry_count = i64::try_from(count(entries)).map_err(|_| invalid_integer_column(1, "table of contents entry count exceeds SQLite integer range"))?;
    let toc_json = serde_json::to_string(entries).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.upsert_book_toc(content_hash.as_str(), entry_count, &toc_json)?;
    Ok(())
}

/// One book's navigation, or `None` where the book has not been inspected for
/// one yet. An inspected book with no navigation returns `Some(vec![])`, which
/// is what stops the caller parsing the file again.
pub fn book_toc(conn: &rusqlite::Connection, content_hash: &ContentHash) -> Result<Option<Vec<BookTocEntry>>, rusqlite::Error> {
    let stored: Option<String> = conn.query_row(include_str!("sql/toc/book_toc_select.sql"), [content_hash.as_str()], |row| row.get(0)).optional()?;
    stored.map(|json| serde_json::from_str::<Vec<BookTocEntry>>(&json).map_err(|error| invalid_text_column(0, format!("stored table of contents is not readable: {error}")))).transpose()
}

/// Records which entry the reader is inside. Written beside a position update,
/// because that is exactly when it changes.
pub fn set_reading_entry(conn: &rusqlite::Connection, content_hash: &ContentHash, target: &str) -> Result<(), rusqlite::Error> {
    conn.upsert_book_reading_entry(content_hash.as_str(), target)?;
    Ok(())
}

pub fn reading_entry(conn: &rusqlite::Connection, content_hash: &ContentHash) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(include_str!("sql/toc/reading_entry_select.sql"), [content_hash.as_str()], |row| row.get(0)).optional()
}

/// The title of one entry in a stored tree. The target is the identity; the
/// wording comes from whatever navigation document is stored now.
pub fn entry_title(entries: &[BookTocEntry], target: &str) -> Option<String> {
    for entry in entries {
        if entry.target == target {
            return Some(entry.title.clone());
        }
        if let Some(found) = entry_title(&entry.children, target) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, target: &str, children: Vec<BookTocEntry>) -> BookTocEntry {
        BookTocEntry { title: title.to_owned(), target: target.to_owned(), children }
    }

    fn hash(seed: &str) -> ContentHash {
        ContentHash::new(&seed.repeat(64))
    }

    fn library() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn a_stored_tree_comes_back_whole() {
        let mut conn = library();
        let hash = hash("a");
        let nav = vec![entry("Part I", "part1.xhtml", vec![entry("Chapter 1", "ch1.xhtml", Vec::new()), entry("Chapter 2", "ch2.xhtml#start", Vec::new())])];

        let transaction = conn.transaction().unwrap();
        upsert_book_toc(&transaction, &hash, &nav).unwrap();
        transaction.commit().unwrap();

        assert_eq!(book_toc(&conn, &hash).unwrap(), Some(nav), "depth and targets survive the round trip");
    }

    #[test]
    fn a_book_with_no_navigation_is_distinguishable_from_one_never_inspected() {
        let mut conn = library();
        let inspected = hash("b");
        let unseen = hash("c");

        let transaction = conn.transaction().unwrap();
        upsert_book_toc(&transaction, &inspected, &[]).unwrap();
        transaction.commit().unwrap();

        assert_eq!(book_toc(&conn, &inspected).unwrap(), Some(Vec::new()), "inspected and empty");
        assert_eq!(book_toc(&conn, &unseen).unwrap(), None, "not inspected, so the caller may still parse it");
    }

    #[test]
    fn re_inspecting_replaces_the_stored_tree() {
        let mut conn = library();
        let hash = hash("d");

        let transaction = conn.transaction().unwrap();
        upsert_book_toc(&transaction, &hash, &[]).unwrap();
        upsert_book_toc(&transaction, &hash, &[entry("Chapter 1", "ch1.xhtml", Vec::new())]).unwrap();
        transaction.commit().unwrap();

        let stored = book_toc(&conn, &hash).unwrap().unwrap();
        assert_eq!(stored.len(), 1, "the later inspection wins rather than accumulating");
    }

    #[test]
    fn the_entry_count_counts_the_whole_tree_not_its_roots() {
        let mut conn = library();
        let hash = hash("e");
        let nav = vec![entry("Part I", "p1", vec![entry("One", "1", vec![entry("One point one", "1.1", Vec::new())])]), entry("Part II", "p2", Vec::new())];

        let transaction = conn.transaction().unwrap();
        upsert_book_toc(&transaction, &hash, &nav).unwrap();
        transaction.commit().unwrap();

        let counted: i64 = conn.query_row(include_str!("sql/toc/the_entry_count_counts_the_whole_tree_not_its_roots_select.sql"), [hash.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(counted, 4, "two roots, one child and one grandchild");
    }
}

include_sql!("src/toc/sql/toc.sql");

/// One navigation document per book, in the form inspection already produced
/// it: a document's own tree, or a PDF's outline lifted into the same shape at
/// the same boundary. Nothing here knows which it was reading.
///
/// An audiobook is written by the audiobook path instead, because its chapters
/// can also arrive from enrichment rather than from the file, and the rule
/// deciding which of the two wins lives there.
///
/// Import and scan both come through here. They are the only two ways a book
/// enters a library, and which one a reader used should not decide whether the
/// book's page can list its chapters.
pub fn inspected_navigation(format: book_model::BookFormat, toc: &[book_model::BookTocEntry]) -> Option<Vec<library_model::BookTocEntry>> {
    match format {
        book_model::BookFormat::Epub | book_model::BookFormat::Mobi | book_model::BookFormat::Pdf => Some(toc.to_vec()),
        book_model::BookFormat::M4b => None,
    }
}
