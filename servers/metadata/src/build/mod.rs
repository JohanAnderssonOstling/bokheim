//! Build immutable answers using the existing resolution rules, without network I/O.

#[cfg(test)]
use crate::build::tests::snapshot_from;
use crate::schema::{self, now_ms};
use crate::*;

pub(crate) mod authority_ingestion;
pub(crate) mod batch;
pub(crate) mod enrichment;
pub(crate) mod identity;
pub(crate) mod import;
pub(crate) mod incremental;
mod reclassify;
mod edition_index;
pub(crate) mod loc_dump;
pub(crate) mod openlibrary;
mod popularity;
mod providers;
mod work_classification;
mod work_lookup;
pub use work_lookup::index_work_classifications;
pub use incremental::build_answers_incremental;
pub use reclassify::reclassify_answers;
pub use edition_index::{extend_edition_index, EditionIndexReport};
pub(crate) mod service;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod wikidata_books;

const BUILD_BATCH_ISBNS: usize = 4096;

#[derive(Debug, Serialize)]
pub struct BuildReport {
    pub mode: &'static str,
    pub resolved_rows: u64,
    pub reused_rows: u64,
    pub reclassified_rows: u64,
    pub reclassified_editions: u64,
    pub isbn_rows: u64,
    pub answer_json_bytes: u64,
    pub file_bytes: u64,
    pub elapsed_seconds: f64,
    pub isbns_per_second: f64,
    pub preparation_seconds: f64,
    pub lookup_seconds: f64,
    pub provider_seconds: f64,
    pub write_seconds: f64,
    pub edition_index_seconds: f64,
    pub authority_index_seconds: f64,
    pub queue_seconds: f64,
}

/// OUT must be a new file. Publish it only after all tables and the manifest are complete.
pub fn build_answers(snapshot: &Path, providers: Option<&Path>, demand: Option<&Path>, descriptions: Option<&Path>, output: &Path) -> Result<BuildReport, MetadataError> {
    build_answers_with_ratings(snapshot, providers, demand, descriptions, None, output)
}

pub fn build_answers_with_ratings(snapshot: &Path, providers: Option<&Path>, demand: Option<&Path>, descriptions: Option<&Path>, ratings: Option<&Path>, output: &Path) -> Result<BuildReport, MetadataError> {
    if output.exists() {
        return Err(MetadataError("answer output already exists; build to a new dated file".into()));
    }
    let started = Instant::now();
    let inputs = incremental::InputIdentity::read(snapshot, descriptions, ratings)?;
    let resolver = Resolver::open(snapshot, descriptions)?;
    let source = resolver.connection()?;
    validate_source(&source)?;
    let provider = providers.map(read_connection).transpose()?;
    let demand_input = demand.map(read_connection).transpose()?;
    if let Some(c) = &demand_input {
        c.execute_batch("BEGIN").map_err(error)?;
    }
    if let Some(c) = &provider {
        c.execute_batch("BEGIN").map_err(error)?;
    }
    let through = provider.as_ref().map(|c| c.query_row("SELECT COALESCE(MAX(fetched_at_ms),0) FROM provider_result", [], |r| r.get::<_, i64>(0)).map_err(error)).transpose()?.unwrap_or(0);
    let temporary = output.with_extension(format!("building-{}", std::process::id()));
    let _reservation = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(error)?;
    let result = (|| {
        let out = Connection::open(&temporary).map_err(error)?;
        out.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=-65536;").map_err(error)?;
        out.execute_batch(schema::ANSWERS).map_err(error)?;
        out.execute_batch(incremental::STATE_SCHEMA).map_err(error)?;
        // Build secondary queue indexes after loading and ranking the rows.
        out.execute_batch("DROP INDEX wanted_by_rank; DROP INDEX wanted_by_work;").map_err(error)?;
        // A disk-backed union covers OL, supplements, crawled-only and requested-only ISBNs.
        out.execute_batch("CREATE TABLE build_isbn(isbn13 INTEGER PRIMARY KEY,requests INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID; BEGIN;").map_err(error)?;
        {
            let mut insert_isbn = out.prepare("INSERT OR IGNORE INTO build_isbn(isbn13) VALUES(?1)").map_err(error)?;
            for table in ["edition_isbn", "isbn_classification", "isbn_lcc", "lc_record_isbn"] {
                if has_table(&source, table)? {
                    let mut q = source.prepare(&format!("SELECT DISTINCT isbn13 FROM {table} ORDER BY isbn13")).map_err(error)?;
                    for row in q.query_map([], |r| r.get::<_, i64>(0)).map_err(error)? {
                        insert_isbn.execute([row.map_err(error)?]).map_err(error)?;
                    }
                }
            }
            if let Some(c) = &provider {
                let mut q = c.prepare("SELECT DISTINCT isbn13 FROM provider_result ORDER BY isbn13").map_err(error)?;
                for row in q.query_map([], |r| r.get::<_, i64>(0)).map_err(error)? {
                    insert_isbn.execute([row.map_err(error)?]).map_err(error)?;
                }
            }
            if let Some(c) = &demand_input {
                let mut q = c.prepare("SELECT isbn13,requests FROM demand ORDER BY isbn13").map_err(error)?;
                for row in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(error)? {
                    let (isbn, requests) = row.map_err(error)?;
                    out.execute("INSERT INTO build_isbn VALUES(?1,?2) ON CONFLICT(isbn13) DO UPDATE SET requests=excluded.requests", params![isbn, requests]).map_err(error)?;
                }
            }
        }
        out.execute_batch("COMMIT").map_err(error)?;
        out.execute_batch("BEGIN").map_err(error)?;
        popularity::load(&out, ratings)?;
        out.execute_batch("COMMIT").map_err(error)?;
        let preparation_seconds = started.elapsed().as_secs_f64();
        let mut lookup_seconds = 0.0;
        let mut provider_seconds = 0.0;
        let mut write_seconds = 0.0;
        let mut after = i64::MIN;
        let mut count = 0;
        let mut bytes = 0;
        let mut work_cache = batch::WorkEvidenceCache::default();
        loop {
            let mut q = out.prepare_cached("SELECT isbn13,requests FROM build_isbn WHERE isbn13>?1 ORDER BY isbn13 LIMIT ?2").map_err(error)?;
            let rows = q.query_map(params![after, BUILD_BATCH_ISBNS as i64], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            drop(q);
            if rows.is_empty() {
                break;
            }
            after = rows.last().unwrap().0;
            let lookup_started = Instant::now();
            let mut response = resolver.enrich_build_batch_cached(rows.iter().map(|(id, _)| format!("{id:013}")).collect(), &mut work_cache)?;
            lookup_seconds += lookup_started.elapsed().as_secs_f64();
            let provider_started = Instant::now();
            if let Some(c) = &provider {
                providers::apply_many(c, &mut response)?;
            }
            popularity::attach(&out, &mut response, &resolver.state.snapshot.dump_date)?;
            provider_seconds += provider_started.elapsed().as_secs_f64();
            let write_started = Instant::now();
            out.execute_batch("BEGIN").map_err(error)?;
            let (written, json_bytes) = write_answer_batch(&out, rows, response)?;
            count += written;
            bytes += json_bytes;
            out.execute_batch("COMMIT").map_err(error)?;
            write_seconds += write_started.elapsed().as_secs_f64();
            if count % 100_000 < BUILD_BATCH_ISBNS as u64 {
                tracing::info!(isbn_rows = count, elapsed_seconds = started.elapsed().as_secs_f64(), lookup_seconds, provider_seconds, write_seconds, "building answers");
            }
        }
        // All ISBNs of a work with a resolved LCC leave the crawler queue together.
        out.execute_batch("BEGIN").map_err(error)?;
        tracing::info!("building edition search index");
        let index_started = Instant::now();
        crate::build::identity::build_candidates(&source, &out)?;
        let edition_index_seconds = index_started.elapsed().as_secs_f64();
        tracing::info!(edition_index_seconds, "building authority search index");
        let index_started = Instant::now();
        build_authorities(&source, &out)?;
        let authority_index_seconds = index_started.elapsed().as_secs_f64();
        tracing::info!(authority_index_seconds, "finalizing crawler queue");
        let queue_started = Instant::now();
        // Candidate evidence already contains exact/work/sibling consensus.
        out.execute_batch(
            "CREATE INDEX wanted_by_work ON wanted(work_id,isbn13);
DELETE FROM wanted WHERE work_id IN (SELECT work_id FROM answer_work_lcc UNION SELECT work_id FROM edition_candidate WHERE work_id IS NOT NULL);
",
        )
        .map_err(error)?;
        out.execute_batch("UPDATE wanted SET rank=-COALESCE((SELECT count FROM build_rating WHERE work_id=wanted.work_id),0) WHERE rank=0; DROP TABLE build_rating;").map_err(error)?;
        out.execute_batch(
            "CREATE INDEX wanted_by_rank ON wanted(rank,isbn13); CREATE INDEX edition_candidate_by_work ON edition_candidate(work_id); CREATE INDEX edition_title_key_by_edition ON edition_title_key(edition_id); DROP TABLE build_isbn;",
        )
        .map_err(error)?;
        incremental::save_state(&out, provider.as_ref(), demand_input.as_ref(), &inputs, count, bytes)?;
        inputs.verify(snapshot, descriptions, ratings)?;
        let queue_seconds = queue_started.elapsed().as_secs_f64();
        out.execute("INSERT INTO snapshot VALUES(1,?1,?2,?3,?4)", params![schema::ANSWER_VERSION, resolver.state.snapshot.dump_date, now_ms(), through]).map_err(error)?;
        out.execute_batch("COMMIT; PRAGMA main.optimize;").map_err(error)?;
        let integrity: String = out.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(error)?;
        if integrity != "ok" {
            return Err(MetadataError(format!("answer integrity check failed: {integrity}")));
        }
        drop(out);
        // Same-filesystem publication fails if OUT appeared while building.
        std::fs::hard_link(&temporary, output).map_err(error)?;
        let elapsed = started.elapsed().as_secs_f64();
        Ok(BuildReport {
            mode: "full",
            resolved_rows: count,
            reused_rows: 0,
            reclassified_rows: 0,
            reclassified_editions: 0,
            isbn_rows: count,
            answer_json_bytes: bytes,
            file_bytes: std::fs::metadata(output).map_err(error)?.len(),
            elapsed_seconds: elapsed,
            isbns_per_second: count as f64 / elapsed,
            preparation_seconds,
            lookup_seconds,
            provider_seconds,
            write_seconds,
            edition_index_seconds,
            authority_index_seconds,
            queue_seconds,
        })
    })();
    let _ = std::fs::remove_file(temporary);
    result
}

pub(crate) fn read_connection(path: &Path) -> Result<Connection, MetadataError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(error)
}
fn has_table(c: &Connection, table: &str) -> Result<bool, MetadataError> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |r| r.get(0)).map_err(error)
}
fn validate_source(c: &Connection) -> Result<(), MetadataError> {
    let version: i64 = c.query_row("SELECT value FROM metadata_metric WHERE name='builder_schema_version'", [], |r| r.get(0)).map_err(error)?;
    if ![9, 10].contains(&version) {
        return Err(MetadataError(format!("unsupported input builder schema {version}; expected 9 or 10; reimport the dumps")));
    }
    // Current import schema is required; no old year-column/subtitle branches.
    c.prepare("SELECT b.book_year,s.subtitle,w.subtitle FROM edition_bibliography b LEFT JOIN edition_subtitle s USING(edition_id) LEFT JOIN edition e USING(edition_id) LEFT JOIN work_subtitle w USING(work_id) LIMIT 0").map_err(error)?;
    Ok(())
}
pub(crate) fn has_lcc(answer: &RichIsbnMetadataResult) -> bool {
    answer.matches.iter().flat_map(|m| &m.classifications).any(|c| c.scheme == ClassificationScheme::LibraryOfCongress && metadata_contract::classification_consensus_notation(c.scheme, &c.notation).is_some())
}

fn put_authority(out: &Connection, m: metadata_contract::AuthoritySubjectMatch, keys: Vec<String>) -> Result<(), MetadataError> {
    let json = serde_json::to_vec(&m).map_err(error)?;
    let authors = serde_json::to_string(&m.authors).map_err(error)?;
    for key in keys {
        out.execute("INSERT OR IGNORE INTO authority_title VALUES(?1,?2,?3,?4,?5)", params![key, authors, m.provider_id, m.record_ids[0], json]).map_err(error)?;
    }
    Ok(())
}
fn build_authorities(source: &Connection, out: &Connection) -> Result<(), MetadataError> {
    work_lookup::build_work_lookup(source, out)?;
    use metadata_contract::matching::NormalizedTitle;
    use metadata_contract::{AuthoritySubjectMatch, ClassificationEvidence};
    if has_table(source, "lc_title")? {
        #[derive(Deserialize)]
        struct Record {
            title: String,
            main_title: String,
            authors: Vec<String>,
        }
        let mut q = source.prepare("SELECT lc_record_id,metadata FROM lc_record ORDER BY lc_record_id").map_err(error)?;
        for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(error)? {
            let (id, json) = row.map_err(error)?;
            let record: Record = serde_json::from_str(&json).map_err(error)?;
            let codes = source
                .prepare_cached("SELECT notation FROM lc_record_lcc WHERE lc_record_id=?1 ORDER BY notation")
                .map_err(error)?
                .query_map([&id], |r| r.get::<_, String>(0))
                .map_err(error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(error)?;
            let classifications = authority_codes(&codes, format!("library_of_congress_title_author:{id}"));
            if classifications.is_empty() {
                continue;
            }
            let keys = source.prepare_cached("SELECT title_key FROM lc_title WHERE lc_record_id=?1").map_err(error)?.query_map([&id], |r| r.get::<_, String>(0)).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            put_authority(out, AuthoritySubjectMatch { title: record.title, main_title: record.main_title, authors: record.authors, provider_id: "library_of_congress".into(), record_ids: vec![id], classifications }, keys)?;
        }
    }
    let mut q=source.prepare("SELECT b.edition_id,b.title,s.subtitle FROM edition_bibliography b LEFT JOIN edition_subtitle s USING(edition_id) WHERE b.edition_id<0 AND NOT EXISTS(SELECT 1 FROM edition_isbn i WHERE i.edition_id=b.edition_id) ORDER BY b.edition_id").map_err(error)?;
    for row in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).map_err(error)? {
        let (id, title, subtitle) = row.map_err(error)?;
        let authors = source
            .prepare_cached("SELECT a.name FROM edition_author e JOIN author a USING(author_id) WHERE e.edition_id=?1 AND a.name IS NOT NULL ORDER BY e.position")
            .map_err(error)?
            .query_map([id], |r| r.get::<_, String>(0))
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        let codes = source
            .prepare_cached("SELECT notation FROM edition_classification WHERE edition_id=?1 AND scheme=2 ORDER BY notation")
            .map_err(error)?
            .query_map([id], |r| r.get::<_, String>(0))
            .map_err(error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        let classifications = authority_codes(&codes, format!("wikidata_title_author:Q{}", -id));
        if classifications.is_empty() {
            continue;
        }
        let full = subtitle.map(|s| format!("{title}: {s}")).unwrap_or_else(|| title.clone());
        let keys = NormalizedTitle::new(&full).lookup_keys();
        put_authority(out, AuthoritySubjectMatch { main_title: title, title: full, authors, provider_id: "wikidata".into(), record_ids: vec![format!("Q{}", -id)], classifications }, keys)?;
    }
    fn authority_codes(codes: &[String], method: String) -> Vec<Classification> {
        codes
            .iter()
            .filter_map(|n| metadata_contract::classification_consensus_notation(ClassificationScheme::LibraryOfCongress, n))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|notation| Classification {
                scheme: ClassificationScheme::LibraryOfCongress,
                notation,
                source: ClassificationSource::Work,
                evidence: vec![ClassificationEvidence { method: method.clone(), isbn13: String::new(), open_library_work_id: String::new(), open_library_edition_id: String::new() }],
            })
            .collect()
    }
    Ok(())
}

/// Fold extracted Wikidata evidence into a copied current-schema dump before building answers.
pub fn import_authorities(snapshot: &Path, evidence: &Path, target: &Path) -> Result<(), MetadataError> {
    crate::build::authority_ingestion::ingest_wikidata_authorities(&Resolver::open(snapshot, None)?, evidence, target)
}

#[cfg(test)]
mod wanted_tests {
    use super::*;
    #[test]
    fn lcc_on_one_ambiguous_claimant_does_not_close_an_unclassified_work() {
        let dir = tempfile::tempdir().unwrap();
        let source = snapshot_from(
            dir.path(),
            &[
                serde_json::json!({"key":"/books/OL1M","works":[{"key":"/works/OL10W"}],"isbn_13":["9780306406157"],"lc_classifications":["QA76"]}),
                serde_json::json!({"key":"/books/OL2M","works":[{"key":"/works/OL20W"}],"isbn_13":["9780306406157","9780131103627"]}),
            ],
            &[],
            &[],
        );
        let out = dir.path().join("answers.sqlite");
        build_answers(&source, None, None, None, &out).unwrap();
        assert_eq!(Connection::open(out).unwrap().query_row("SELECT work_id FROM wanted WHERE isbn13=9780131103627", [], |r| r.get::<_, i64>(0)).unwrap(), 20);
    }
}

/// Distinct work owners of an answer already known to contain usable LCC.
fn classified_works(answer: &RichIsbnMetadataResult) -> BTreeSet<i64> {
    answer.matches.iter().filter(|m| m.classifications.iter().any(|c| c.scheme == ClassificationScheme::LibraryOfCongress)).filter_map(|m| m.open_library_work_id.as_deref().and_then(|id| open_library_id(id, 'W'))).collect()
}

fn remove_answer(out: &Connection, isbn: i64) -> Result<(bool, u64), MetadataError> {
    let bytes = out.query_row("SELECT length(answer) FROM isbn_answer WHERE isbn13=?1", [isbn], |r| r.get::<_, i64>(0)).optional().map_err(error)?;
    let Some(bytes) = bytes else { return Ok((false, 0)) };
    for table in ["isbn_answer", "wanted", "answer_classification"] {
        out.execute(&format!("DELETE FROM {table} WHERE isbn13=?1"), [isbn]).map_err(error)?;
    }
    Ok((true, u64::try_from(bytes).map_err(error)?))
}

fn write_answer_batch(out: &Connection, rows: Vec<(i64, i64)>, response: Vec<RichIsbnMetadataResult>) -> Result<(u64, u64), MetadataError> {
    let mut count = 0;
    let mut bytes = 0;
    let mut descriptions = BTreeMap::<i64, String>::new();
    let mut wanted = out.prepare_cached("INSERT INTO wanted VALUES(?1,?2,?3)").map_err(error)?;
    let mut work_lcc = out.prepare_cached("INSERT OR IGNORE INTO answer_work_lcc VALUES(?1)").map_err(error)?;
    let mut isbn_classification = out.prepare_cached("INSERT INTO answer_classification VALUES(?1,?2)").map_err(error)?;
    let mut put_description = out.prepare_cached("INSERT OR IGNORE INTO work_description VALUES(?1,?2)").map_err(error)?;
    let mut get_description = out.prepare_cached("SELECT text FROM work_description WHERE work_id=?1").map_err(error)?;
    let mut put_answer = out.prepare_cached("INSERT INTO isbn_answer VALUES(?1,?2)").map_err(error)?;
    for ((isbn, requests), mut answer) in rows.into_iter().zip(response) {
        let work = answer.matches.first().filter(|_| answer.matches.len() == 1).and_then(|m| m.open_library_work_id.as_deref()).and_then(|id| open_library_id(id, 'W'));
        let classified = has_lcc(&answer);
        if !classified {
            wanted.execute(params![isbn, if requests > 0 { (-1_000_000_000_000_i64).saturating_sub(requests) } else { 0 }, work]).map_err(error)?;
        }
        if classified {
            if answer.matches.len() == 1 {
                isbn_classification.execute(params![isbn, serde_json::to_vec(&answer.matches[0].classifications).map_err(error)?]).map_err(error)?;
            }
            for work in classified_works(&answer) {
                work_lcc.execute([work]).map_err(error)?;
            }
        }
        let mut description_works = vec![];
        for m in &mut answer.matches {
            let mut description_work = None;
            if let (Some(work), Some(text)) = (m.open_library_work_id.as_deref().and_then(|id| open_library_id(id, 'W')), m.description.as_ref()) {
                // Only a single description per work can be shared. Conflicting edition
                // descriptions remain in their answers rather than changing the result.
                if let std::collections::btree_map::Entry::Vacant(entry) = descriptions.entry(work) {
                    put_description.execute(params![work, text]).map_err(error)?;
                    entry.insert(get_description.query_row([work], |r| r.get(0)).map_err(error)?);
                }
                if descriptions.get(&work) == Some(text) {
                    m.description = None;
                    description_work = Some(work);
                }
            }
            description_works.push(description_work);
        }
        let json = serde_json::to_vec(&schema::StoredAnswer { result: answer, description_works }).map_err(error)?;
        bytes += json.len() as u64;
        put_answer.execute(params![isbn, json]).map_err(error)?;
        count += 1;
    }
    Ok((count, bytes))
}
