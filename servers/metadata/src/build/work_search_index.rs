//! Indexed title/author candidates; original metadata still decides identity.
use super::work_edition_index::identity;
use crate::*;
use metadata_contract::matching::{author_credits, NormalizedAuthor};

// Bump when author normalization, aliases, search keys or hashing change.
const VERSION: i64 = 1;
pub(crate) const MAX_AUTHOR_KEYS: usize = 512;

fn index_path(source: &Path) -> PathBuf {
    let mut name = source.as_os_str().to_os_string();
    name.push(".work-search.sqlite3");
    PathBuf::from(name)
}

fn hash_key(value: &str) -> [u8; 8] {
    blake3::hash(value.as_bytes()).as_bytes()[..8].try_into().unwrap()
}

/// Compatible names either share a full word and another word's initial,
/// or agree on all name words (including mononyms and initial-only credits).
/// Include every normalized variant so aliases and omitted names remain eligible.
pub(crate) fn author_keys(names: &[String]) -> BTreeSet<[u8; 8]> {
    let mut keys = BTreeSet::new();
    for name in author_credits(names) {
        if matches!(name.trim().to_ascii_lowercase().as_str(), "unknown" | "untitled") {
            continue;
        }
        let author = NormalizedAuthor::new(&name);
        for variant in author.variants {
            let mut words = variant.words;
            for (i, word) in words.iter().enumerate().filter(|(_, word)| word.chars().count() > 1) {
                for initial in words.iter().enumerate().filter(|(j, _)| *j != i).filter_map(|(_, other)| other.chars().next()).collect::<BTreeSet<_>>() {
                    keys.insert(hash_key(&format!("word:{word}|initial:{initial}")));
                }
            }
            // Sorted words also retain initial-only credits in any name order.
            words.sort_unstable();
            keys.insert(hash_key(&format!("exact:{}", words.join(" "))));
        }
    }
    keys
}

pub(crate) fn search_key(title: &str, author: &[u8; 8]) -> [u8; 16] {
    let mut key = [0; 16];
    key[..8].copy_from_slice(&hash_key(title));
    key[8..].copy_from_slice(author);
    key
}

pub(crate) fn open_index(source: &Path) -> Result<Pool<SqliteConnectionManager>, MetadataError> {
    let path = index_path(source);
    if !path.try_exists().map_err(error)? {
        return Err(MetadataError("work lookup requires title/author indexes; run metadata-server index-work-search SNAPSHOT.sqlite".into()));
    }
    let pool = super::openlibrary::open_description_pool(&path)?;
    let c = pool.get().map_err(error)?;
    let (version, bytes, modified): (i64, i64, String) = c.query_row("SELECT version,source_bytes,source_modified_ns FROM index_source WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(error)?;
    if version != VERSION || (bytes, modified) != identity(source)? {
        return Err(MetadataError("work search index is stale; rebuild it from the configured immutable work source".into()));
    }
    c.prepare("SELECT key,work_id FROM work_search LIMIT 0").map_err(error)?;
    drop(c);
    Ok(pool)
}

pub fn index_work_search(source: &Path) -> Result<serde_json::Value, MetadataError> {
    let started = Instant::now();
    let source = source.canonicalize().map_err(error)?;
    let output = index_path(&source);
    if output.try_exists().map_err(error)? {
        return Err(MetadataError(format!("{} already exists; build from a new source or remove the obsolete index", output.display())));
    }
    let before = identity(&source)?;
    let temporary = super::openlibrary::building_path(&output);
    let reservation = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(error)?;
    drop(reservation);
    let result = (|| {
        let input = super::read_connection(&source)?;
        super::validate_source(&input)?;
        input.execute_batch("PRAGMA query_only=ON; PRAGMA cache_size=-16384;").map_err(error)?;
        let c = Connection::open(&temporary).map_err(error)?;
        c.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA temp_store=FILE; PRAGMA cache_size=-65536;
            CREATE TABLE index_source(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,source_bytes INTEGER NOT NULL,source_modified_ns TEXT NOT NULL);
            CREATE TABLE author_key(author_id INTEGER NOT NULL,key BLOB NOT NULL,PRIMARY KEY(author_id,key)) WITHOUT ROWID;
            CREATE TABLE title_key(work_id INTEGER NOT NULL,key BLOB NOT NULL,PRIMARY KEY(work_id,key)) WITHOUT ROWID;
            CREATE TABLE work_search(key BLOB NOT NULL,work_id INTEGER NOT NULL,PRIMARY KEY(key,work_id)) WITHOUT ROWID;
            BEGIN;",
        )
        .map_err(error)?;
        let mut authors = 0_u64;
        {
            let mut query = input.prepare("SELECT author_id,name FROM author WHERE name IS NOT NULL ORDER BY author_id").map_err(error)?;
            let mut insert = c.prepare("INSERT OR IGNORE INTO author_key VALUES(?1,?2)").map_err(error)?;
            let mut rows = query.query([]).map_err(error)?;
            while let Some(row) = rows.next().map_err(error)? {
                let id: i64 = row.get(0).map_err(error)?;
                let name: String = row.get(1).map_err(error)?;
                for key in author_keys(&[name]) {
                    insert.execute(params![id, key.as_slice()]).map_err(error)?;
                }
                authors += 1;
                if authors % 500_000 == 0 {
                    tracing::info!(authors, elapsed_seconds = started.elapsed().as_secs_f64(), "normalizing work author search keys");
                }
            }
        }
        tracing::info!(authors, "indexing work title keys");
        {
            let mut query = input.prepare("SELECT work_id,normalized_title FROM work_bibliography WHERE work_id>0 UNION ALL SELECT work_id,normalized_title FROM work_subtitle WHERE work_id>0 ORDER BY work_id").map_err(error)?;
            let mut insert = c.prepare("INSERT OR IGNORE INTO title_key VALUES(?1,?2)").map_err(error)?;
            let mut rows = query.query([]).map_err(error)?;
            while let Some(row) = rows.next().map_err(error)? {
                let id: i64 = row.get(0).map_err(error)?;
                let title: String = row.get(1).map_err(error)?;
                insert.execute(params![id, hash_key(&title).as_slice()]).map_err(error)?;
            }
        }
        drop(input);
        c.execute("ATTACH DATABASE ?1 AS source", [crate::snapshot::immutable_uri(&source)?]).map_err(error)?;
        c.execute_batch("PRAGMA source.cache_size=-16384;").map_err(error)?;
        tracing::info!("joining title and author search keys");
        let keys = c
            .execute(
                "INSERT INTO work_search SELECT DISTINCT CAST(t.key || a.key AS BLOB),w.work_id FROM source.work_author w CROSS JOIN title_key t CROSS JOIN author_key a WHERE t.work_id=w.work_id AND a.author_id=w.author_id ORDER BY 1,2",
                [],
            )
            .map_err(error)?;
        if identity(&source)? != before {
            return Err(MetadataError("work source changed while building title/author indexes".into()));
        }
        c.execute("INSERT INTO index_source VALUES(1,?1,?2,?3)", params![VERSION, before.0, before.1]).map_err(error)?;
        tracing::info!(keys, "finalizing work search index");
        c.execute_batch("DROP TABLE author_key; DROP TABLE title_key; COMMIT; DETACH DATABASE source; VACUUM main; ANALYZE main; PRAGMA main.optimize;").map_err(error)?;
        let check: String = c.query_row("PRAGMA main.quick_check", [], |r| r.get(0)).map_err(error)?;
        if check != "ok" {
            return Err(MetadataError(format!("work search index integrity check failed: {check}")));
        }
        Ok((authors, keys))
    })();
    match result {
        Ok((authors, keys)) => {
            fs::rename(&temporary, &output).map_err(error)?;
            Ok(serde_json::json!({"output":output,"authors":authors,"search_keys":keys,"bytes":fs::metadata(output).map_err(error)?.len(),"elapsed_seconds":started.elapsed().as_secs_f64()}))
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}
