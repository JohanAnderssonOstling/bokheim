//! Immutable publication with bounded updates over the same imported snapshot.
use super::{batch::WorkEvidenceCache, *};
use rusqlite::{params_from_iter, Row};
use std::cmp::Ordering as KeyOrder;

// Bump when the stored resolution, candidate or queue rules change. Server-only
// changes can continue to use the existing immutable answers.
pub(super) const RECIPE_VERSION: i64 = 5;
const MAX_CHANGED_ISBNS: usize = 100_000;

pub(crate) const STATE_SCHEMA: &str = "
CREATE TABLE answer_build(singleton INTEGER PRIMARY KEY CHECK(singleton=1),recipe_version INTEGER NOT NULL,inputs TEXT NOT NULL,isbn_rows INTEGER NOT NULL,json_bytes INTEGER NOT NULL);
CREATE TABLE answer_classification(isbn13 INTEGER PRIMARY KEY,classifications BLOB NOT NULL) WITHOUT ROWID;
CREATE TABLE answer_work_lcc(work_id INTEGER PRIMARY KEY);
CREATE TABLE answer_provider(provider TEXT NOT NULL,isbn13 INTEGER NOT NULL,status TEXT NOT NULL,classifications BLOB NOT NULL,subjects BLOB NOT NULL,PRIMARY KEY(provider,isbn13)) WITHOUT ROWID;
CREATE TABLE answer_demand(isbn13 INTEGER PRIMARY KEY,requests INTEGER NOT NULL) WITHOUT ROWID;
";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct InputIdentity {
    source: FileIdentity,
    descriptions: Option<FileIdentity>,
    ratings: Option<FileIdentity>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct FileIdentity {
    path: PathBuf,
    bytes: u64,
    modified_ns: u128,
    #[cfg(unix)]
    changed: (i64, i64),
}

impl FileIdentity {
    fn read(path: &Path) -> Result<Self, MetadataError> {
        let path = fs::canonicalize(path).map_err(error)?;
        let metadata = fs::metadata(&path).map_err(error)?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            path,
            bytes: metadata.len(),
            modified_ns: metadata.modified().map_err(error)?.duration_since(UNIX_EPOCH).map_err(error)?.as_nanos(),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
}

impl InputIdentity {
    pub(super) fn verify_source(&self, source: &Path) -> Result<(), MetadataError> {
        if self.source != FileIdentity::read(source)? {
            return Err(MetadataError("edition indexing requires the original imported source".into()));
        }
        Ok(())
    }
    pub(super) fn read(source: &Path, descriptions: Option<&Path>, ratings: Option<&Path>) -> Result<Self, MetadataError> {
        Ok(Self { source: FileIdentity::read(source)?, descriptions: descriptions.map(FileIdentity::read).transpose()?, ratings: ratings.map(FileIdentity::read).transpose()? })
    }

    pub(super) fn verify(&self, source: &Path, descriptions: Option<&Path>, ratings: Option<&Path>) -> Result<(), MetadataError> {
        if *self != Self::read(source, descriptions, ratings)? {
            return Err(MetadataError("immutable build inputs changed while building; output was not published".into()));
        }
        Ok(())
    }
}

pub(super) fn save_state(out: &Connection, provider: Option<&Connection>, demand: Option<&Connection>, inputs: &InputIdentity, count: u64, bytes: u64) -> Result<(), MetadataError> {
    if let Some(c) = provider {
        let mut query = c.prepare("SELECT provider,isbn13,status,classifications,subjects FROM provider_result ORDER BY provider,isbn13").map_err(error)?;
        let mut put = out.prepare_cached("INSERT INTO answer_provider VALUES(?1,?2,?3,?4,?5)").map_err(error)?;
        for row in query.query_map([], provider_row).map_err(error)? {
            let (provider, isbn, status, codes, subjects) = row.map_err(error)?;
            put.execute(params![provider, isbn, status, codes, subjects]).map_err(error)?;
        }
    }
    if let Some(c) = demand {
        let mut query = c.prepare("SELECT isbn13,requests FROM demand ORDER BY isbn13").map_err(error)?;
        let mut put = out.prepare_cached("INSERT INTO answer_demand VALUES(?1,?2)").map_err(error)?;
        for row in query.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(error)? {
            let (isbn, requests) = row.map_err(error)?;
            put.execute(params![isbn, requests]).map_err(error)?;
        }
    }
    save_counts(out, inputs, count, bytes)
}

pub(super) fn save_counts(out: &Connection, inputs: &InputIdentity, count: u64, bytes: u64) -> Result<(), MetadataError> {
    out.execute("INSERT OR REPLACE INTO answer_build VALUES(1,?1,?2,?3,?4)", params![RECIPE_VERSION, serde_json::to_string(inputs).map_err(error)?, i64::try_from(count).map_err(error)?, i64::try_from(bytes).map_err(error)?])
        .map_err(error)?;
    Ok(())
}

type ProviderRow = (String, i64, String, Vec<u8>, Vec<u8>);
fn provider_row(row: &Row<'_>) -> rusqlite::Result<ProviderRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
}

// Compare exact evidence, including deletions and edits with unchanged timestamps.
// Streaming in primary-key order bounds memory independently of cache size.
pub(super) fn provider_changes(old: &Connection, current: Option<&Connection>, changes: &mut BTreeSet<i64>) -> Result<(), MetadataError> {
    let mut before = old.prepare("SELECT provider,isbn13,status,classifications,subjects FROM answer_provider ORDER BY provider,isbn13").map_err(error)?;
    let mut after = current.map(|c| c.prepare("SELECT provider,isbn13,status,classifications,subjects FROM provider_result ORDER BY provider,isbn13")).transpose().map_err(error)?;
    let old_rows = before.query_map([], provider_row).map_err(error)?;
    let new_rows = after.as_mut().map(|q| q.query_map([], provider_row)).transpose().map_err(error)?;
    collect_changes(old_rows, new_rows.into_iter().flatten(), |a, b| (&a.0, a.1).cmp(&(&b.0, b.1)), |row| row.1, changes)
}

pub(super) fn demand_changes(old: &Connection, current: Option<&Connection>, changes: &mut BTreeSet<i64>) -> Result<(), MetadataError> {
    fn row(r: &Row<'_>) -> rusqlite::Result<(i64, i64)> {
        Ok((r.get(0)?, r.get(1)?))
    }
    let mut before = old.prepare("SELECT isbn13,requests FROM answer_demand ORDER BY isbn13").map_err(error)?;
    let mut after = current.map(|c| c.prepare("SELECT isbn13,requests FROM demand ORDER BY isbn13")).transpose().map_err(error)?;
    let old_rows = before.query_map([], row).map_err(error)?;
    let new_rows = after.as_mut().map(|q| q.query_map([], row)).transpose().map_err(error)?;
    collect_changes(old_rows, new_rows.into_iter().flatten(), |a, b| a.0.cmp(&b.0), |row| row.0, changes)
}

fn collect_changes<T: PartialEq>(
    mut before: impl Iterator<Item = rusqlite::Result<T>>, mut after: impl Iterator<Item = rusqlite::Result<T>>, compare: impl Fn(&T, &T) -> KeyOrder, isbn: impl Fn(&T) -> i64, changes: &mut BTreeSet<i64>,
) -> Result<(), MetadataError> {
    let mut left = before.next().transpose().map_err(error)?;
    let mut right = after.next().transpose().map_err(error)?;
    while left.is_some() || right.is_some() {
        let order = match (&left, &right) {
            (Some(a), Some(b)) => compare(a, b),
            (Some(_), None) => KeyOrder::Less,
            _ => KeyOrder::Greater,
        };
        if order.is_le() {
            let row = left.take().unwrap();
            if order.is_lt() || right.as_ref() != Some(&row) {
                changes.insert(isbn(&row));
            }
            left = before.next().transpose().map_err(error)?;
        }
        if order.is_ge() {
            let row = right.take().unwrap();
            if order.is_gt() {
                changes.insert(isbn(&row));
            }
            right = after.next().transpose().map_err(error)?;
        }
        if changes.len() > MAX_CHANGED_ISBNS {
            break;
        }
    }
    Ok(())
}

/// PREVIOUS remains immutable. Unsupported/changed inputs use a fresh full build.
pub fn build_answers_incremental(snapshot: &Path, providers: Option<&Path>, demand: Option<&Path>, descriptions: Option<&Path>, ratings: Option<&Path>, previous: &Path, output: &Path) -> Result<BuildReport, MetadataError> {
    if output.exists() {
        return Err(MetadataError("answer output already exists; build to a new dated file".into()));
    }
    let started = Instant::now();
    let inputs = InputIdentity::read(snapshot, descriptions, ratings)?;
    let old = crate::snapshot::open(previous)?;
    let state = if has_table(&old, "answer_build")? {
        old.query_row("SELECT recipe_version,inputs,isbn_rows,json_bytes FROM answer_build WHERE singleton=1", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))).optional().map_err(error)?
    } else {
        None
    };
    let compatible = state.as_ref().is_some_and(|(version, identity, _, _)| *version == RECIPE_VERSION && serde_json::from_str::<InputIdentity>(identity).ok().as_ref() == Some(&inputs));
    if !compatible {
        if state.as_ref().is_some_and(|(version, identity, _, _)| *version == 2 && serde_json::from_str::<InputIdentity>(identity).ok().as_ref() == Some(&inputs)) {
            return super::reclassify::publish_reclassification(snapshot, providers, demand, descriptions, ratings, previous, output);
        }
        tracing::info!("incremental base is incompatible; building a fresh snapshot");
        return build_answers_with_ratings(snapshot, providers, demand, descriptions, ratings, output);
    }
    let (_, _, count, bytes) = state.unwrap();
    let mut count = u64::try_from(count).map_err(error)?;
    let mut bytes = u64::try_from(bytes).map_err(error)?;
    let version: i64 = old.query_row("SELECT schema_version FROM snapshot WHERE singleton=1", [], |r| r.get(0)).map_err(error)?;
    if version != schema::ANSWER_VERSION {
        return Err(MetadataError("unsupported incremental answer schema".into()));
    }
    let provider = providers.map(read_connection).transpose()?;
    let demand_input = demand.map(read_connection).transpose()?;
    for c in provider.iter().chain(demand_input.iter()) {
        c.execute_batch("BEGIN").map_err(error)?;
    }
    let through = provider.as_ref().map(|c| c.query_row("SELECT COALESCE(MAX(fetched_at_ms),0) FROM provider_result", [], |r| r.get::<_, i64>(0)).map_err(error)).transpose()?.unwrap_or(0);
    let mut metadata_changes = BTreeSet::new();
    let mut demand_updates = BTreeSet::new();
    provider_changes(&old, provider.as_ref(), &mut metadata_changes)?;
    demand_changes(&old, demand_input.as_ref(), &mut demand_updates)?;
    let changes = metadata_changes.union(&demand_updates).copied().collect::<BTreeSet<_>>();
    if changes.len() > MAX_CHANGED_ISBNS {
        tracing::info!("incremental changes exceed the bounded update limit; building a fresh snapshot");
        return build_answers_with_ratings(snapshot, providers, demand, descriptions, ratings, output);
    }
    let old_through: i64 = old.query_row("SELECT provider_results_through_ms FROM snapshot WHERE singleton=1", [], |r| r.get(0)).map_err(error)?;
    if changes.is_empty() && through == old_through {
        inputs.verify(snapshot, descriptions, ratings)?;
        fs::hard_link(previous, output).map_err(error)?;
        return Ok(report("reused", started, count, bytes, 0, count, fs::metadata(output).map_err(error)?.len()));
    }
    let resolver = Resolver::open(snapshot, descriptions)?;
    let source = resolver.connection()?;
    validate_source(&source)?;
    // Request counts affect priority only. Resolve a demand-only change when
    // it adds an answer or removes the last input that owned that ISBN.
    for &isbn in &demand_updates {
        let existed = old.query_row("SELECT EXISTS(SELECT 1 FROM isbn_answer WHERE isbn13=?1)", [isbn], |r| r.get::<_, bool>(0)).map_err(error)?;
        if !existed || !included(&source, provider.as_ref(), demand_input.as_ref(), isbn)? {
            metadata_changes.insert(isbn);
        }
    }
    // A changed classification can reopen every ISBN of the associated work.
    let (refresh, editions, works) = affected(&source, &metadata_changes)?;
    if refresh.len() > MAX_CHANGED_ISBNS {
        tracing::info!("affected works exceed the bounded update limit; building a fresh snapshot");
        return build_answers_with_ratings(snapshot, providers, demand, descriptions, ratings, output);
    }
    tracing::info!(changed_isbns = changes.len(), refreshed_isbns = refresh.len(), editions = editions.len(), works = works.len(), "building incremental answers");
    let temporary = output.with_extension(format!("building-{}", std::process::id()));
    let _reservation = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(error)?;
    let result = (|| {
        // The base stays immutable; only a cumulative delta is written/copied.
        let out = crate::snapshot::create_update(previous, &temporary)?;
        out.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=-65536; BEGIN;").map_err(error)?;
        super::popularity::load(&out, ratings)?;
        let mut stats = report("incremental", started, count, bytes, 0, count, 0);
        let mut cache = WorkEvidenceCache::default();
        let mut removed = 0;
        // Refresh all ISBNs of these works, so their aggregate can be rebuilt
        // once without maintaining a random-write counter for every full-build ISBN.
        for chunk in works.iter().copied().collect::<Vec<_>>().chunks(500) {
            out.execute(&format!("DELETE FROM answer_work_lcc WHERE work_id IN ({})", sql_placeholders(chunk.len())), params_from_iter(chunk)).map_err(error)?;
        }
        for chunk in refresh.iter().copied().collect::<Vec<_>>().chunks(BUILD_BATCH_ISBNS) {
            let mut rows = Vec::new();
            let write_started = Instant::now();
            for &isbn in chunk {
                let (existed, old_bytes) = remove_answer(&out, isbn)?;
                if existed {
                    count -= 1;
                    bytes -= old_bytes;
                    removed += 1;
                }
                if included(&source, provider.as_ref(), demand_input.as_ref(), isbn)? {
                    let requests = demand_input.as_ref().map(|c| c.query_row("SELECT requests FROM demand WHERE isbn13=?1", [isbn], |r| r.get::<_, i64>(0)).optional().map_err(error)).transpose()?.flatten().unwrap_or(0);
                    rows.push((isbn, requests));
                }
            }
            stats.write_seconds += write_started.elapsed().as_secs_f64();
            if rows.is_empty() {
                continue;
            }
            let lookup = Instant::now();
            let mut response = resolver.enrich_build_batch_cached(rows.iter().map(|(isbn, _)| format!("{isbn:013}")).collect(), &mut cache)?;
            stats.lookup_seconds += lookup.elapsed().as_secs_f64();
            let lookup = Instant::now();
            if let Some(c) = &provider {
                super::providers::apply_many(c, &mut response)?;
            }
            super::popularity::attach(&out, &mut response, &resolver.state.snapshot.dump_date)?;
            stats.provider_seconds += lookup.elapsed().as_secs_f64();
            let write = Instant::now();
            let (written, new_bytes) = write_answer_batch(&out, rows, response)?;
            stats.resolved_rows += written;
            count += written;
            bytes += new_bytes;
            stats.write_seconds += write.elapsed().as_secs_f64();
        }
        let index = Instant::now();
        for chunk in editions.iter().copied().collect::<Vec<_>>().chunks(500) {
            let placeholders = sql_placeholders(chunk.len());
            out.execute(&format!("DELETE FROM edition_title_key WHERE edition_id IN ({placeholders})"), params_from_iter(chunk)).map_err(error)?;
            out.execute(&format!("DELETE FROM edition_candidate WHERE edition_id IN ({placeholders})"), params_from_iter(chunk)).map_err(error)?;
        }
        super::identity::build_candidates_for(&source, &out, Some(&editions))?;
        stats.edition_index_seconds = index.elapsed().as_secs_f64();
        let queue = Instant::now();
        // Only refreshed ISBNs have been reinserted into wanted. Closing their
        // work again also leaves previously pruned unaffected rows untouched.
        for chunk in works.iter().copied().collect::<Vec<_>>().chunks(500) {
            let sql = format!("DELETE FROM wanted WHERE work_id IN ({}) AND (work_id IN (SELECT work_id FROM answer_work_lcc) OR EXISTS(SELECT 1 FROM edition_candidate c WHERE c.work_id=wanted.work_id))", sql_placeholders(chunk.len()));
            out.execute(&sql, params_from_iter(chunk)).map_err(error)?;
        }
        for &isbn in &demand_updates {
            let requests = demand_input.as_ref().map(|c| c.query_row("SELECT requests FROM demand WHERE isbn13=?1", [isbn], |r| r.get::<_, i64>(0)).optional().map_err(error)).transpose()?.flatten().unwrap_or(0);
            let rank = if requests > 0 { (-1_000_000_000_000_i64).saturating_sub(requests) } else { 0 };
            out.execute("UPDATE wanted SET rank=?1 WHERE isbn13=?2", params![rank, isbn]).map_err(error)?;
        }
        let rerank = refresh.union(&demand_updates).copied().collect();
        rank_changed(&out, &rerank)?;
        sync_inputs(&out, provider.as_ref(), demand_input.as_ref(), &changes)?;
        save_counts(&out, &inputs, count, bytes)?;
        out.execute("UPDATE snapshot SET built_at_ms=?1,provider_results_through_ms=?2 WHERE singleton=1", params![now_ms(), through]).map_err(error)?;
        inputs.verify(snapshot, descriptions, ratings)?;
        stats.queue_seconds = queue.elapsed().as_secs_f64();
        out.execute_batch("COMMIT; PRAGMA main.optimize;").map_err(error)?;
        let integrity: String = out.query_row("PRAGMA main.quick_check", [], |r| r.get(0)).map_err(error)?;
        if integrity != "ok" {
            return Err(MetadataError(format!("answer integrity check failed: {integrity}")));
        }
        drop(out);
        fs::hard_link(&temporary, output).map_err(error)?;
        stats.isbn_rows = count;
        stats.answer_json_bytes = bytes;
        stats.reused_rows -= removed;
        stats.file_bytes = fs::metadata(output).map_err(error)?.len();
        stats.elapsed_seconds = started.elapsed().as_secs_f64();
        stats.isbns_per_second = stats.resolved_rows as f64 / stats.elapsed_seconds;
        Ok(stats)
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn report(mode: &'static str, started: Instant, count: u64, bytes: u64, resolved: u64, reused: u64, file_bytes: u64) -> BuildReport {
    BuildReport {
        mode,
        resolved_rows: resolved,
        reused_rows: reused,
        reclassified_rows: 0,
        reclassified_editions: 0,
        isbn_rows: count,
        answer_json_bytes: bytes,
        file_bytes,
        elapsed_seconds: started.elapsed().as_secs_f64(),
        isbns_per_second: 0.0,
        preparation_seconds: started.elapsed().as_secs_f64(),
        lookup_seconds: 0.0,
        provider_seconds: 0.0,
        write_seconds: 0.0,
        edition_index_seconds: 0.0,
        authority_index_seconds: 0.0,
        queue_seconds: 0.0,
    }
}

pub(super) fn included(source: &Connection, provider: Option<&Connection>, demand: Option<&Connection>, isbn: i64) -> Result<bool, MetadataError> {
    for table in ["edition_isbn", "isbn_classification", "isbn_lcc", "lc_record_isbn"] {
        if has_table(source, table)? && source.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE isbn13=?1)"), [isbn], |r| r.get::<_, bool>(0)).map_err(error)? {
            return Ok(true);
        }
    }
    for (c, table) in [(provider, "provider_result"), (demand, "demand")] {
        if let Some(c) = c {
            if c.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE isbn13=?1)"), [isbn], |r| r.get::<_, bool>(0)).map_err(error)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn affected(source: &Connection, changed: &BTreeSet<i64>) -> Result<(BTreeSet<i64>, BTreeSet<i64>, BTreeSet<i64>), MetadataError> {
    let mut refresh = changed.clone();
    let mut editions = BTreeSet::new();
    let mut works = BTreeSet::new();
    for chunk in changed.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT e.edition_id,e.work_id FROM edition_isbn i JOIN edition e USING(edition_id) WHERE i.isbn13 IN ({})", sql_placeholders(chunk.len()));
        let mut q = source.prepare_cached(&sql).map_err(error)?;
        for row in q.query_map(params_from_iter(chunk), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))).map_err(error)? {
            let (id, work) = row.map_err(error)?;
            editions.insert(id);
            works.extend(work);
        }
    }
    for chunk in works.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT e.edition_id,i.isbn13 FROM edition e JOIN edition_isbn i USING(edition_id) WHERE e.work_id IN ({})", sql_placeholders(chunk.len()));
        let mut q = source.prepare_cached(&sql).map_err(error)?;
        for row in q.query_map(params_from_iter(chunk), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(error)? {
            let (id, isbn) = row.map_err(error)?;
            editions.insert(id);
            refresh.insert(isbn);
            if refresh.len() > MAX_CHANGED_ISBNS {
                return Ok((refresh, editions, works));
            }
        }
    }
    // A refreshed ISBN can also be the fallback of an edition in another work.
    for chunk in refresh.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("SELECT edition_id FROM edition_isbn WHERE isbn13 IN ({})", sql_placeholders(chunk.len()));
        let mut q = source.prepare_cached(&sql).map_err(error)?;
        for row in q.query_map(params_from_iter(chunk), |r| r.get::<_, i64>(0)).map_err(error)? {
            editions.insert(row.map_err(error)?);
        }
    }
    Ok((refresh, editions, works))
}

pub(super) fn sync_inputs(out: &Connection, provider: Option<&Connection>, demand: Option<&Connection>, changed: &BTreeSet<i64>) -> Result<(), MetadataError> {
    for &isbn in changed {
        for name in ["loc", "librarything"] {
            out.execute("DELETE FROM answer_provider WHERE provider=?1 AND isbn13=?2", params![name, isbn]).map_err(error)?;
            if let Some(c) = provider {
                let row = c.query_row("SELECT provider,isbn13,status,classifications,subjects FROM provider_result WHERE provider=?1 AND isbn13=?2", params![name, isbn], provider_row).optional().map_err(error)?;
                if let Some((provider, isbn, status, codes, subjects)) = row {
                    out.execute("INSERT INTO answer_provider VALUES(?1,?2,?3,?4,?5)", params![provider, isbn, status, codes, subjects]).map_err(error)?;
                }
            }
        }
        out.execute("DELETE FROM answer_demand WHERE isbn13=?1", [isbn]).map_err(error)?;
        if let Some(c) = demand {
            if let Some(requests) = c.query_row("SELECT requests FROM demand WHERE isbn13=?1", [isbn], |r| r.get::<_, i64>(0)).optional().map_err(error)? {
                out.execute("INSERT INTO answer_demand VALUES(?1,?2)", params![isbn, requests]).map_err(error)?;
            }
        }
    }
    Ok(())
}

fn rank_changed(out: &Connection, refreshed: &BTreeSet<i64>) -> Result<(), MetadataError> {
    for chunk in refreshed.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = format!("UPDATE wanted SET rank=-COALESCE((SELECT count FROM build_rating WHERE work_id=wanted.work_id),0) WHERE rank=0 AND isbn13 IN ({})", sql_placeholders(chunk.len()));
        out.execute(&sql, params_from_iter(chunk)).map_err(error)?;
    }
    out.execute_batch("DROP TABLE build_rating").map_err(error)
}
