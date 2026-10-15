//! An immutable base plus one cumulative immutable update file. The update's
//! embedded manifest pins the base; temporary views merge them without copying it.
use crate::{error, schema, Connection, MetadataError, OpenFlags, Path, PathBuf, Pool, SqliteConnectionManager, QUERY_CONCURRENCY, READ_CACHE_KIB, READ_MMAP_BYTES};
use reqwest::Url;
use rusqlite::{params, Error as SqliteError};
use serde::{Deserialize, Serialize};
use std::{fs, time::UNIX_EPOCH};

const OVERLAY_VERSION: i64 = 1;
const TABLES: &[&str] = &["isbn_answer", "work_description", "edition_candidate", "edition_title_key", "authority_title", "wanted", "answer_classification", "answer_work_lcc", "answer_provider", "answer_demand"];

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BaseIdentity {
    path: PathBuf,
    bytes: u64,
    modified_ns: u128,
}

impl BaseIdentity {
    fn read(path: &Path) -> Result<Self, MetadataError> {
        let path = path.canonicalize().map_err(error)?;
        let metadata = fs::metadata(&path).map_err(error)?;
        Ok(Self { path, bytes: metadata.len(), modified_ns: metadata.modified().map_err(error)?.duration_since(UNIX_EPOCH).map_err(error)?.as_nanos() })
    }
}

fn immutable_uri(path: &Path) -> Result<String, MetadataError> {
    let mut url = Url::from_file_path(path.canonicalize().map_err(error)?).map_err(|_| MetadataError("invalid immutable snapshot path".into()))?;
    // Ordinary SQLite readers observe committed in-place maintenance and WAL.
    // `immutable=1` would retain stale pages after such an update.
    url.set_query(Some("mode=ro"));
    Ok(url.into())
}

pub(crate) fn open(path: &Path) -> Result<Connection, MetadataError> {
    let c = Connection::open_with_flags(immutable_uri(path)?, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).map_err(error)?;
    configure(&c, false)?;
    c.execute_batch("PRAGMA query_only=ON").map_err(error)?;
    Ok(c)
}

pub(crate) fn pool(path: &Path) -> Result<Pool<SqliteConnectionManager>, MetadataError> {
    // Validate synchronously so a missing/replaced base is reported directly,
    // rather than waiting for a connection-pool timeout.
    open(path)?;
    let manager = SqliteConnectionManager::file(immutable_uri(path)?).with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX).with_init(|c| {
        configure(c, false).map_err(|e| SqliteError::ToSqlConversionFailure(Box::new(e)))?;
        c.execute_batch("PRAGMA query_only=ON")
    });
    Pool::builder().max_size(QUERY_CONCURRENCY).min_idle(Some(1)).build(manager).map_err(error)
}

fn manifest(c: &Connection) -> Result<Option<BaseIdentity>, MetadataError> {
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE name='answer_overlay' AND type='table')", [], |r| r.get(0)).map_err(error)?;
    if !exists {
        return Ok(None);
    }
    let (version, identity): (i64, String) = c.query_row("SELECT version,base FROM main.answer_overlay WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)?;
    if version != OVERLAY_VERSION {
        return Err(MetadataError(format!("unsupported answer overlay version {version}")));
    }
    Ok(Some(serde_json::from_str(&identity).map_err(error)?))
}

pub(crate) fn base_path(c: &Connection) -> Result<PathBuf, MetadataError> {
    if let Some(base) = manifest(c)? {
        return Ok(base.path);
    }
    let path: String = c.query_row("SELECT file FROM pragma_database_list WHERE name='main'", [], |r| r.get(0)).map_err(error)?;
    Path::new(&path).canonicalize().map_err(error)
}

pub(crate) fn is_update(c: &Connection) -> Result<bool, MetadataError> {
    Ok(manifest(c)?.is_some())
}

pub(crate) fn require_separate(c: &Connection, mutable: &Path, label: &str) -> Result<(), MetadataError> {
    if !mutable.exists() {
        return Ok(());
    }
    let target = mutable.canonicalize().map_err(error)?;
    let main: String = c.query_row("SELECT file FROM pragma_database_list WHERE name='main'", [], |r| r.get(0)).map_err(error)?;
    for immutable in [PathBuf::from(main), base_path(c)?] {
        let same = target == immutable;
        #[cfg(unix)]
        let same = {
            use std::os::unix::fs::MetadataExt;
            let a = fs::metadata(&target).map_err(error)?;
            let b = fs::metadata(&immutable).map_err(error)?;
            same || (a.dev(), a.ino()) == (b.dev(), b.ino())
        };
        if same {
            return Err(MetadataError(format!("{label} database must be separate from immutable answer files")));
        }
    }
    Ok(())
}

pub(crate) fn create_update(previous: &Path, output: &Path) -> Result<Connection, MetadataError> {
    let old = open(previous)?;
    if manifest(&old)?.is_some() {
        // Published updates stay immutable too. Copy only their cumulative delta,
        // always referencing the original base: never grow a chain of layers.
        fs::copy(previous, output).map_err(error)?;
    }
    let out = Connection::open(output).map_err(error)?;
    out.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=-65536;").map_err(error)?;
    if manifest(&old)?.is_none() {
        let base = BaseIdentity::read(previous)?;
        out.execute_batch(schema::ANSWERS).map_err(error)?;
        out.execute_batch(crate::build::incremental::STATE_SCHEMA).map_err(error)?;
        out.execute_batch("CREATE INDEX edition_candidate_by_work ON edition_candidate(work_id); CREATE INDEX edition_title_key_by_edition ON edition_title_key(edition_id); CREATE INDEX answer_provider_by_isbn ON answer_provider(isbn13,provider); CREATE TABLE answer_overlay(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,base TEXT NOT NULL);").map_err(error)?;
        for table in TABLES {
            let columns = columns(&out, table)?;
            let keys = primary_keys(&columns)?;
            let definitions = keys.iter().map(|c| format!("{} {} NOT NULL", c.name, c.kind)).collect::<Vec<_>>().join(",");
            let names = keys.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(",");
            out.execute_batch(&format!("ALTER TABLE {table} RENAME TO delta_{table}; CREATE TABLE overlay_key_{table}({definitions},PRIMARY KEY({names})) WITHOUT ROWID;")).map_err(error)?;
        }
        out.execute("INSERT INTO answer_overlay VALUES(1,?1,?2)", params![OVERLAY_VERSION, serde_json::to_string(&base).map_err(error)?]).map_err(error)?;
        configure(&out, true)?;
        out.execute_batch("INSERT INTO main.snapshot SELECT * FROM base.snapshot;").map_err(error)?;
        if old.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='answer_build')", [], |r| r.get::<_, bool>(0)).map_err(error)? {
            out.execute_batch("INSERT INTO main.answer_build SELECT * FROM base.answer_build;").map_err(error)?;
        }
    } else {
        configure(&out, true)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build::tests::snapshot_from, build_answers, build_answers_incremental, Classification, ClassificationScheme, ClassificationSource, MetadataService};

    #[test]
    fn updates_and_deletions_do_not_copy_or_mutate_the_base_and_use_indexed_reads() {
        let temp = tempfile::tempdir().unwrap();
        let source = snapshot_from(
            temp.path(),
            &[serde_json::json!({"key":"/books/OL1M","title":"Changed title","isbn_13":["9780306406157"]}), serde_json::json!({"key":"/books/OL2M","title":"Unchanged title","isbn_13":["9780131103627"],"lc_classifications":["E1"]})],
            &[],
            &[],
        );
        let provider_path = temp.path().join("provider.sqlite");
        let provider = crate::crawl::open_provider(&provider_path).unwrap();
        let base = temp.path().join("base.sqlite");
        build_answers(&source, Some(&provider_path), None, None, &base).unwrap();
        // Extra base pages demonstrate that update size is independent of base
        // size, without adding irrelevant resolution work to the fixture.
        Connection::open(&base).unwrap().execute_batch("CREATE TABLE immutable_padding(value BLOB); INSERT INTO immutable_padding VALUES(zeroblob(8388608));
            WITH RECURSIVE ids(id) AS (VALUES(1000) UNION ALL SELECT id+1 FROM ids WHERE id<6000)
            INSERT INTO edition_candidate SELECT id,isbn13,NULL,'Unrelated title',NULL,NULL,'[]','[]',NULL,classifications FROM ids,edition_candidate WHERE edition_id=2;
            INSERT INTO edition_title_key SELECT 'unrelated-'||edition_id,edition_id,1,0 FROM edition_candidate WHERE edition_id>=1000;
            ANALYZE;").unwrap();
        let original = fs::read(&base).unwrap();
        let codes = serde_json::to_vec(&vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76".into(), source: ClassificationSource::ExactEdition, evidence: vec![] }]).unwrap();
        provider.execute("INSERT INTO provider_result VALUES('loc',9780306406157,1,'matched',?1,?2)", params![codes, b"[]".as_slice()]).unwrap();
        let changed = temp.path().join("changed.sqlite");
        let report = build_answers_incremental(&source, Some(&provider_path), None, None, None, &base, &changed).unwrap();
        assert_eq!(report.resolved_rows, 1);
        assert!(report.file_bytes < original.len() as u64 / 10);
        let raw = Connection::open(&changed).unwrap();
        assert_eq!(raw.query_row("SELECT count(*) FROM delta_isbn_answer", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        let c = open(&changed).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM isbn_answer", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert!(MetadataService::open(&changed, &base).is_err());
        let alias = temp.path().join("base-alias.sqlite");
        fs::hard_link(&base, &alias).unwrap();
        assert!(MetadataService::open(&changed, &alias).is_err());
        assert!(require_separate(&c, &alias, "provider").is_err());
        let mut plan = c.prepare(&format!("EXPLAIN QUERY PLAN {}", crate::serve::identity::CANDIDATE_QUERY)).unwrap();
        let details = plan.query_map(["Unchanged title"], |r| r.get::<_, String>(3)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert!(details.iter().any(|d| d.contains("SEARCH b USING INTEGER PRIMARY KEY")), "{details:?}");
        assert!(!details.iter().any(|d| d == "SCAN b" || d.contains("SCAN base.edition_candidate")), "{details:?}");
        let server = MetadataService::open(&changed, &temp.path().join("serve-demand.sqlite")).unwrap();
        let answers = server.enrich_rich(vec!["9780306406157".into(), "9780131103627".into()]).unwrap();
        assert_eq!(answers[0].matches[0].classifications[0].notation, "QA76");
        assert_eq!(answers[1].matches[0].classifications[0].notation, "E1");
        // A reader already opened on an old immutable generation stays pinned.
        provider.execute_batch("DELETE FROM provider_result").unwrap();
        let deleted = temp.path().join("deleted.sqlite");
        build_answers_incremental(&source, Some(&provider_path), None, None, None, &changed, &deleted).unwrap();
        let next = MetadataService::open(&deleted, &temp.path().join("next-demand.sqlite")).unwrap();
        assert!(next.enrich_rich(vec!["9780306406157".into()]).unwrap()[0].matches[0].classifications.is_empty());
        assert_eq!(server.enrich_rich(vec!["9780306406157".into()]).unwrap()[0].matches[0].classifications[0].notation, "QA76");
        assert!(fs::metadata(&deleted).unwrap().len() < original.len() as u64 / 10);
        let second_base = manifest(&open(&deleted).unwrap()).unwrap().unwrap();
        assert_eq!(second_base.path, base.canonicalize().unwrap());
        assert_eq!(fs::read(&base).unwrap(), original);
        // The builder's attached base is read-only even before query_only is set.
        let scratch = temp.path().join("scratch.sqlite");
        let writer = create_update(&deleted, &scratch).unwrap();
        assert!(writer.execute("DELETE FROM base.isbn_answer", []).is_err());
        assert_eq!(fs::read(&base).unwrap(), original);
    }

    #[test]
    fn missing_or_replaced_bases_are_rejected_before_serving() {
        let temp = tempfile::tempdir().unwrap();
        let source = snapshot_from(temp.path(), &[], &[], &[]);
        let base = temp.path().join("base.sqlite");
        build_answers(&source, None, None, None, &base).unwrap();
        let update = temp.path().join("update.sqlite");
        drop(create_update(&base, &update).unwrap());
        Connection::open(&base).unwrap().execute_batch("CREATE TABLE replacement(value BLOB); INSERT INTO replacement VALUES(zeroblob(8192));").unwrap();
        assert!(open(&update).unwrap_err().to_string().contains("immutable answer base changed"));
        assert!(MetadataService::open(&update, &temp.path().join("demand.sqlite")).is_err());
        fs::remove_file(base).unwrap();
        assert!(open(&update).is_err());
    }
}

struct Column {
    name: String,
    kind: String,
    key_order: i64,
}

fn columns(c: &Connection, table: &str) -> Result<Vec<Column>, MetadataError> {
    let mut q = c.prepare(&format!("PRAGMA main.table_info({table})")).map_err(error)?;
    let rows = q.query_map([], |r| Ok(Column { name: r.get(1)?, kind: r.get(2)?, key_order: r.get(5)? })).map_err(error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(error)
}

fn primary_keys(columns: &[Column]) -> Result<Vec<&Column>, MetadataError> {
    let mut keys = columns.iter().filter(|c| c.key_order > 0).collect::<Vec<_>>();
    keys.sort_by_key(|c| c.key_order);
    if keys.is_empty() {
        return Err(MetadataError("overlay table has no primary key".into()));
    }
    Ok(keys)
}

fn configure(c: &Connection, writable: bool) -> Result<(), MetadataError> {
    c.set_prepared_statement_cache_capacity(64);
    c.execute_batch(&format!("PRAGMA cache_size=-{READ_CACHE_KIB}; PRAGMA mmap_size={READ_MMAP_BYTES};")).map_err(error)?;
    let Some(base) = manifest(c)? else { return Ok(()) };
    if base != BaseIdentity::read(&base.path)? {
        return Err(MetadataError("immutable answer base changed; refusing to open its overlay".into()));
    }
    let main: String = c.query_row("SELECT file FROM pragma_database_list WHERE name='main'", [], |r| r.get(0)).map_err(error)?;
    if Path::new(&main).canonicalize().map_err(error)? == base.path {
        return Err(MetadataError("answer overlay cannot reference itself".into()));
    }
    c.execute("ATTACH DATABASE ?1 AS base", [immutable_uri(&base.path)?]).map_err(error)?;
    let layered: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM base.sqlite_master WHERE name='answer_overlay')", [], |r| r.get(0)).map_err(error)?;
    if layered {
        return Err(MetadataError("answer overlay must reference a full base, not another overlay".into()));
    }
    c.execute_batch(&format!("PRAGMA base.cache_size=-{READ_CACHE_KIB}; PRAGMA base.mmap_size={READ_MMAP_BYTES};")).map_err(error)?;
    for table in TABLES {
        let physical = format!("delta_{table}");
        let columns = columns(c, &physical)?;
        let keys = primary_keys(&columns)?;
        let match_base = keys.iter().map(|k| format!("t.{}=b.{}", k.name, k.name)).collect::<Vec<_>>().join(" AND ");
        let in_base: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM base.sqlite_master WHERE name=?1 AND type='table')", [table], |r| r.get(0)).map_err(error)?;
        let optional = matches!(*table, "answer_classification" | "answer_work_lcc" | "answer_provider" | "answer_demand");
        if !in_base && !optional {
            return Err(MetadataError(format!("immutable answer base is missing required table {table}")));
        }
        let base_rows = if in_base { format!(" UNION ALL SELECT b.* FROM base.{table} b WHERE NOT EXISTS(SELECT 1 FROM main.overlay_key_{table} t WHERE {match_base})") } else { String::new() };
        c.execute_batch(&format!("CREATE TEMP VIEW {table} AS SELECT * FROM main.{physical}{base_rows};")).map_err(error)?;
        if writable {
            let names = columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(",");
            let new_values = columns.iter().map(|c| format!("NEW.{}", c.name)).collect::<Vec<_>>().join(",");
            let key_names = keys.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(",");
            let new_keys = keys.iter().map(|c| format!("NEW.{}", c.name)).collect::<Vec<_>>().join(",");
            let old_keys = keys.iter().map(|c| format!("OLD.{}", c.name)).collect::<Vec<_>>().join(",");
            let match_old = keys.iter().map(|c| format!("{}=OLD.{}", c.name, c.name)).collect::<Vec<_>>().join(" AND ");
            let match_new = keys.iter().map(|c| format!("{}=NEW.{}", c.name, c.name)).collect::<Vec<_>>().join(" AND ");
            let insert = if *table == "work_description" {
                // Preserve the existing shared description, including when it
                // lives only in the base. Conflicts stay embedded in the answer.
                format!("INSERT OR IGNORE INTO {physical}({names}) SELECT {new_values} WHERE NOT EXISTS(SELECT 1 FROM {table} WHERE {match_new}); INSERT OR IGNORE INTO overlay_key_{table}({key_names}) SELECT {new_keys} WHERE EXISTS(SELECT 1 FROM {physical} WHERE {match_new});")
            } else {
                format!("INSERT OR REPLACE INTO {physical}({names}) VALUES({new_values}); INSERT OR REPLACE INTO overlay_key_{table}({key_names}) VALUES({new_keys});")
            };
            c.execute_batch(&format!("
                CREATE TEMP TRIGGER overlay_insert_{table} INSTEAD OF INSERT ON {table} BEGIN {insert} END;
                CREATE TEMP TRIGGER overlay_delete_{table} INSTEAD OF DELETE ON {table} BEGIN DELETE FROM {physical} WHERE {match_old}; INSERT OR REPLACE INTO overlay_key_{table}({key_names}) VALUES({old_keys}); END;
                CREATE TEMP TRIGGER overlay_update_{table} INSTEAD OF UPDATE ON {table} BEGIN DELETE FROM {physical} WHERE {match_old}; INSERT OR REPLACE INTO overlay_key_{table}({key_names}) VALUES({old_keys}); INSERT OR REPLACE INTO {physical}({names}) VALUES({new_values}); INSERT OR REPLACE INTO overlay_key_{table}({key_names}) VALUES({new_keys}); END;
            ")).map_err(error)?;
        }
    }
    Ok(())
}
