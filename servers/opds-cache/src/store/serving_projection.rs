//! Bounded, replayable projection into an independent WAL database.
use super::*;
use std::sync::Mutex;

const BATCH: usize = 256;
pub(super) struct ServingStore {
    pool: Pool<SqliteConnectionManager>,
    writer: Mutex<std::fs::File>,
}

pub(super) struct ProjectorGuard<'a>(std::sync::MutexGuard<'a, std::fs::File>);
impl Drop for ProjectorGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl ServingStore {
    pub fn open(authority: &std::path::Path) -> Result<Self, CatalogStoreError> {
        // A new schema/taxonomy gets its own file. Never overwrite the catalog
        // or a previous projection. The ready bit publishes the initial build.
        let mut path = authority.as_os_str().to_os_string();
        path.push(format!(".serving-v3-t{}.db", subject_projection::UNIFIED_MATCHER_VERSION));
        let mut lock_path = path.clone();
        lock_path.push(".projector-lock");
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(std::path::PathBuf::from(lock_path)).map_err(|e| CatalogStoreError::new(e.to_string()))?;
        let manager = SqliteConnectionManager::file(std::path::PathBuf::from(path)).with_init(|connection| {
            connection.busy_timeout(std::time::Duration::from_secs(10))?;
            connection.pragma_update(None, "journal_mode", "WAL")?;
            // Acknowledgements in another database must never outlive this commit.
            connection.pragma_update(None, "synchronous", "FULL")
        });
        let pool = Pool::builder().max_size(4).build(manager).map_err(|e| CatalogStoreError::new(e.to_string()))?;
        let result = Self { pool, writer: Mutex::new(lock) };
        result.connection()?.execute_batch(include_str!("serving_schema.sql"))?;
        Ok(result)
    }

    pub fn connection(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, CatalogStoreError> {
        self.pool.get().map_err(|e| CatalogStoreError::new(e.to_string()))
    }

    pub(super) fn exclusive(&self) -> Result<ProjectorGuard<'_>, CatalogStoreError> {
        let guard = self.writer.lock().map_err(|_| CatalogStoreError::new("serving projector poisoned"))?;
        guard.lock().map_err(|e| CatalogStoreError::new(e.to_string()))?;
        Ok(ProjectorGuard(guard))
    }
}

/// Source removal is a purge, unlike ordinary card retirement. Callers hold
/// the projector guard until both serving and authority deletion have committed.
pub(super) fn purge_source(connection: &Connection, source: &str, sidecar: bool) -> Result<(), CatalogStoreError> {
    if !sidecar {
        connection.execute("INSERT INTO catalog_browse_search_fts(catalog_browse_search_fts,rowid,search_text)
            SELECT 'delete',document.entry_id,document.search_text FROM catalog_browse_search_document document
            JOIN catalog_browse_book browse USING(entry_id) WHERE browse.source_id=?1", [source])?;
    }
    for table in ["catalog_browse_search_document","catalog_browse_subject","catalog_browse_language","catalog_browse_format"] {
        connection.execute(&format!("DELETE FROM {table} WHERE entry_id IN (SELECT entry_id FROM catalog_browse_book WHERE source_id=?1)"), [source])?;
    }
    connection.execute("DELETE FROM catalog_browse_book WHERE source_id=?1", [source])?;
    if sidecar {
        connection.execute("DELETE FROM projection_retired WHERE entry_id IN (SELECT entry_id FROM catalog_book WHERE source_id=?1)", [source])?;
        connection.execute("DELETE FROM catalog_book WHERE source_id=?1", [source])?;
        connection.execute("DELETE FROM projection_source WHERE source_id=?1", [source])?;
        connection.execute("UPDATE projection_state SET revision=revision+1,incarnation=lower(hex(randomblob(16)))", [])?;
        connection.execute("UPDATE catalog_browse_serving_metadata SET revision=(SELECT revision FROM projection_state)", [])?;
    }
    Ok(())
}

pub(super) fn initialize(store: &CatalogStore) -> Result<(), CatalogStoreError> {
    let _guard = store.serving.exclusive()?;
    let authority = store.connection()?;
    let epoch: String = authority.query_row("SELECT epoch FROM catalog_transport_state WHERE singleton=1", [], |r| r.get(0))?;
    let mut serving = store.serving.connection()?;
    let transaction = serving.transaction()?;
    let previous: Option<String> = transaction.query_row("SELECT authority_epoch FROM projection_state", [], |r| r.get(0)).optional()?;
    if previous.as_deref() == Some(&epoch) {
        return Ok(());
    }
    // A replaced authority invalidates IDs, even when its filename is unchanged.
    for table in [
        "catalog_browse_search_document",
        "catalog_browse_subject",
        "catalog_browse_language",
        "catalog_browse_format",
        "catalog_browse_book",
        "catalog_book",
        "projection_retired",
        "catalog_subject_node",
        "projection_source",
        "projection_state",
    ] {
        transaction.execute(&format!("DELETE FROM {table}"), [])?;
    }
    transaction.execute("INSERT INTO projection_state(singleton,authority_epoch) VALUES(1,?1)", [&epoch])?;
    transaction.execute("INSERT OR REPLACE INTO catalog_browse_serving_metadata VALUES(1,?1,0,0)", [CATALOG_BROWSE_SERVING_INDEX_VERSION])?;
    // Bootstrap from sources, not the journal: a deleted sidecar must also
    // recover books whose journal records were already acknowledged.
    let mut statement = authority.prepare("SELECT source_id FROM catalog_source")?;
    for source in statement.query_map([], |r| r.get::<_, String>(0))? {
        transaction.execute("INSERT INTO projection_source(source_id,sequence) VALUES(?1,-1)", [source?])?;
    }
    transaction.commit()?;
    Ok(())
}

pub(super) fn ready(connection: &Connection) -> Result<bool, CatalogStoreError> {
    Ok(connection.query_row("SELECT ready FROM projection_state WHERE singleton=1", [], |r| r.get(0))?)
}

pub(super) fn epoch(connection: &Connection) -> Result<String, CatalogStoreError> {
    Ok(connection
        .query_row("SELECT authority_epoch,incarnation FROM projection_state WHERE singleton=1", [], |r| Ok(format!("{}:serving-v3-t{}:{}", r.get::<_, String>(0)?, subject_projection::UNIFIED_MATCHER_VERSION, r.get::<_, String>(1)?)))?)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    source: String,
    edition: String,
    work: String,
}
impl Key {
    fn canonical_group(&self) -> bool {
        self.source == "opds-project-gutenberg" && self.work.starts_with("urn:gutenberg:")
    }
}

struct Prepared {
    key: Key,
    entry: i64,
    generation: i64,
    payload: Vec<u8>,
    title: String,
    popularity: i64,
    sequence: i64,
    logs: i64,
    ratings: i64,
    search: String,
    languages: Vec<String>,
    formats: Vec<String>,
    subjects: BTreeSet<String>,
}

fn strings(connection: &Connection, sql: &str, key: &Key, generation: i64) -> Result<Vec<String>, CatalogStoreError> {
    let mut statement = connection.prepare_cached(sql)?;
    let rows = statement.query_map(params![key.source, generation, key.edition, key.work], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn prepare(connection: &Connection, key: &Key) -> Result<Option<Prepared>, CatalogStoreError> {
    let identity_filter = if key.canonical_group() { "" } else { " AND p.edition_id=?2" };
    let mut statement = connection.prepare_cached(&format!(
        "SELECT p.entry_id,p.generation,p.payload,p.normalized_title,p.provider_popularity,p.sequence,
                COALESCE(pop.reading_log_count,0),COALESCE(pop.rating_count,0),p.edition_id
         FROM catalog_book p JOIN catalog_source s ON s.source_id=p.source_id AND s.active_generation=p.generation
         LEFT JOIN catalog_book_popularity pop USING(source_id,generation,edition_id,work_id)
         WHERE p.source_id=?1 AND p.work_id=?3 AND p.importable=1 {identity_filter}
         ORDER BY CASE WHEN instr(lower(COALESCE(p.acquisition_url,'')),'.images')>0
                      AND instr(lower(COALESCE(p.acquisition_url,'')),'.noimages')=0 THEN 1 ELSE 0 END DESC,p.edition_id LIMIT 1"
    ))?;
    let result = statement
        .query_row(params![key.source, key.edition, key.work], |r| {
            Ok(Prepared {
                key: Key { source: key.source.clone(), edition: r.get(8)?, work: key.work.clone() },
                entry: r.get(0)?,
                generation: r.get(1)?,
                payload: r.get(2)?,
                title: r.get(3)?,
                popularity: r.get(4)?,
                sequence: r.get(5)?,
                logs: r.get(6)?,
                ratings: r.get(7)?,
                search: String::new(),
                languages: Vec::new(),
                formats: Vec::new(),
                subjects: BTreeSet::new(),
            })
        })
        .optional()?;
    let Some(mut row) = result else {
        return Ok(None);
    };
    let key = &row.key;
    row.languages = strings(
        connection,
        "SELECT DISTINCT lower(substr(language,1,instr(language || '-', '-')-1)) FROM catalog_book_language WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND language!=''",
        key,
        row.generation,
    )?;
    row.formats = strings(connection, "SELECT book_format FROM catalog_book_format WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", key, row.generation)?;
    let authors = strings(connection, "SELECT normalized_name FROM catalog_book_author WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 ORDER BY position", key, row.generation)?;
    let toc = strings(connection, "SELECT normalized_title FROM catalog_book_toc WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 ORDER BY position", key, row.generation)?;
    row.search = format!("{} {} {}", row.title, authors.join(" "), toc.join(" ")).trim().to_owned();
    // Derive ancestors locally: no scan/rebuild of the book-subject
    // membership table, and only the curated unified taxonomy is browsable.
    for path in strings(connection, "SELECT subject_path FROM catalog_book_subject_assignment WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND system_id='unified'", key, row.generation)? {
        row.subjects.insert(String::new());
        let mut ancestor = String::new();
        for title in path.split(" / ") {
            if !ancestor.is_empty() {
                ancestor.push_str(" / ");
            }
            ancestor.push_str(title);
            row.subjects.insert(ancestor.clone());
        }
    }
    Ok(Some(row))
}

fn apply(connection: &Connection, key: &Key, row: Option<&Prepared>) -> Result<(), CatalogStoreError> {
    let ids = {
        let identity_filter = if key.canonical_group() { "" } else { " AND edition_id=?2" };
        let mut statement = connection.prepare_cached(&format!("SELECT entry_id FROM catalog_browse_book WHERE source_id=?1 AND work_id=?3 {identity_filter}"))?;
        let ids = statement.query_map(params![key.source, key.edition, key.work], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?;
        ids
    };
    for id in ids {
        // Entry IDs are payload-hash identities, not mutable book identities.
        // Retain old cards for in-flight index requests without invalidating
        // every client's cache on each projection commit.
        connection.prepare_cached("INSERT OR IGNORE INTO projection_retired VALUES(?1,unixepoch())")?.execute([id])?;
        for table in ["catalog_browse_search_document", "catalog_browse_subject", "catalog_browse_language", "catalog_browse_format", "catalog_browse_book"] {
            connection.prepare_cached(&format!("DELETE FROM {table} WHERE entry_id=?1"))?.execute([id])?;
        }
    }
    let Some(row) = row else {
        return Ok(());
    };
    connection.prepare_cached("INSERT OR IGNORE INTO catalog_book VALUES(?1,?2,?3,?4,?5,?6)")?.execute(params![row.entry, row.key.source, row.generation, row.key.edition, row.key.work, row.payload])?;
    connection.prepare_cached("DELETE FROM projection_retired WHERE entry_id=?1")?.execute([row.entry])?;
    connection.prepare_cached("INSERT INTO catalog_browse_book VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)")?.execute(params![
        row.entry,
        row.key.source,
        row.generation,
        row.key.edition,
        row.key.work,
        row.title,
        row.popularity,
        row.sequence,
        row.logs,
        row.ratings,
        !row.languages.is_empty()
    ])?;
    connection.prepare_cached("INSERT INTO catalog_browse_search_document VALUES(?1,?2)")?.execute(params![row.entry, row.search])?;
    for language in &row.languages {
        connection.prepare_cached("INSERT INTO catalog_browse_language VALUES(?1,?2)")?.execute(params![row.entry, language])?;
    }
    for format in &row.formats {
        connection.prepare_cached("INSERT INTO catalog_browse_format VALUES(?1,?2)")?.execute(params![row.entry, format])?;
    }
    for path in &row.subjects {
        connection.prepare_cached("INSERT INTO catalog_browse_subject VALUES(?1,'unified',?2)")?.execute(params![row.entry, path])?;
        if path.is_empty() {
            continue;
        }
        let (parent, title) = path.rsplit_once(" / ").unwrap_or(("", path));
        connection.prepare_cached("INSERT OR IGNORE INTO catalog_subject_node VALUES('unified',?1,?2,?3)")?.execute(params![path, parent, title])?;
    }
    Ok(())
}

// Expand source visibility changes using keyset pagination over both the old
// view and active authority. This includes removals, without holding its writer
// during the expansion. The cursor is committed with the corresponding rows.
fn source_keys(authority: &Connection, serving: &Connection, source: &str, cursor: Option<(&str, &str)>) -> Result<Vec<Key>, CatalogStoreError> {
    let mut keys = BTreeSet::new();
    let (edition, work) = cursor.unwrap_or(("", ""));
    for (connection, sql) in [
        (authority, "SELECT p.edition_id,p.work_id FROM catalog_book p JOIN catalog_source s ON s.source_id=p.source_id AND s.active_generation=p.generation WHERE p.source_id=?1 {cursor} ORDER BY p.edition_id,p.work_id LIMIT ?4"),
        (serving, "SELECT edition_id,work_id FROM catalog_browse_book WHERE source_id=?1 {cursor} ORDER BY edition_id,work_id LIMIT ?4"),
    ] {
        let sql = sql.replace("{cursor}", if cursor.is_some() { "AND (edition_id,work_id)>(?2,?3)" } else { "" });
        let mut statement = connection.prepare_cached(&sql)?;
        for key in statement.query_map(params![source, edition, work, BATCH as i64], |r| Ok(Key { source: source.to_owned(), edition: r.get(0)?, work: r.get(1)? }))? {
            keys.insert(key?);
        }
    }
    Ok(keys.into_iter().take(BATCH).collect())
}

impl CatalogStore {
    pub fn refresh_browse_serving_index(&self) -> Result<bool, CatalogStoreError> {
        let before: i64 = self.serving.connection()?.query_row("SELECT revision FROM projection_state", [], |r| r.get(0))?;
        while self.refresh_browse_serving_index_batch()? {}
        let after: i64 = self.serving.connection()?.query_row("SELECT revision FROM projection_state", [], |r| r.get(0))?;
        Ok(before != after)
    }

    pub fn refresh_browse_serving_index_batch(&self) -> Result<bool, CatalogStoreError> {
        let _guard = self.serving.exclusive()?;
        let mut writer = self.connection()?;
        let mut reader = self.connection()?;
        let mut serving = self.serving.connection()?;
        let pending: bool = serving.query_row("SELECT EXISTS(SELECT 1 FROM projection_source WHERE done=0)", [], |r| r.get(0))?;
        let dirty: bool = reader.query_row("SELECT EXISTS(SELECT 1 FROM catalog_serving_dirty) OR EXISTS(SELECT 1 FROM catalog_serving_dirty_source)", [], |r| r.get(0))?;
        if !pending && !dirty && ready(&serving)? {
            expire_payloads(&mut serving)?;
            return Ok(false);
        }
        // Make the observed authority WAL durable before publishing data from
        // it into another durable database (other ingestion commits use NORMAL).
        writer.pragma_update(None, "synchronous", "FULL")?;
        // Reserve the writer only long enough to pin a read snapshot and move
        // the invalidation epoch. No metadata preparation occurs under it.
        let fence = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let fence_started = Instant::now();
        let snapshot = reader.transaction_with_behavior(TransactionBehavior::Deferred)?;
        snapshot.query_row("SELECT sequence FROM catalog_serving_change_sequence", [], |r| r.get::<_, i64>(0))?;
        let changes = serving_changes::books(&snapshot, BATCH)?;
        let sources = serving_changes::sources(&snapshot)?;
        let source_count: i64 = snapshot.query_row("SELECT COUNT(*) FROM catalog_serving_dirty_source", [], |r| r.get(0))?;
        let work = !changes.is_empty() || !sources.is_empty() || pending;
        if work {
            fence.execute("UPDATE catalog_serving_change_sequence SET sequence=sequence+1 WHERE singleton=1", [])?;
        }
        fence.commit()?;
        let fence_ms = fence_started.elapsed().as_millis();
        writer.pragma_update(None, "synchronous", "NORMAL")?;
        drop(writer);
        let preparation_started = Instant::now();
        let mut keys = changes.iter().map(|c| Key { source: c.source_id.clone(), edition: c.edition_id.clone(), work: c.work_id.clone() }).collect::<BTreeSet<_>>();
        // Process one source per batch; book edits still make progress.
        let source = if let Some(source) = sources.first() {
            Some((source.source_id.clone(), source.sequence))
        } else {
            serving.query_row("SELECT source_id,sequence FROM projection_source WHERE done=0 ORDER BY source_id LIMIT 1", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).optional()?
        };
        let mut progress = None;
        let mut ack_sources = Vec::new();
        if let Some((source, sequence)) = source {
            let old = serving
                .query_row("SELECT sequence,edition_cursor,work_cursor,done FROM projection_source WHERE source_id=?1", [&source], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, bool>(3)?))
                })
                .optional()?;
            let cursor = old.as_ref().filter(|o| o.0 == sequence).and_then(|o| o.1.as_deref().zip(o.2.as_deref()));
            let done = old.as_ref().is_some_and(|o| o.0 == sequence && o.3);
            let expanded = if done { Vec::new() } else { source_keys(&snapshot, &serving, &source, cursor)? };
            let finished = expanded.len() < BATCH;
            let next = expanded.last().map(|k| (k.edition.clone(), k.work.clone())).or_else(|| cursor.map(|(e, w)| (e.to_owned(), w.to_owned())));
            keys.extend(expanded);
            if finished && sequence >= 0 {
                ack_sources.push(serving_changes::SourceChange { source_id: source.clone(), sequence });
            }
            progress = Some((source, sequence, next, finished));
        }
        // Canonical variants are one invalidation unit, including promotion
        // of a previously hidden sibling when the preferred edition disappears.
        let mut groups = BTreeSet::new();
        keys.retain(|k| !k.canonical_group() || groups.insert((k.source.clone(), k.work.clone())));
        let prepared = keys.iter().map(|key| prepare(&snapshot, key).map(|row| (key, row))).collect::<Result<Vec<_>, _>>()?;
        snapshot.commit()?;
        let preparation_ms = preparation_started.elapsed().as_millis();
        let commit_started = Instant::now();
        let transaction = serving.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (key, row) in &prepared {
            apply(&transaction, key, row.as_ref())?;
        }
        if let Some((source, sequence, next, done)) = progress {
            transaction.execute(
                "INSERT INTO projection_source VALUES(?1,?2,?3,?4,?5) ON CONFLICT(source_id) DO UPDATE SET sequence=excluded.sequence,edition_cursor=excluded.edition_cursor,work_cursor=excluded.work_cursor,done=excluded.done",
                params![source, sequence, next.as_ref().map(|n| &n.0), next.as_ref().map(|n| &n.1), done],
            )?;
        }
        if work || !ready(&transaction)? {
            let sources_finished = source_count == ack_sources.len() as i64;
            transaction.execute("UPDATE projection_state SET revision=revision+1,ready=MAX(ready,?1 AND NOT EXISTS(SELECT 1 FROM projection_source WHERE done=0))", [sources_finished])?;
            transaction.execute("UPDATE catalog_browse_serving_metadata SET ready=(SELECT ready FROM projection_state),revision=(SELECT revision FROM projection_state)", [])?;
        }
        transaction.commit()?;
        let commit_ms = commit_started.elapsed().as_millis();
        // Commit-before-ack: a crash replays idempotently. A newer epoch cannot
        // be erased by this acknowledgement. Batch all deletes in one commit.
        if !changes.is_empty() || !ack_sources.is_empty() {
            let mut writer = self.connection()?;
            let transaction = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
            serving_changes::acknowledge(&transaction, &changes, &ack_sources)?;
            transaction.commit()?;
        }
        tracing::debug!(books = prepared.len(), fence_ms, preparation_ms, commit_ms, "catalogue serving projection batch");
        expire_payloads(&mut serving)?;
        Ok(work)
    }
}

fn expire_payloads(serving: &mut Connection) -> Result<(), CatalogStoreError> {
    let ids = {
        let mut statement = serving.prepare_cached("SELECT entry_id FROM projection_retired WHERE retired_at<unixepoch()-3600 ORDER BY retired_at,entry_id LIMIT 256")?;
        let ids = statement.query_map([], |r| r.get::<_, i64>(0))?.collect::<Result<Vec<_>, _>>()?;
        ids
    };
    if ids.is_empty() {
        return Ok(());
    }
    let transaction = serving.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for id in ids {
        transaction.execute("DELETE FROM catalog_book WHERE entry_id=?1", [id])?;
        transaction.execute("DELETE FROM projection_retired WHERE entry_id=?1", [id])?;
    }
    transaction.commit()?;
    Ok(())
}
