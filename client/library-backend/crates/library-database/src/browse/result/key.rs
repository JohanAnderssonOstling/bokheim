//! The dependencies of each browse scope and the revisions that identify it.
use super::{BrowseLocation, BrowseOptions, Connection, Database, DatabaseError, LibraryBrowseQuery};
use crate::browse::revision::{self, Revision};
use rusqlite::OptionalExtension;

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub(super) struct ResultKey {
    pub(super) location: BrowseLocation,
    pub(super) options: BrowseOptions,
    library: Revision,
    taxonomy: Option<TaxonomyRevision>,
    catalog: Option<CatalogRevision>,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
struct TaxonomyRevision {
    data_version: i64,
    schema_version: i64,
    revision: Option<String>,
    local_changes: Option<u64>,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
struct CatalogRevision {
    generation: u64,
    data_version: i64,
    release: Option<(i64, i64, i64, String)>,
}

/// Scope policy lives here; callers do not clear unrelated revision counters.
#[derive(Default)]
pub(super) struct Dependencies {
    pub(super) catalog: bool,
    subjects: bool,
    authors: bool,
    folders: bool,
}

impl Dependencies {
    pub(super) fn for_location(location: BrowseLocation) -> Result<Self, DatabaseError> {
        Ok(match location {
            BrowseLocation::Catalog(_) => Self { catalog: true, subjects: true, ..Self::default() },
            BrowseLocation::CatalogAuthor(_) => Self { catalog: true, ..Self::default() },
            BrowseLocation::Subject(_) => Self { subjects: true, ..Self::default() },
            BrowseLocation::Author(_) => Self { authors: true, ..Self::default() },
            BrowseLocation::Folder(_) => Self { folders: true, ..Self::default() },
            BrowseLocation::Books | BrowseLocation::Home(_) => Self::default(),
            _ => return Err(DatabaseError::message("this location does not own a paged browse result")),
        })
    }

    fn library_revision(&self, revision: Revision) -> Revision {
        if self.catalog {
            // Library membership only affects the catalog's held-book exclusions.
            return Revision { visibility: revision.visibility, legacy: revision.legacy, ..Revision::default() };
        }
        Revision { subjects: if self.subjects { revision.subjects } else { 0 }, authors: if self.authors { revision.authors } else { 0 }, folders: if self.folders { revision.folders } else { 0 }, ..revision }
    }

    pub(super) fn read_key(&self, db: &Database, query: &LibraryBrowseQuery, downloaded_only: bool, catalog: Option<&Connection>, generation: u64) -> Result<ResultKey, DatabaseError> {
        // Pin the library WAL snapshot before reading its revision counters.
        db.connection.query_row("SELECT count(*) FROM main.sqlite_schema", [], |row| row.get::<_, i64>(0))?;
        Ok(ResultKey {
            location: query.location,
            options: BrowseOptions::from_query(query, downloaded_only),
            library: self.library_revision(Revision::read(&db.connection)?),
            taxonomy: if self.subjects { Some(TaxonomyRevision::read(&db.connection)?) } else { None },
            catalog: catalog.map(|catalog| CatalogRevision::read(catalog, generation)).transpose()?,
        })
    }
}

impl TaxonomyRevision {
    fn read(connection: &Connection) -> Result<Self, DatabaseError> {
        Ok(Self {
            data_version: connection.pragma_query_value(Some("curated"), "data_version", |row| row.get(0))?,
            schema_version: connection.pragma_query_value(Some("curated"), "schema_version", |row| row.get(0))?,
            revision: revision::taxonomy(connection)?,
            local_changes: (!connection.is_readonly("curated")?).then(|| connection.total_changes()),
        })
    }
}

impl CatalogRevision {
    fn read(connection: &Connection, generation: u64) -> Result<Self, DatabaseError> {
        Ok(Self {
            generation,
            data_version: connection.pragma_query_value(None, "data_version", |row| row.get(0))?,
            release: connection.query_row("SELECT version,release_id,format_version,taxonomy_revision FROM catalog_browse_revision WHERE singleton=1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional()?,
        })
    }
}
