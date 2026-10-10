//! Build immutable answers using the existing resolution rules, without network I/O.

#[cfg(test)]
use crate::crawl::{open_provider, save_loc};
#[cfg(test)]
use crate::crawl::library_of_congress::LibraryOfCongressLookup::Matched;
#[cfg(test)]
use crate::build::tests::snapshot_from;
use crate::schema::{self, now_ms};
use crate::*;

pub(crate) mod authority_ingestion;
pub(crate) mod duplicate_work;
pub(crate) mod enrichment;
pub(crate) mod identity;
pub(crate) mod import;
pub(crate) mod loc_dump;
pub(crate) mod openlibrary;
pub(crate) mod service;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod wikidata_books;

#[derive(Debug, Serialize)]
pub struct BuildReport {
    pub isbn_rows: u64,
    pub answer_json_bytes: u64,
    pub file_bytes: u64,
    pub elapsed_seconds: f64,
    pub isbns_per_second: f64,
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
    let resolver = Resolver::open(snapshot, descriptions)?;
    let source = resolver.connection()?;
    validate_source(&source)?;
    let provider = providers.map(read_connection).transpose()?;
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
        // A disk-backed union covers OL, supplements, crawled-only and requested-only ISBNs.
        out.execute_batch("CREATE TABLE build_isbn(isbn13 INTEGER PRIMARY KEY,requests INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID; CREATE TABLE build_work_lcc(work_id INTEGER PRIMARY KEY); BEGIN;").map_err(error)?;
        for table in ["edition_isbn", "isbn_classification", "isbn_lcc", "lc_record_isbn"] {
            if has_table(&source, table)? {
                let mut q = source.prepare(&format!("SELECT DISTINCT isbn13 FROM {table} ORDER BY isbn13")).map_err(error)?;
                for row in q.query_map([], |r| r.get::<_, i64>(0)).map_err(error)? {
                    out.execute("INSERT OR IGNORE INTO build_isbn(isbn13) VALUES(?1)", [row.map_err(error)?]).map_err(error)?;
                }
            }
        }
        if let Some(c) = &provider {
            let mut q = c.prepare("SELECT DISTINCT isbn13 FROM provider_result ORDER BY isbn13").map_err(error)?;
            for row in q.query_map([], |r| r.get::<_, i64>(0)).map_err(error)? {
                out.execute("INSERT OR IGNORE INTO build_isbn(isbn13) VALUES(?1)", [row.map_err(error)?]).map_err(error)?;
            }
        }
        if let Some(path) = demand {
            let c = read_connection(path)?;
            let mut q = c.prepare("SELECT isbn13,requests FROM demand ORDER BY isbn13").map_err(error)?;
            for row in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(error)? {
                let (isbn, requests) = row.map_err(error)?;
                out.execute("INSERT INTO build_isbn VALUES(?1,?2) ON CONFLICT(isbn13) DO UPDATE SET requests=excluded.requests", params![isbn, requests]).map_err(error)?;
            }
        }
        out.execute_batch("COMMIT").map_err(error)?;
        let mut after = 0;
        let mut count = 0;
        let mut bytes = 0;
        loop {
            let mut q = out.prepare("SELECT isbn13,requests FROM build_isbn WHERE isbn13>?1 ORDER BY isbn13 LIMIT 256").map_err(error)?;
            let rows = q.query_map([after], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            drop(q);
            if rows.is_empty() {
                break;
            }
            after = rows.last().unwrap().0;
            let response = resolver.enrich_rich(rows.iter().map(|(id, _)| format!("{id:013}")).collect())?;
            out.execute_batch("BEGIN").map_err(error)?;
            for ((isbn, requests), mut answer) in rows.into_iter().zip(response) {
                if let Some(c) = &provider {
                    apply_providers(c, isbn, &mut answer)?;
                }
                let work = answer.matches.first().filter(|_| answer.matches.len() == 1).and_then(|m| m.open_library_work_id.as_deref()).and_then(|id| open_library_id(id, 'W'));
                if !has_lcc(&answer) {
                    out.execute("INSERT INTO wanted VALUES(?1,?2,?3)", params![isbn, if requests > 0 { (-1_000_000_000_000_i64).saturating_sub(requests) } else { 0 }, work]).map_err(error)?;
                }
                if has_lcc(&answer) {
                    for work in
                        answer.matches.iter().filter(|m| m.classifications.iter().any(|c| c.scheme == ClassificationScheme::LibraryOfCongress)).filter_map(|m| m.open_library_work_id.as_deref().and_then(|id| open_library_id(id, 'W')))
                    {
                        out.execute("INSERT OR IGNORE INTO build_work_lcc VALUES(?1)", [work]).map_err(error)?;
                    }
                }
                let mut description_works = vec![];
                for m in &mut answer.matches {
                    let mut description_work = None;
                    if let (Some(work), Some(text)) = (m.open_library_work_id.as_deref().and_then(|id| open_library_id(id, 'W')), m.description.as_ref()) {
                        // Only a single description per work can be shared. Conflicting edition
                        // descriptions remain in their answers rather than changing the result.
                        out.execute("INSERT OR IGNORE INTO work_description VALUES(?1,?2)", params![work, text]).map_err(error)?;
                        let shared: String = out.query_row("SELECT text FROM work_description WHERE work_id=?1", [work], |r| r.get(0)).map_err(error)?;
                        if shared == *text {
                            m.description = None;
                            description_work = Some(work);
                        }
                    }
                    description_works.push(description_work);
                }
                let json = serde_json::to_vec(&schema::StoredAnswer { result: answer, description_works }).map_err(error)?;
                bytes += json.len() as u64;
                out.execute("INSERT INTO isbn_answer VALUES(?1,?2)", params![isbn, json]).map_err(error)?;
                count += 1;
            }
            out.execute_batch("COMMIT").map_err(error)?;
            if count % 100_000 < 256 {
                tracing::info!(isbn_rows = count, elapsed_seconds = started.elapsed().as_secs_f64(), "building answers");
            }
        }
        // All ISBNs of a work with a resolved LCC leave the crawler queue together.
        out.execute_batch("BEGIN").map_err(error)?;
        crate::build::identity::build_candidates(&source, &out)?;
        build_authorities(&source, &out)?;
        // Candidate evidence already contains exact/work/sibling consensus.
        out.execute_batch(
            "DELETE FROM wanted WHERE work_id IN (SELECT work_id FROM build_work_lcc UNION SELECT work_id FROM edition_candidate WHERE work_id IS NOT NULL);
DROP TABLE build_work_lcc;",
        )
        .map_err(error)?;
        if let Some(path) = ratings {
            out.execute_batch("CREATE TABLE build_rating(work_id INTEGER PRIMARY KEY,count INTEGER NOT NULL)").map_err(error)?;
            let reader = BufReader::new(MultiGzDecoder::new(File::open(path).map_err(error)?));
            let mut q = out.prepare_cached("INSERT INTO build_rating VALUES(?1,1) ON CONFLICT(work_id) DO UPDATE SET count=count+1").map_err(error)?;
            for line in reader.lines() {
                let line = line.map_err(error)?;
                if let Some(work) = line.split('\t').next().and_then(|id| open_library_id(id, 'W')) {
                    q.execute([work]).map_err(error)?;
                }
            }
            drop(q);
            out.execute_batch("UPDATE wanted SET rank=-COALESCE((SELECT count FROM build_rating WHERE work_id=wanted.work_id),0) WHERE rank=0; DROP TABLE build_rating;").map_err(error)?;
        }
        out.execute_batch("DROP TABLE build_isbn").map_err(error)?;
        out.execute("INSERT INTO snapshot VALUES(1,?1,?2,?3,?4)", params![schema::ANSWER_VERSION, resolver.state.snapshot.dump_date, now_ms(), through]).map_err(error)?;
        out.execute_batch("COMMIT; PRAGMA optimize;").map_err(error)?;
        let integrity: String = out.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(error)?;
        if integrity != "ok" {
            return Err(MetadataError(format!("answer integrity check failed: {integrity}")));
        }
        drop(out);
        // Same-filesystem publication fails if OUT appeared while building.
        std::fs::hard_link(&temporary, output).map_err(error)?;
        let elapsed = started.elapsed().as_secs_f64();
        Ok(BuildReport { isbn_rows: count, answer_json_bytes: bytes, file_bytes: std::fs::metadata(output).map_err(error)?.len(), elapsed_seconds: elapsed, isbns_per_second: count as f64 / elapsed })
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
    if version != 9 {
        return Err(MetadataError(format!("unsupported input builder schema {version}; expected 9; reimport the dumps")));
    }
    // Current import schema is required; no old year-column/subtitle branches.
    c.prepare("SELECT b.book_year,s.subtitle,w.subtitle FROM edition_bibliography b LEFT JOIN edition_subtitle s USING(edition_id) LEFT JOIN edition e USING(edition_id) LEFT JOIN work_subtitle w USING(work_id) LIMIT 0").map_err(error)?;
    Ok(())
}
pub(crate) fn has_lcc(answer: &RichIsbnMetadataResult) -> bool {
    answer.matches.iter().flat_map(|m| &m.classifications).any(|c| c.scheme == ClassificationScheme::LibraryOfCongress && metadata_contract::classification_consensus_notation(c.scheme, &c.notation).is_some())
}

fn apply_providers(c: &Connection, isbn: i64, answer: &mut RichIsbnMetadataResult) -> Result<(), MetadataError> {
    if answer.status == LookupStatus::InvalidIsbn || answer.matches.len() > 1 {
        return Ok(());
    }
    let mut q = c.prepare_cached("SELECT status,classifications,subjects FROM provider_result WHERE provider=?1 AND isbn13=?2").map_err(error)?;
    for provider in ["loc", "librarything"] {
        if has_lcc(answer) {
            break;
        }
        let value = q.query_row(params![provider, isbn], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?, r.get::<_, Vec<u8>>(2)?))).optional().map_err(error)?;
        let Some((status, codes, subjects)) = value else {
            continue;
        };
        if status != "matched" {
            continue;
        }
        let codes: Vec<Classification> = serde_json::from_slice(&codes).map_err(error)?;
        let subjects: Vec<SubjectHeading> = serde_json::from_slice(&subjects).map_err(error)?;
        if codes.is_empty() && subjects.is_empty() {
            continue;
        }
        if answer.matches.is_empty() {
            answer.matches.push(RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: vec![], classifications: vec![], authors: vec![], description: None, subjects: vec![] });
        }
        // Preserve existing schemes while allowing crawled LCC to fill a record
        // that previously had only BISAC or DDC.
        let present = answer.matches[0].classifications.iter().map(|c| c.scheme).collect::<BTreeSet<_>>();
        answer.matches[0].classifications.extend(codes.into_iter().filter(|c| !present.contains(&c.scheme)));
        answer.matches[0].subjects.extend(subjects);
        answer.matches[0].subjects.sort();
        answer.matches[0].subjects.dedup();
        answer.status = LookupStatus::Matched;
    }
    Ok(())
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
