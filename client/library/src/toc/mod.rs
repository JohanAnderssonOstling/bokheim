//! Stored navigation documents.
//!
//! Every format arrives in a different shape — an EPUB nav document is already
//! a tree of hrefs, a PDF outline is a flat list carrying a depth, an M4B is an
//! ordered list of offsets — and every reader used to normalise its own. The
//! normalisation happens here instead, on the way in, so that one hierarchical
//! form is what the library stores and what every surface reads: title, target,
//! children. What a target *means* is the format's business and is recorded in
//! the target itself (`pdf-page:`, `audiobook-position-ms:`, or a publisher's
//! href).

use crate::storage_values::invalid_integer_column;
use crate::storage_values::invalid_text_column;
use crate::toc::queries::TocSql;
use library_model::{BookTocEntry, PublicationTocEntry};
use rusqlite::OptionalExtension;
use sync_common::ContentHash;

/// Where a stored navigation document came from. `None` is a result, not an
/// absence: it records that the file was inspected and carries no navigation,
/// so nothing re-parses it hoping for a different answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TocSource {
    EmbeddedEpub,
    PdfOutline,
    None,
}

impl TocSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::EmbeddedEpub => "embedded_epub",
            Self::PdfOutline => "pdf_outline",
            Self::None => "none",
        }
    }
}

fn count(entries: &[BookTocEntry]) -> usize {
    entries.iter().map(|entry| 1 + count(&entry.children)).sum()
}

/// The inspection's entries are the same tree under another name — the two
/// types exist so that a storage contract and a UI contract can move
/// independently, not because their shapes differ.
pub fn entries_from_inspection(entries: &[PublicationTocEntry]) -> Vec<BookTocEntry> {
    entries.iter().map(|entry| BookTocEntry { title: entry.title.clone(), target: entry.target.clone(), children: entries_from_inspection(&entry.children) }).collect()
}

/// Rebuilds a tree from entries that state their depth instead of their
/// children — the shape a PDF outline arrives in, because the pdfium wrapper
/// counts a bookmark's ancestors rather than carrying `/First` and `/Next`
/// through. Takes triples rather than a PDF type so that storage keeps no
/// opinion about formats.
///
/// A depth that skips a level (0 then 2, which malformed outlines do produce)
/// attaches to the deepest entry so far rather than being dropped: the reader
/// wants every entry, and an outline's own inconsistency is not a reason to
/// hide a chapter.
pub fn entries_from_depths(items: impl IntoIterator<Item = (String, String, usize)>) -> Vec<BookTocEntry> {
    let mut roots: Vec<BookTocEntry> = Vec::new();
    let mut path: Vec<usize> = Vec::new();
    for (title, target, depth) in items {
        path.truncate(depth);
        let mut list = &mut roots;
        for index in &path {
            list = &mut list[*index].children;
        }
        list.push(BookTocEntry { title, target, children: Vec::new() });
        let position = list.len() - 1;
        path.push(position);
    }
    roots
}

/// Records a book's navigation, including the fact that it has none. Written in
/// the caller's transaction, beside the metadata it was inspected with.
pub fn upsert_book_toc(conn: &rusqlite::Transaction<'_>, content_hash: &ContentHash, source: TocSource, entries: &[BookTocEntry]) -> Result<(), rusqlite::Error> {
    let entry_count = i64::try_from(count(entries)).map_err(|_| invalid_integer_column(1, "table of contents entry count exceeds SQLite integer range"))?;
    let toc_json = serde_json::to_string(entries).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    conn.upsert_book_toc(content_hash.as_str(), entry_count, source.as_str(), &toc_json)?;
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
    fn depths_become_children() {
        let flat = [
            ("Part I".to_owned(), "pdf-page:1".to_owned(), 0),
            ("Chapter 1".to_owned(), "pdf-page:2".to_owned(), 1),
            ("Section 1.1".to_owned(), "pdf-page:3".to_owned(), 2),
            ("Chapter 2".to_owned(), "pdf-page:9".to_owned(), 1),
            ("Part II".to_owned(), "pdf-page:20".to_owned(), 0),
        ];

        let tree = entries_from_depths(flat);

        assert_eq!(tree.len(), 2, "two parts at the root");
        assert_eq!(tree[0].children.len(), 2, "Part I holds both of its chapters");
        assert_eq!(tree[0].children[0].children[0].title, "Section 1.1", "and the section stays under its chapter");
        assert_eq!(tree[1].title, "Part II");
        assert!(tree[1].children.is_empty());
    }

    #[test]
    fn a_skipped_depth_keeps_the_entry_rather_than_dropping_it() {
        let tree = entries_from_depths([("Root".to_owned(), "pdf-page:1".to_owned(), 0), ("Orphan".to_owned(), "pdf-page:2".to_owned(), 3)]);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 1, "the orphan attaches to the deepest entry so far");
        assert_eq!(tree[0].children[0].title, "Orphan");
    }

    #[test]
    fn a_stored_tree_comes_back_whole() {
        let mut conn = library();
        let hash = hash("a");
        let nav = vec![entry("Part I", "part1.xhtml", vec![entry("Chapter 1", "ch1.xhtml", Vec::new()), entry("Chapter 2", "ch2.xhtml#start", Vec::new())])];

        let transaction = conn.transaction().unwrap();
        upsert_book_toc(&transaction, &hash, TocSource::EmbeddedEpub, &nav).unwrap();
        transaction.commit().unwrap();

        assert_eq!(book_toc(&conn, &hash).unwrap(), Some(nav), "depth and targets survive the round trip");
    }

    #[test]
    fn a_book_with_no_navigation_is_distinguishable_from_one_never_inspected() {
        let mut conn = library();
        let inspected = hash("b");
        let unseen = hash("c");

        let transaction = conn.transaction().unwrap();
        upsert_book_toc(&transaction, &inspected, TocSource::None, &[]).unwrap();
        transaction.commit().unwrap();

        assert_eq!(book_toc(&conn, &inspected).unwrap(), Some(Vec::new()), "inspected and empty");
        assert_eq!(book_toc(&conn, &unseen).unwrap(), None, "not inspected, so the caller may still parse it");
    }

    #[test]
    fn re_inspecting_replaces_the_stored_tree() {
        let mut conn = library();
        let hash = hash("d");

        let transaction = conn.transaction().unwrap();
        upsert_book_toc(&transaction, &hash, TocSource::None, &[]).unwrap();
        upsert_book_toc(&transaction, &hash, TocSource::EmbeddedEpub, &[entry("Chapter 1", "ch1.xhtml", Vec::new())]).unwrap();
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
        upsert_book_toc(&transaction, &hash, TocSource::EmbeddedEpub, &nav).unwrap();
        transaction.commit().unwrap();

        let counted: i64 = conn.query_row(include_str!("sql/toc/the_entry_count_counts_the_whole_tree_not_its_roots_select.sql"), [hash.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(counted, 4, "two roots, one child and one grandchild");
    }
}

pub(crate) mod queries;

/// A PDF outline is a tree in the file — `/First`, `/Next` and `/Parent` — that
/// the pdfium wrapper reports as a flat list with each bookmark's ancestor
/// count. This puts the tree back and gives every entry the page it points at,
/// so a contents row can open the book there.
pub fn pdf_navigation(outline: &[pdf_view_common::PdfStoredOutline]) -> Vec<library_model::BookTocEntry> {
    entries_from_depths(outline.iter().map(|entry| (entry.title.clone(), library_model::pdf_toc_target(entry.page_index), entry.depth)))
}

/// One navigation document per book, whatever the format called it: a
/// document's is the publisher's own tree, a PDF's is its outline put back
/// together, and an audiobook has none — its chapters are stored as chapters.
/// `None` here means "this format stores no navigation document", which is not
/// the same as [`TocSource::None`], the recorded fact that a file was
/// inspected and carries none.
///
/// Import and scan both come through here. They are the only two ways a book
/// enters a library, and which one a reader used should not decide whether the
/// book's page can list its chapters.
pub fn inspected_navigation(format: library_model::BookFormat, toc: &[library_model::PublicationTocEntry], pdf: Option<&pdf_view_common::PdfReaderMetadata>) -> Option<(TocSource, Vec<library_model::BookTocEntry>)> {
    match format {
        library_model::BookFormat::Epub | library_model::BookFormat::Mobi => {
            let entries = entries_from_inspection(toc);
            Some((if entries.is_empty() { TocSource::None } else { TocSource::EmbeddedEpub }, entries))
        }
        library_model::BookFormat::Pdf => {
            let entries = pdf_navigation(pdf.map(|pdf| pdf.outline.as_slice()).unwrap_or_default());
            Some((if entries.is_empty() { TocSource::None } else { TocSource::PdfOutline }, entries))
        }
        library_model::BookFormat::M4b => None,
    }
}
