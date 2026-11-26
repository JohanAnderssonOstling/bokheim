//! Indexed description-only edition evidence, independent of LCC coverage.
use super::openlibrary::open_description_pool;
use super::work_edition_index::identity;
use crate::*;
use rusqlite::params_from_iter;

pub(super) const VERSION: i64 = 1;
pub(super) const SCHEMA: &str = "
CREATE TABLE description_import_setting(singleton INTEGER PRIMARY KEY CHECK(singleton=1),builder_version INTEGER NOT NULL,record_limit INTEGER,dump_date TEXT NOT NULL) STRICT;
CREATE TABLE description_import_input(singleton INTEGER PRIMARY KEY CHECK(singleton=1),path TEXT NOT NULL,size_bytes INTEGER NOT NULL,modified_ms INTEGER NOT NULL) STRICT;
CREATE TABLE import_checkpoint(phase TEXT PRIMARY KEY,records_read INTEGER NOT NULL,completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0,1))) WITHOUT ROWID;
CREATE TABLE index_source(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,source_bytes INTEGER NOT NULL,source_modified_ns TEXT NOT NULL) STRICT;
CREATE TABLE edition_description(edition_id INTEGER PRIMARY KEY,description TEXT NOT NULL) STRICT;
CREATE TABLE edition_description_work(work_id INTEGER NOT NULL,edition_id INTEGER NOT NULL,PRIMARY KEY(work_id,edition_id)) WITHOUT ROWID;
CREATE TABLE edition_description_isbn(isbn13 INTEGER NOT NULL,edition_id INTEGER NOT NULL,PRIMARY KEY(isbn13,edition_id)) WITHOUT ROWID;
";

pub(crate) fn index_path(source: &Path) -> PathBuf {
    let mut name = source.as_os_str().to_os_string();
    name.push(".edition-descriptions.sqlite3");
    PathBuf::from(name)
}

pub(crate) fn validated_index_path(source: &Path) -> Result<Option<PathBuf>, MetadataError> {
    let path = index_path(source);
    if !path.try_exists().map_err(error)? {
        return Ok(None);
    }
    let c = super::read_connection(&path)?;
    let (version, bytes, modified): (i64, i64, String) = c.query_row("SELECT version,source_bytes,source_modified_ns FROM index_source WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(error)?;
    if version != VERSION || (bytes, modified) != identity(source)? {
        return Err(MetadataError("edition description supplement is stale; rebuild it from the configured work source and matching edition dump".into()));
    }
    for sql in ["SELECT edition_id,description FROM edition_description LIMIT 0", "SELECT work_id,edition_id FROM edition_description_work LIMIT 0", "SELECT isbn13,edition_id FROM edition_description_isbn LIMIT 0"] {
        c.prepare(sql).map_err(error)?;
    }
    Ok(Some(path))
}

#[derive(Clone)]
pub(crate) struct EditionDescriptions {
    pool: Pool<SqliteConnectionManager>,
    description_indexes: bool,
}

impl EditionDescriptions {
    pub(crate) fn require_separate(&self, demand: &Path) -> Result<(), MetadataError> {
        crate::snapshot::require_separate(&*self.pool.get().map_err(error)?, demand, "demand")
    }

    pub(crate) fn open(source: &Path, imported: &Pool<SqliteConnectionManager>) -> Result<Option<Self>, MetadataError> {
        if let Some(path) = validated_index_path(source)? {
            return Ok(Some(Self { pool: open_description_pool(&path)?, description_indexes: true }));
        }
        let c = imported.get().map_err(error)?;
        let present: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='edition_description')", [], |r| r.get(0)).map_err(error)?;
        let description_indexes: bool = c.query_row("SELECT count(*)=2 FROM sqlite_master WHERE type='table' AND name IN ('edition_description_work','edition_description_isbn')", [], |r| r.get(0)).map_err(error)?;
        Ok(present.then(|| Self { pool: imported.clone(), description_indexes }))
    }

    /// An exact ISBN takes precedence over descriptions from sibling editions.
    /// Missing values do not conflict; different nonempty descriptions abstain.
    pub(crate) fn description(&self, isbns: &[String], works: &BTreeSet<i64>) -> Result<Option<String>, MetadataError> {
        let c = self.pool.get().map_err(error)?;
        let isbns = isbns.iter().filter_map(|isbn| Isbn13::parse(isbn)).collect::<BTreeSet<_>>();
        if !isbns.is_empty() {
            let table = if self.description_indexes { "edition_description_isbn" } else { "edition_isbn" };
            let sql = format!("SELECT DISTINCT trim(d.description) FROM {table} i JOIN edition_description d USING(edition_id) WHERE i.isbn13 IN ({}) AND trim(d.description)!='' LIMIT 2", sql_placeholders(isbns.len()));
            let mut statement = c.prepare_cached(&sql).map_err(error)?;
            let values = statement.query_map(params_from_iter(isbns), |r| r.get::<_, String>(0)).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            if !values.is_empty() {
                return Ok((values.len() == 1).then(|| values.into_iter().next().expect("one description")));
            }
        }
        if works.is_empty() {
            return Ok(None);
        }
        let table = if self.description_indexes { "edition_description_work" } else { "edition" };
        let sql = format!("SELECT DISTINCT trim(d.description) FROM {table} e JOIN edition_description d USING(edition_id) WHERE e.work_id IN ({}) AND trim(d.description)!='' LIMIT 2", sql_placeholders(works.len()));
        let mut statement = c.prepare_cached(&sql).map_err(error)?;
        let values = statement.query_map(params_from_iter(works), |r| r.get::<_, String>(0)).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
        Ok((values.len() == 1).then(|| values.into_iter().next().expect("one description")))
    }
}
