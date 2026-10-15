//! Transactional maintenance of work LCC in an existing answer database.
use super::{
    batch::{load_work_codes_cached, WorkEvidenceCache},
    incremental::{self, InputIdentity, RECIPE_VERSION},
    *,
};
use metadata_contract::{classification_consensus_notation, RichWorkMetadataMatch};
use rusqlite::{backup::Backup, params_from_iter};

type BuildState = (i64, String, i64, i64);

fn state(c: &Connection) -> Result<Option<BuildState>, MetadataError> {
    if !has_table(c, "answer_build")? {
        return Ok(None);
    }
    c.query_row("SELECT recipe_version,inputs,isbn_rows,json_bytes FROM answer_build WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(error)
}

fn lcc(m: &RichWorkMetadataMatch) -> Vec<Classification> {
    m.classifications.iter().filter(|c| c.scheme == ClassificationScheme::LibraryOfCongress).cloned().collect()
}

/// Explicit legacy migration asserts that SNAPSHOT is the input of ANSWERS.
/// Recorded inputs are checked exactly; legacy files must share the dump date.
/// Only work LCC and dependent search/queue entries are changed, together with
/// pending provider/demand evidence. Readers must use ordinary SQLite reads.
pub fn reclassify_answers(snapshot: &Path, providers: Option<&Path>, demand: Option<&Path>, descriptions: Option<&Path>, ratings: Option<&Path>, answers: &Path) -> Result<BuildReport, MetadataError> {
    let started = Instant::now();
    let inputs = InputIdentity::read(snapshot, descriptions, ratings)?;
    let resolver = Resolver::open(snapshot, descriptions)?;
    let source = resolver.connection()?;
    validate_source(&source)?;
    let out = Connection::open_with_flags(answers, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(error)?;
    out.busy_timeout(Duration::from_secs(30)).map_err(error)?;
    if crate::snapshot::is_update(&out)? {
        return Err(MetadataError("in-place reclassification requires a full answer database, not an overlay".into()));
    }
    for input in [Some(snapshot), providers, demand, descriptions, ratings].into_iter().flatten() {
        crate::snapshot::require_separate(&out, input, "reclassification input")?;
    }
    out.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA cache_size=-65536; BEGIN IMMEDIATE;").map_err(error)?;
    let (version, date): (i64, String) = out.query_row("SELECT schema_version,dump_date FROM snapshot WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)?;
    if version != schema::ANSWER_VERSION || date != resolver.state.snapshot.dump_date {
        return Err(MetadataError("reclassification requires the same answer schema and imported dump date".into()));
    }
    let old_state = state(&out)?;
    if let Some((recipe, identity, _, _)) = &old_state {
        if ![2, RECIPE_VERSION].contains(recipe) || serde_json::from_str::<InputIdentity>(identity).map_err(error)? != inputs {
            return Err(MetadataError("reclassification requires supported classification rules and unchanged immutable inputs".into()));
        }
    }
    // Legacy bootstrapping reads record lengths once, without deserializing or
    // resolving them. Subsequent updates use the saved exact totals.
    let (mut count, mut bytes) = if let Some((_, _, count, bytes)) = old_state {
        (u64::try_from(count).map_err(error)?, u64::try_from(bytes).map_err(error)?)
    } else {
        tracing::info!("counting legacy answer rows and bytes for incremental state");
        let (count, bytes): (i64, i64) = out.query_row("SELECT count(*),COALESCE(sum(length(answer)),0) FROM isbn_answer", [], |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)?;
        (u64::try_from(count).map_err(error)?, u64::try_from(bytes).map_err(error)?)
    };
    let provider = providers.map(read_connection).transpose()?;
    let original_count = count;
    let demand_input = demand.map(read_connection).transpose()?;
    for c in provider.iter().chain(demand_input.iter()) {
        c.execute_batch("BEGIN").map_err(error)?;
    }
    let result = (|| {
        // Add only absent bookkeeping tables; never replace existing evidence.
        for statement in incremental::STATE_SCHEMA.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            out.execute_batch(&statement.replacen("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ", 1)).map_err(error)?;
        }
        out.execute_batch("CREATE INDEX IF NOT EXISTS answer_provider_by_isbn ON answer_provider(isbn13,provider); CREATE INDEX IF NOT EXISTS edition_candidate_by_work ON edition_candidate(work_id); CREATE INDEX IF NOT EXISTS edition_title_key_by_edition ON edition_title_key(edition_id); CREATE TEMP TABLE changed_work(work_id INTEGER PRIMARY KEY); CREATE TEMP TABLE removed_work_lcc(work_id INTEGER PRIMARY KEY);").map_err(error)?;
        popularity::load(&out, ratings)?;
        let mut cache = WorkEvidenceCache::default();
        let mut changed = 0_u64;
        let mut indexed = 0_u64;
        let mut visited = 0_u64;
        // Visit the ISBN index in edition order: edition/classification page
        // reads stay ordered instead of probing millions of singleton works.
        let mut uri = reqwest::Url::from_file_path(snapshot.canonicalize().map_err(error)?).map_err(|_| MetadataError("invalid reclassification source path".into()))?;
        uri.set_query(Some("mode=ro"));
        out.execute("ATTACH DATABASE ?1 AS classification_source", [uri.as_str()]).map_err(error)?;
        tracing::info!("selecting work-classification owners with unclassified ISBN editions");
        out.execute_batch(
            "CREATE TEMP TABLE reclassify_work(work_id INTEGER PRIMARY KEY) WITHOUT ROWID;
            INSERT OR IGNORE INTO reclassify_work SELECT e.work_id
            FROM classification_source.edition_isbn i INDEXED BY edition_isbn_by_edition
            JOIN classification_source.edition e USING(edition_id)
            WHERE e.work_id IS NOT NULL
            AND NOT EXISTS(SELECT 1 FROM classification_source.edition_classification c WHERE c.edition_id=e.edition_id AND c.scheme=2)
            AND (EXISTS(SELECT 1 FROM classification_source.work_classification w WHERE w.work_id=e.work_id AND w.scheme=2)
                 OR EXISTS(SELECT 1 FROM classification_source.edition_work_classification w WHERE w.work_id=e.work_id AND w.scheme=2));",
        )
        .map_err(error)?;
        let eligible: i64 = out.query_row("SELECT count(*) FROM reclassify_work", [], |r| r.get(0)).map_err(error)?;
        tracing::info!(eligible_works = eligible, "selected inherited classification owners");
        let mut after = i64::MIN;
        loop {
            // Release the temporary-table cursor before candidate rebuilding
            // creates/drops its own temporary tables on this connection.
            let works = {
                let mut query = out.prepare("SELECT work_id FROM reclassify_work WHERE work_id>?1 ORDER BY work_id LIMIT 500").map_err(error)?;
                let rows = query.query_map([after], |r| r.get::<_, i64>(0)).map_err(error)?.collect::<Result<BTreeSet<_>, _>>().map_err(error)?;
                rows
            };
            if works.is_empty() {
                break;
            }
            after = *works.last().unwrap();
            visited += works.len() as u64;
            let sql = format!("SELECT e.edition_id,e.work_id,i.isbn13 FROM edition e JOIN edition_isbn i USING(edition_id) WHERE e.work_id IN ({}) AND NOT EXISTS(SELECT 1 FROM edition_classification c WHERE c.edition_id=e.edition_id AND c.scheme=2) ORDER BY e.edition_id,i.isbn13", sql_placeholders(works.len()));
            let mut q = source.prepare_cached(&sql).map_err(error)?;
            let editions = q.query_map(params_from_iter(&works), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            // Exact-edition-only works have no inherited answers or candidates
            // to migrate. Avoid loading their complete sibling evidence.
            let needed = editions.iter().map(|r| r.1).collect::<BTreeSet<_>>();
            if needed.is_empty() {
                continue;
            }
            let selected = load_work_codes_cached(&source, &needed, &mut cache)?;
            let isbns = editions.iter().map(|r| r.2).collect::<BTreeSet<_>>();
            for chunk in isbns.iter().copied().collect::<Vec<_>>().chunks(BUILD_BATCH_ISBNS) {
                let fresh = batch::lookup_many_cached(&source, chunk.iter().map(|isbn| format!("{isbn:013}")).collect(), &mut cache)?;
                let mut fresh = fresh
                    .into_iter()
                    .map(|r| RichIsbnMetadataResult {
                        requested_isbn: r.requested_isbn,
                        canonical_isbn13: r.canonical_isbn13,
                        status: r.status,
                        matches: r
                            .matches
                            .into_iter()
                            .map(|m| RichWorkMetadataMatch {
                                open_library_work_id: m.open_library_work_id,
                                exact_edition_ids: m.exact_edition_ids,
                                classifications: m.classifications,
                                authors: vec![],
                                description: None,
                                popularity: None,
                                subjects: vec![],
                            })
                            .collect(),
                    })
                    .collect::<Vec<_>>();
                if let Some(c) = &provider {
                    providers::apply_many(c, &mut fresh)?;
                }
                for (&isbn, fresh) in chunk.iter().zip(fresh) {
                    let json: Option<Vec<u8>> = out.query_row("SELECT answer FROM isbn_answer WHERE isbn13=?1", [isbn], |r| r.get(0)).optional().map_err(error)?;
                    let Some(json) = json else { continue };
                    let mut stored: schema::StoredAnswer = serde_json::from_slice(&json).map_err(error)?;
                    // Preserve ambiguity, identities and every rich field.
                    if stored.result.matches.len() != fresh.matches.len() || stored.result.matches.iter().zip(&fresh.matches).any(|(a, b)| a.open_library_work_id != b.open_library_work_id || a.exact_edition_ids != b.exact_edition_ids) {
                        return Err(MetadataError(format!("cached identity differs from the supplied core for ISBN {isbn}; reclassification was rolled back")));
                    }
                    let mut different = false;
                    for (cached, new) in stored.result.matches.iter_mut().zip(fresh.matches) {
                        let codes = lcc(&new);
                        if lcc(cached) == codes {
                            continue;
                        }
                        different = true;
                        if let Some(work) = cached.open_library_work_id.as_deref().and_then(|id| open_library_id(id, 'W')) {
                            out.execute("INSERT OR IGNORE INTO changed_work VALUES(?1)", [work]).map_err(error)?;
                            if lcc(cached).iter().any(|c| classification_consensus_notation(c.scheme, &c.notation).is_some()) && !codes.iter().any(|c| classification_consensus_notation(c.scheme, &c.notation).is_some()) {
                                out.execute("INSERT OR IGNORE INTO removed_work_lcc VALUES(?1)", [work]).map_err(error)?;
                            }
                        }
                        cached.classifications.retain(|c| c.scheme != ClassificationScheme::LibraryOfCongress);
                        cached.classifications.extend(codes);
                        cached.classifications.sort_by(|a, b| (a.scheme, &a.notation, a.source).cmp(&(b.scheme, &b.notation, b.source)));
                    }
                    if different {
                        let replacement = serde_json::to_vec(&stored).map_err(error)?;
                        bytes = bytes - json.len() as u64 + replacement.len() as u64;
                        out.execute("UPDATE isbn_answer SET answer=?1 WHERE isbn13=?2", params![replacement, isbn]).map_err(error)?;
                        out.execute("DELETE FROM answer_classification WHERE isbn13=?1", [isbn]).map_err(error)?;
                        if has_lcc(&stored.result) {
                            if stored.result.matches.len() == 1 {
                                out.execute("INSERT INTO answer_classification VALUES(?1,?2)", params![isbn, serde_json::to_vec(&stored.result.matches[0].classifications).map_err(error)?]).map_err(error)?;
                            }
                            for work in classified_works(&stored.result) {
                                out.execute("INSERT OR IGNORE INTO answer_work_lcc VALUES(?1)", [work]).map_err(error)?;
                            }
                        }
                        changed += 1;
                    }
                }
            }
            let mut rebuild = BTreeSet::new();
            for (edition, work, _) in &editions {
                if rebuild.contains(edition) {
                    continue;
                }
                let codes = selected.get(work).cloned().unwrap_or_default().into_iter().map(|(scheme, notation)| Classification { scheme, notation, source: ClassificationSource::Work, evidence: vec![] }).collect::<Vec<_>>();
                let existing: Option<Vec<u8>> = out.query_row("SELECT classifications FROM edition_candidate WHERE edition_id=?1", [edition], |r| r.get(0)).optional().map_err(error)?;
                let existing: Vec<Classification> = existing.map(|json| serde_json::from_slice(&json)).transpose().map_err(error)?.unwrap_or_default();
                // Empty work codes can leave an exact authority/ISBN fallback.
                let replace = if codes.is_empty() { existing.iter().any(|c| c.scheme == ClassificationScheme::LibraryOfCongress && c.source == ClassificationSource::Work) } else { codes != existing };
                if replace {
                    out.execute("INSERT OR IGNORE INTO changed_work VALUES(?1)", [work]).map_err(error)?;
                    if !existing.is_empty() && codes.iter().any(|c| classification_consensus_notation(c.scheme, &c.notation).is_some()) {
                        // Frozen bibliographic inputs and title keys are already
                        // correct. Replace only this row's classification bytes.
                        out.execute("UPDATE edition_candidate SET classifications=?1 WHERE edition_id=?2", params![serde_json::to_vec(&codes).map_err(error)?, edition]).map_err(error)?;
                        indexed += 1;
                    } else {
                        rebuild.insert(*edition);
                    }
                }
            }
            if !rebuild.is_empty() {
                // Changed answers already have current summaries. Legacy
                // ISBN fallbacks read only their requested cached answer.
                for chunk in rebuild.iter().copied().collect::<Vec<_>>().chunks(500) {
                    let sql = sql_placeholders(chunk.len());
                    out.execute(&format!("DELETE FROM edition_title_key WHERE edition_id IN ({sql})"), params_from_iter(chunk)).map_err(error)?;
                    out.execute(&format!("DELETE FROM edition_candidate WHERE edition_id IN ({sql})"), params_from_iter(chunk)).map_err(error)?;
                }
                identity::build_candidates_for(&source, &out, Some(&rebuild))?;
                indexed += rebuild.len() as u64;
            }
            if visited % 10_000 < 500 {
                tracing::info!(works = visited, changed_answers = changed, changed_editions = indexed, elapsed_seconds = started.elapsed().as_secs_f64(), "reclassifying work evidence");
            }
        }
        let (resolved, removed) = refresh_inputs(&resolver, &source, &out, provider.as_ref(), demand_input.as_ref(), &mut cache, &mut count, &mut bytes)?;
        let mut q = out.prepare("SELECT work_id FROM changed_work ORDER BY work_id").map_err(error)?;
        let mut rows = q.query_map([], |r| r.get::<_, i64>(0)).map_err(error)?;
        loop {
            let works = rows.by_ref().take(500).collect::<Result<BTreeSet<_>, _>>().map_err(error)?;
            if works.is_empty() {
                break;
            }
            let mut recount = BTreeSet::new();
            for &work in &works {
                let (removed, classified): (bool, bool) = out
                    .query_row("SELECT EXISTS(SELECT 1 FROM removed_work_lcc WHERE work_id=?1), EXISTS(SELECT 1 FROM answer_work_lcc WHERE work_id=?1) OR EXISTS(SELECT 1 FROM edition_candidate WHERE work_id=?1)", [work], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .map_err(error)?;
                // A surviving answer or candidate already closes the queue.
                // Recount when evidence was removed or the work may reopen.
                if removed || !classified {
                    recount.insert(work);
                }
            }
            if !recount.is_empty() {
                refresh_work_state(&source, &out, &recount)?;
            }
            let sql = format!("DELETE FROM wanted WHERE work_id IN ({}) AND (work_id IN (SELECT work_id FROM answer_work_lcc) OR EXISTS(SELECT 1 FROM edition_candidate c WHERE c.work_id=wanted.work_id))", sql_placeholders(works.len()));
            out.execute(&sql, params_from_iter(&works)).map_err(error)?;
        }
        drop(rows);
        drop(q);
        out.execute_batch("DROP TABLE changed_work; DROP TABLE removed_work_lcc; DROP TABLE reclassify_work; DROP TABLE build_rating;").map_err(error)?;
        incremental::save_counts(&out, &inputs, count, bytes)?;
        let through = provider.as_ref().map(|c| c.query_row("SELECT COALESCE(MAX(fetched_at_ms),0) FROM provider_result", [], |r| r.get::<_, i64>(0)).map_err(error)).transpose()?.unwrap_or(0);
        out.execute("UPDATE snapshot SET built_at_ms=?1,provider_results_through_ms=?2 WHERE singleton=1", params![now_ms(), through]).map_err(error)?;
        inputs.verify(snapshot, descriptions, ratings)?;
        // Updates and index writes are constraint-checked by SQLite. Do not
        // scan every untouched page of a large base for targeted maintenance.
        out.execute_batch("COMMIT;").map_err(error)?;
        Ok(BuildReport {
            mode: "reclassified",
            resolved_rows: resolved,
            reused_rows: original_count.saturating_sub(removed),
            reclassified_rows: changed,
            reclassified_editions: indexed,
            isbn_rows: count,
            answer_json_bytes: bytes,
            file_bytes: fs::metadata(answers).map_err(error)?.len(),
            elapsed_seconds: started.elapsed().as_secs_f64(),
            isbns_per_second: (changed + resolved) as f64 / started.elapsed().as_secs_f64(),
            preparation_seconds: 0.0,
            lookup_seconds: 0.0,
            provider_seconds: 0.0,
            write_seconds: 0.0,
            edition_index_seconds: 0.0,
            authority_index_seconds: 0.0,
            queue_seconds: 0.0,
        })
    })();
    if result.is_err() {
        let _ = out.execute_batch("ROLLBACK");
    }
    result
}

// Preserve the existing `build --previous` publication contract. Explicit
// `reclassify` updates ANSWERS directly; dated publications reuse its SQL path
// on a copy and keep PREVIOUS untouched.
pub(super) fn publish_reclassification(snapshot: &Path, providers: Option<&Path>, demand: Option<&Path>, descriptions: Option<&Path>, ratings: Option<&Path>, previous: &Path, output: &Path) -> Result<BuildReport, MetadataError> {
    let temporary = output.with_extension(format!("reclassifying-{}", std::process::id()));
    let _reservation = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(error)?;
    let result = (|| {
        let old = crate::snapshot::open(previous)?;
        if crate::snapshot::is_update(&old)? {
            return Err(MetadataError("classification recipe migration of an overlay requires a full database".into()));
        }
        // A mutable base may have committed pages in WAL. SQLite backup also
        // gives a consistent source if another writer commits during copying.
        let mut copy = Connection::open(&temporary).map_err(error)?;
        Backup::new(&old, &mut copy).map_err(error)?.run_to_completion(8192, Duration::from_millis(10), None).map_err(error)?;
        drop(copy);
        let mut report = reclassify_answers(snapshot, providers, demand, descriptions, ratings, &temporary)?;
        // A dated file must be self-contained for read-only deployment.
        // There are no readers on this private copy while journaling changes.
        let copy = Connection::open(&temporary).map_err(error)?;
        copy.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").map_err(error)?;
        drop(copy);
        report.file_bytes = fs::metadata(&temporary).map_err(error)?.len();
        fs::hard_link(&temporary, output).map_err(error)?;
        Ok(report)
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn refresh_inputs(
    resolver: &Resolver, source: &Connection, out: &Connection, provider: Option<&Connection>, demand: Option<&Connection>, cache: &mut WorkEvidenceCache, count: &mut u64, bytes: &mut u64,
) -> Result<(u64, u64), MetadataError> {
    let mut metadata = BTreeSet::new();
    let mut demand_updates = BTreeSet::new();
    incremental::provider_changes(out, provider, &mut metadata)?;
    incremental::demand_changes(out, demand, &mut demand_updates)?;
    let changes = metadata.union(&demand_updates).copied().collect::<BTreeSet<_>>();
    for &isbn in &demand_updates {
        let existed: bool = out.query_row("SELECT EXISTS(SELECT 1 FROM isbn_answer WHERE isbn13=?1)", [isbn], |r| r.get(0)).map_err(error)?;
        if !existed || !incremental::included(source, provider, demand, isbn)? {
            metadata.insert(isbn);
        }
    }
    let (refresh, editions, works) = incremental::affected(source, &metadata)?;
    if changes.len() > 100_000 || refresh.len() > 100_000 {
        return Err(MetadataError("pending provider/demand changes exceed the update limit; reclassification was rolled back".into()));
    }
    let mut resolved = 0;
    let mut removed = 0;
    for chunk in works.iter().copied().collect::<Vec<_>>().chunks(500) {
        out.execute(&format!("DELETE FROM answer_work_lcc WHERE work_id IN ({})", sql_placeholders(chunk.len())), params_from_iter(chunk)).map_err(error)?;
    }
    for chunk in refresh.iter().copied().collect::<Vec<_>>().chunks(BUILD_BATCH_ISBNS) {
        let mut rows = Vec::new();
        for &isbn in chunk {
            let (existed, old_bytes) = remove_answer(out, isbn)?;
            if existed {
                *count -= 1;
                *bytes -= old_bytes;
                removed += 1;
            }
            if incremental::included(source, provider, demand, isbn)? {
                let requests = demand.map(|c| c.query_row("SELECT requests FROM demand WHERE isbn13=?1", [isbn], |r| r.get::<_, i64>(0)).optional().map_err(error)).transpose()?.flatten().unwrap_or(0);
                rows.push((isbn, requests));
            }
        }
        let mut response = resolver.enrich_build_batch_cached(rows.iter().map(|(isbn, _)| format!("{isbn:013}")).collect(), cache)?;
        if let Some(c) = provider {
            providers::apply_many(c, &mut response)?;
        }
        popularity::attach(out, &mut response, &resolver.state.snapshot.dump_date)?;
        let (written, new_bytes) = write_answer_batch(out, rows, response)?;
        *count += written;
        *bytes += new_bytes;
        resolved += written;
    }
    for chunk in editions.iter().copied().collect::<Vec<_>>().chunks(500) {
        let sql = sql_placeholders(chunk.len());
        out.execute(&format!("DELETE FROM edition_title_key WHERE edition_id IN ({sql})"), params_from_iter(chunk)).map_err(error)?;
        out.execute(&format!("DELETE FROM edition_candidate WHERE edition_id IN ({sql})"), params_from_iter(chunk)).map_err(error)?;
    }
    identity::build_candidates_for(source, out, Some(&editions))?;
    for work in works {
        out.execute("INSERT OR IGNORE INTO changed_work VALUES(?1)", [work]).map_err(error)?;
    }
    incremental::sync_inputs(out, provider, demand, &changes)?;
    for &isbn in &demand_updates {
        let requests = out.query_row("SELECT requests FROM answer_demand WHERE isbn13=?1", [isbn], |r| r.get::<_, i64>(0)).optional().map_err(error)?.unwrap_or(0);
        let rank = if requests > 0 { (-1_000_000_000_000_i64).saturating_sub(requests) } else { 0 };
        out.execute("UPDATE wanted SET rank=?1 WHERE isbn13=?2", params![rank, isbn]).map_err(error)?;
    }
    let rerank = refresh.union(&demand_updates).copied().collect::<Vec<_>>();
    for chunk in rerank.chunks(500) {
        out.execute(&format!("UPDATE wanted SET rank=-COALESCE((SELECT count FROM build_rating WHERE work_id=wanted.work_id),0) WHERE rank=0 AND isbn13 IN ({})", sql_placeholders(chunk.len())), params_from_iter(chunk)).map_err(error)?;
    }
    Ok((resolved, removed))
}

fn refresh_work_state(source: &Connection, out: &Connection, works: &BTreeSet<i64>) -> Result<(), MetadataError> {
    let ids = sql_placeholders(works.len());
    out.execute(&format!("DELETE FROM answer_work_lcc WHERE work_id IN ({ids})"), params_from_iter(works)).map_err(error)?;
    let mut q = source.prepare_cached(&format!("SELECT DISTINCT i.isbn13 FROM edition e JOIN edition_isbn i USING(edition_id) WHERE e.work_id IN ({ids})")).map_err(error)?;
    for isbn in q.query_map(params_from_iter(works), |r| r.get::<_, i64>(0)).map_err(error)? {
        let isbn = isbn.map_err(error)?;
        let json: Option<Vec<u8>> = out.query_row("SELECT answer FROM isbn_answer WHERE isbn13=?1", [isbn], |r| r.get(0)).optional().map_err(error)?;
        let Some(json) = json else { continue };
        let stored: schema::StoredAnswer = serde_json::from_slice(&json).map_err(error)?;
        out.execute("DELETE FROM answer_classification WHERE isbn13=?1", [isbn]).map_err(error)?;
        if has_lcc(&stored.result) {
            if stored.result.matches.len() == 1 {
                out.execute("INSERT INTO answer_classification VALUES(?1,?2)", params![isbn, serde_json::to_vec(&stored.result.matches[0].classifications).map_err(error)?]).map_err(error)?;
            }
            for work in classified_works(&stored.result) {
                out.execute("INSERT OR IGNORE INTO answer_work_lcc VALUES(?1)", [work]).map_err(error)?;
            }
        } else {
            let work = stored.result.matches.first().filter(|_| stored.result.matches.len() == 1).and_then(|m| m.open_library_work_id.as_deref()).and_then(|id| open_library_id(id, 'W'));
            let requests = out.query_row("SELECT requests FROM answer_demand WHERE isbn13=?1", [isbn], |r| r.get::<_, i64>(0)).optional().map_err(error)?.unwrap_or(0);
            let rating = work.map(|work| out.query_row("SELECT count FROM build_rating WHERE work_id=?1", [work], |r| r.get::<_, i64>(0)).optional().map_err(error)).transpose()?.flatten().unwrap_or(0);
            let rank = if requests > 0 { (-1_000_000_000_000_i64).saturating_sub(requests) } else { -rating };
            out.execute("INSERT OR IGNORE INTO wanted VALUES(?1,?2,?3)", params![isbn, rank, work]).map_err(error)?;
        }
    }
    Ok(())
}
