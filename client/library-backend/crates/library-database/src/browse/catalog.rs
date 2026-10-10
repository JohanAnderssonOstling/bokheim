//! The catalog's side of subject browsing: filters, order, cards and facets
//! over the books in memory, by position in `catalog::books()`. Catalog books
//! are EPUBs that are not on this device and have no reading state.
use std::collections::{BTreeMap, BTreeSet, HashSet};

use content_address::ContentHash;
use library_model::{BookCardRow, BookDetail, BookSource, LibraryFileTypeFilter, LibraryFormatCount, LibraryLanguageCount};

use super::books::MatchingBooks;
use super::options::{file_type_query_value, BrowseOptions};
use super::BooksSql;
use crate::catalog::CatalogBook;
use crate::{Database, DatabaseError};

impl Database {
    /// The hashes of the library's visible books.
    pub(super) fn library_book_hashes(&self) -> Result<HashSet<ContentHash>, DatabaseError> {
        let mut hashes = HashSet::new();
        self.connection.library_book_hashes(|row| {
            hashes.insert(row.get(0)?);
            Ok(())
        })?;
        Ok(hashes)
    }
}

pub(super) fn matching(candidates: &BTreeSet<i64>, options: &BrowseOptions) -> MatchingBooks {
    let books = crate::catalog::books();
    let epub = options.file_types == 0 || options.file_types & file_type_query_value(LibraryFileTypeFilter::Book) != 0;
    let mut matching = BTreeSet::new();
    let mut text_matches = BTreeSet::new();
    for &index in candidates {
        let Some(book) = usize::try_from(index).ok().and_then(|index| books.get(index)) else { continue };
        let language = options.languages.is_empty() || book.languages.iter().any(|language| options.languages.contains(&format!(",{language},")));
        if !epub || options.downloaded_only || !language {
            continue;
        }
        matching.insert(index);
        if !options.search.is_empty() && (book.search_title.contains(&options.search) || book.search_author.contains(&options.search)) {
            text_matches.insert(index);
        }
    }
    MatchingBooks { books: matching, downloaded: BTreeSet::new(), text_matches }
}

/// Cards for `selected`, alphabetically, as the library orders by title.
/// `in_library` marks the books the library already holds.
pub(super) fn cards(selected: &BTreeSet<i64>, in_library: &HashSet<ContentHash>) -> Vec<BookCardRow> {
    let books = crate::catalog::books();
    let mut chosen = selected.iter().filter_map(|&index| usize::try_from(index).ok().and_then(|index| books.get(index))).collect::<Vec<_>>();
    chosen.sort_by(|left, right| left.search_title.cmp(&right.search_title).then_with(|| left.title.cmp(&right.title)).then_with(|| left.hash.cmp(&right.hash)));
    chosen.into_iter().map(|book| card(book, in_library.contains(&book.hash))).collect()
}

fn card(book: &CatalogBook, in_library: bool) -> BookCardRow {
    BookCardRow {
        source: BookSource::Catalog { content_hash: book.hash, in_library },
        title: book.title.clone(),
        subtitle: book.subtitle.clone(),
        author: book.author.clone(),
        progress: 0.0,
        downloaded: false,
        download_requested: false,
        format_category: LibraryFileTypeFilter::Book,
        audiobook_duration_ms: None,
        added_at: 0,
        source_directory: None,
    }
}

/// The page of a catalog book the library does not hold: its record, with
/// nothing about reading or placement.
pub(super) fn detail(hash: &ContentHash) -> Option<BookDetail> {
    let book = crate::catalog::books().iter().find(|book| book.hash == *hash)?;
    Some(BookDetail {
        book: card(book, false),
        audiobook_chapter_count: None,
        description: book.record.description.clone(),
        source_directory_id: None,
        path: Vec::new(),
        authors: Vec::new(),
        credits: Vec::new(),
        current_entry: None,
        current_target: None,
        toc: Vec::new(),
    })
}

/// Language and format counts over the whole catalog, as the library's are
/// over the whole library.
pub(super) fn facets(options: &BrowseOptions) -> (Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>) {
    let books = crate::catalog::books();
    let mut languages = BTreeMap::<&str, u32>::new();
    for book in books {
        for language in &book.languages {
            *languages.entry(language).or_default() += 1;
        }
    }
    let mut language_counts = languages.into_iter().map(|(language, book_count)| LibraryLanguageCount { language: language.to_owned(), book_count }).collect();
    let mut format_counts = if books.is_empty() { Vec::new() } else { vec![LibraryFormatCount { format: LibraryFileTypeFilter::Book, book_count: books.len() as u32 }] };
    super::facets::include_selected_facets(&mut language_counts, &mut format_counts, &options.languages, options.file_types);
    (language_counts, format_counts)
}
