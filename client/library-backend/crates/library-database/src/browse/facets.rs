//! Shared decoding and selected-choice retention for browse facets.
use super::revision::Revision;
use super::{CountsSql, invalid_integer_column, options::BrowseOptions, options::file_type_of, options::file_type_query_value};
use crate::{Database, DatabaseError};
use library_model::{LibraryFileTypeFilter, LibraryFormatCount, LibraryLanguageCount};
use rusqlite::{Connection, OptionalExtension};

type Counts = (Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>);

#[derive(Default)]
pub(crate) struct Facets {
    library: Option<((i64, i64, Option<(u64, i64)>), Counts)>,
    catalog: Option<((u64, i64, Option<(i64, i64, i64, String)>), Counts)>,
}

impl Database {
    pub(super) fn library_facets(&self, options: &BrowseOptions) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), DatabaseError> {
        let (mut language_counts, mut format_counts) = self.read_library_facets()?;
        include_selected_facets(&mut language_counts, &mut format_counts, &options.languages, options.file_types);
        Ok((language_counts, format_counts))
    }

    /// Languages on visible library books, without loading their cards or files.
    pub fn library_language_counts(&self) -> Result<Vec<LibraryLanguageCount>, DatabaseError> {
        self.read_library_facets().map(|(languages, _)| languages)
    }

    fn read_library_facets(&self) -> Result<(Vec<LibraryLanguageCount>, Vec<LibraryFormatCount>), DatabaseError> {
        let revision = Revision::read(&self.connection)?;
        let key = (revision.visibility, revision.facets, revision.legacy);
        if let Some((_, counts)) = self.browse_facets.borrow().library.as_ref().filter(|(found, _)| *found == key) {
            return Ok(counts.clone());
        }
        let mut language_counts = Vec::new();
        let mut format_counts = Vec::new();
        self.connection.library_facets(&mut |row: &rusqlite::Row<'_>| {
            let count = row.get::<_, i64>(2)?;
            let book_count = u32::try_from(count).map_err(|_| invalid_integer_column(2, "facet count must fit in u32"))?;
            match row.get::<_, i64>(0)? {
                0 => {
                    // Rows are per stored format; several formats share one filter.
                    add_format_count(&mut format_counts, row.get(1)?, book_count);
                }
                1 => language_counts.push(LibraryLanguageCount { language: row.get(1)?, book_count }),
                _ => return Err(invalid_integer_column(0, "unknown facet kind")),
            }
            Ok(())
        })?;
        let counts = (language_counts, format_counts);
        self.browse_facets.borrow_mut().library = Some((key, counts.clone()));
        Ok(counts)
    }

    pub(super) fn catalog_facets(&self, catalog: &Connection, options: &BrowseOptions) -> Result<Counts, DatabaseError> {
        let release = catalog.query_row("SELECT version,release_id,format_version,taxonomy_revision FROM catalog_browse_revision WHERE singleton=1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional()?;
        let key = (catalog.total_changes(), catalog.pragma_query_value(None, "data_version", |row| row.get(0))?, release);
        let cached = self.browse_facets.borrow().catalog.as_ref().filter(|(found, _)| *found == key).map(|(_, counts)| counts.clone());
        let (mut languages, mut formats) = match cached {
            Some(counts) => counts,
            None => {
                let unselected = BrowseOptions { languages: String::new(), file_types: 0, ..options.clone() };
                let counts = super::catalog::facets_in(catalog, &unselected)?;
                self.browse_facets.borrow_mut().catalog = Some((key, counts.clone()));
                counts
            }
        };
        include_selected_facets(&mut languages, &mut formats, &options.languages, options.file_types);
        Ok((languages, formats))
    }
}

/// Library queries supply grouped counts; catalog entries supply one each.
/// Both use the same stored-format categories.
pub(super) fn add_format_count(counts: &mut Vec<LibraryFormatCount>, format: Option<i64>, book_count: u32) {
    let format = file_type_of(format);
    match counts.iter_mut().find(|count| count.format == format) {
        Some(count) => count.book_count += book_count,
        None => counts.push(LibraryFormatCount { format, book_count }),
    }
}

/// Selected filters stay listed even when no book matches them. Languages
/// sort by code and formats by their filter order.
pub(super) fn include_selected_facets(language_counts: &mut Vec<LibraryLanguageCount>, format_counts: &mut Vec<LibraryFormatCount>, languages: &str, file_types: i32) {
    for language in languages.trim_matches(',').split(',').filter(|language| !language.is_empty()) {
        if !language_counts.iter().any(|count| count.language == language) {
            language_counts.push(LibraryLanguageCount { language: language.to_owned(), book_count: 0 });
        }
    }
    for format in [LibraryFileTypeFilter::Book, LibraryFileTypeFilter::Pdf, LibraryFileTypeFilter::Audiobook] {
        if file_types & file_type_query_value(format) != 0 && !format_counts.iter().any(|count| count.format == format) {
            format_counts.push(LibraryFormatCount { format, book_count: 0 });
        }
    }
    language_counts.sort_by(|left, right| left.language.cmp(&right.language));
    format_counts.sort_by_key(|count| LibraryFileTypeFilter::ALL.iter().position(|format| *format == count.format));
}
