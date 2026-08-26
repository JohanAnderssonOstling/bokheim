use super::{
    advance_catalog_transport_revision, normalize_search_text, params, prepare_catalog_subject_assignments, storage_i64, BTreeMap, BTreeSet, CatalogCache, CatalogCacheError, CatalogPublicationIdentity, CatalogSourceId,
    CatalogSubjectAssignment, CatalogSubjectProjectionAudit, CatalogUnmatchedDcSubject, Connection, OptionalExtension, Transaction, TransactionBehavior, CATALOG_BROWSE_SERVING_INDEX_VERSION, CATALOG_SEARCH_INDEX_VERSION,
    CATALOG_SUBJECT_BROWSE_INDEX_VERSION,
};
use publication_model::PublicationSubject;

pub(crate) const ENRICHED_SUBJECT_POSITION_BASE: i64 = 1_000_000;

impl CatalogCache {
    /// Builds the external-content FTS indexes once, after the HTTP listener is
    /// available. Their database triggers keep them current across later
    /// crawls, enrichment, and rollback to an older server binary.
    pub fn refresh_search_index(&self) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let current = connection.query_row("SELECT index_version=?1 AND ready=1 FROM catalog_search_index_metadata WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION], |row| row.get::<_, bool>(0))?;
        if current {
            return Ok(false);
        }
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("UPDATE catalog_search_index_metadata SET index_version=?1,ready=0 WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION])?;
        transaction.execute("INSERT INTO catalog_publication_title_fts(catalog_publication_title_fts) VALUES('rebuild')", [])?;
        transaction.execute("INSERT INTO catalog_publication_author_fts(catalog_publication_author_fts) VALUES('rebuild')", [])?;
        transaction.execute("INSERT INTO catalog_publication_toc_fts(catalog_publication_toc_fts) VALUES('rebuild')", [])?;
        transaction.execute("UPDATE catalog_search_index_metadata SET ready=1 WHERE singleton=1", [])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn search_index_ready(&self) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection.query_row("SELECT index_version=?1 AND ready=1 FROM catalog_search_index_metadata WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION], |row| row.get(0)).map_err(Into::into)
    }

    /// Rebuilds the Store's read-optimized projection from the authoritative
    /// ingestion tables. Readers keep using the previous complete snapshot
    /// until this transaction commits; the stored revision only decides
    /// whether another rebuild is required.
    pub fn refresh_browse_serving_index(&self) -> Result<bool, CatalogCacheError> {
        self.refresh_subject_browse_index()?;
        let mut connection = self.pool.get()?;
        let revision = connection.query_row("SELECT revision FROM catalog_transport_state WHERE singleton=1", [], |row| row.get::<_, i64>(0))?;
        let current = connection.query_row(
            "SELECT index_version=?1 AND revision=?2 AND ready=1
               FROM catalog_browse_serving_metadata WHERE singleton=1",
            params![CATALOG_BROWSE_SERVING_INDEX_VERSION, revision],
            |row| row.get::<_, bool>(0),
        )?;
        if current {
            return Ok(false);
        }

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("UPDATE catalog_browse_serving_metadata SET index_version=?1,revision=?2,ready=0 WHERE singleton=1", params![CATALOG_BROWSE_SERVING_INDEX_VERSION, revision])?;
        transaction.execute("DELETE FROM catalog_browse_subject", [])?;
        transaction.execute("DELETE FROM catalog_browse_language", [])?;
        transaction.execute("DELETE FROM catalog_browse_format", [])?;
        transaction.execute("DELETE FROM catalog_browse_search_document", [])?;
        transaction.execute("DELETE FROM catalog_browse_publication", [])?;

        transaction.execute(
            "INSERT INTO catalog_browse_publication(
                 entry_id,source_id,generation,edition_id,work_id,normalized_title,
                 provider_popularity,sequence,reading_log_count,rating_count,has_language)
             SELECT p.entry_id,p.source_id,p.generation,p.edition_id,p.work_id,p.normalized_title,
                    p.provider_popularity,p.sequence,
                    COALESCE(pop.reading_log_count,0),COALESCE(pop.rating_count,0),
                    EXISTS(SELECT 1 FROM catalog_publication_language language
                            WHERE language.source_id=p.source_id AND language.generation=p.generation
                              AND language.edition_id=p.edition_id AND language.work_id=p.work_id)
               FROM catalog_publication p
               JOIN catalog_source active
                 ON active.source_id=p.source_id AND active.active_generation=p.generation
               LEFT JOIN catalog_publication_popularity pop
                 ON pop.source_id=p.source_id AND pop.generation=p.generation
                AND pop.edition_id=p.edition_id AND pop.work_id=p.work_id
              WHERE p.importable=1
                AND (p.source_id!='opds-project-gutenberg'
                     OR p.work_id NOT LIKE 'urn:gutenberg:%'
                     OR NOT EXISTS(
                         SELECT 1 FROM catalog_publication preferred
                          WHERE preferred.source_id=p.source_id AND preferred.generation=p.generation
                            AND preferred.work_id=p.work_id AND preferred.importable=1
                            AND (preferred.gutenberg_has_images>p.gutenberg_has_images
                                 OR (preferred.gutenberg_has_images=p.gutenberg_has_images
                                     AND preferred.edition_id<p.edition_id))))",
            [],
        )?;
        transaction.execute(
            "INSERT INTO catalog_browse_search_document(entry_id,search_text)
             SELECT browse.entry_id,
                    trim(publication.normalized_title || ' ' ||
                         COALESCE((SELECT group_concat(author.normalized_name,' ')
                                     FROM catalog_publication_author author
                                    WHERE author.source_id=browse.source_id AND author.generation=browse.generation
                                      AND author.edition_id=browse.edition_id AND author.work_id=browse.work_id),'') || ' ' ||
                         COALESCE((SELECT group_concat(toc.normalized_title,' ')
                                     FROM catalog_publication_toc toc
                                    WHERE toc.source_id=browse.source_id AND toc.generation=browse.generation
                                      AND toc.edition_id=browse.edition_id AND toc.work_id=browse.work_id),''))
               FROM catalog_browse_publication browse
               JOIN catalog_publication publication
                 ON publication.source_id=browse.source_id AND publication.generation=browse.generation
                AND publication.edition_id=browse.edition_id AND publication.work_id=browse.work_id",
            [],
        )?;
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_browse_language(entry_id,language)
             SELECT browse.entry_id,
                    lower(substr(language.language,1,instr(language.language || '-', '-')-1))
               FROM catalog_browse_publication browse
               JOIN catalog_publication_language language
                 ON language.source_id=browse.source_id AND language.generation=browse.generation
                AND language.edition_id=browse.edition_id AND language.work_id=browse.work_id
              WHERE language.language!=''",
            [],
        )?;
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_browse_format(entry_id,book_format)
             SELECT browse.entry_id,format.book_format
               FROM catalog_browse_publication browse
               JOIN catalog_publication_format format
                 ON format.source_id=browse.source_id AND format.generation=browse.generation
                AND format.edition_id=browse.edition_id AND format.work_id=browse.work_id",
            [],
        )?;
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_browse_subject(entry_id,system_id,node_path,exact_assignment)
             SELECT browse.entry_id,subject.system_id,subject.node_path,subject.exact_assignment
               FROM catalog_browse_publication browse
               JOIN catalog_publication_subject_node subject
                 ON subject.source_id=browse.source_id AND subject.generation=browse.generation
                AND subject.edition_id=browse.edition_id AND subject.work_id=browse.work_id",
            [],
        )?;
        transaction.execute("INSERT INTO catalog_browse_search_fts(catalog_browse_search_fts) VALUES('rebuild')", [])?;
        transaction.execute("UPDATE catalog_browse_serving_metadata SET index_version=?1,revision=?2,ready=1 WHERE singleton=1", params![CATALOG_BROWSE_SERVING_INDEX_VERSION, revision])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn browse_serving_index_ready(&self) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection
            .query_row(
                "SELECT index_version=?1 AND ready=1
                   FROM catalog_browse_serving_metadata WHERE singleton=1",
                [CATALOG_BROWSE_SERVING_INDEX_VERSION],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Rebuilds persisted unified taxonomy assignments in bounded batches.
    /// Servers should run this away from their health-critical startup path.
    pub fn refresh_unified_subject_assignments(&self) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        refresh_unified_subject_assignments(&mut connection)
    }

    /// Materializes every browsable assignment as one deduplicated membership
    /// per ancestor node. Existing populated caches are rebuilt in a single
    /// transaction so readers see either the legacy index or the complete new
    /// hierarchy, never a partially populated tree.
    pub fn refresh_subject_browse_index(&self) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let current = connection.query_row("SELECT index_version=?1 FROM catalog_subject_browse_index_metadata WHERE singleton=1", [CATALOG_SUBJECT_BROWSE_INDEX_VERSION], |row| row.get::<_, bool>(0)).optional()?.unwrap_or(false);
        if current {
            return Ok(false);
        }

        const BATCH_SIZE: i64 = 512;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM catalog_publication_subject_node", [])?;
        transaction.execute("DELETE FROM catalog_subject_node", [])?;
        let mut rowid = 0_i64;
        loop {
            let assignments = {
                let mut statement = transaction.prepare(
                    "SELECT rowid,source_id,generation,edition_id,work_id,system_id,subject_path
                       FROM catalog_publication_subject_assignment
                      WHERE rowid>?1 AND system_id=?2 ORDER BY rowid LIMIT ?3",
                )?;
                let rows = statement
                    .query_map(params![rowid, subject_projection::UNIFIED_SYSTEM_ID, BATCH_SIZE], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            if assignments.is_empty() {
                break;
            }
            rowid = assignments.last().expect("non-empty assignment batch").0;
            for (_, source_id, generation, edition_id, work_id, system_id, subject_path) in assignments {
                insert_subject_browse_memberships(&transaction, &source_id, generation, &edition_id, &work_id, &system_id, &subject_path)?;
            }
        }
        transaction.execute(
            "INSERT INTO catalog_subject_browse_index_metadata(singleton,index_version) VALUES(1,?1)
             ON CONFLICT(singleton) DO UPDATE SET index_version=excluded.index_version",
            [CATALOG_SUBJECT_BROWSE_INDEX_VERSION],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    /// Audits only whether Dublin Core subject headings resolve to coded BISAC
    /// positions. Unified output never determines BISAC validity.
    pub fn subject_projection_audit(&self) -> Result<CatalogSubjectProjectionAudit, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT subject.source_id,subject.edition_id,subject.work_id,
                    subject.name,subject.source,subject.authority,subject.code
               FROM catalog_publication_subject subject
               JOIN catalog_source source ON source.source_id=subject.source_id AND source.active_generation=subject.generation
               JOIN catalog_publication publication ON publication.source_id=subject.source_id AND publication.generation=subject.generation
                 AND publication.edition_id=subject.edition_id AND publication.work_id=subject.work_id
              WHERE publication.importable=1
              ORDER BY subject.source_id,subject.edition_id,subject.work_id,subject.position",
        )?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, Option<String>>(6)?)))?;

        let mut dc_subject_occurrences = 0_u64;
        let mut matched_dc_subject_occurrences = 0_u64;
        let mut dc_headings = BTreeSet::<String>::new();
        let mut unmatched_headings = BTreeSet::<String>::new();
        let mut bisac_combinations = 0_u64;
        let mut bisac_books = 0_u64;
        let mut unmatched = BTreeMap::<String, (u64, Vec<Vec<String>>)>::new();
        let mut current_book = None::<(String, String, String)>;
        let mut subjects = Vec::<PublicationSubject>::new();
        let mut audit_book = |subjects: &[PublicationSubject]| {
            if subjects.is_empty() {
                return;
            }
            let bisac_assignments = subject_projection::match_bisac_subjects(&subjects);
            let coded_assignments = bisac_assignments.iter().filter(|assignment| subject_projection::bisac_code_for_path(assignment.node_path()).is_some()).collect::<Vec<_>>();
            if !coded_assignments.is_empty() {
                bisac_books = bisac_books.saturating_add(1);
            }
            bisac_combinations = bisac_combinations.saturating_add(u64::try_from(coded_assignments.len()).unwrap_or(u64::MAX));
            let matched_positions = coded_assignments.iter().flat_map(|assignment| assignment.evidence_positions().iter().copied()).collect::<BTreeSet<_>>();
            let context = subjects.iter().map(|subject| format!("{} [source={}; authority={}; code={}]", subject.name(), subject.source(), subject.authority().unwrap_or(""), subject.code().unwrap_or(""))).collect::<Vec<_>>();
            for (position, subject) in subjects.iter().enumerate() {
                if !subject.source().trim().eq_ignore_ascii_case("dc:subject") {
                    continue;
                }
                dc_subject_occurrences = dc_subject_occurrences.saturating_add(1);
                let normalized = subject.name().split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
                dc_headings.insert(normalized.clone());
                if matched_positions.contains(&position) {
                    matched_dc_subject_occurrences = matched_dc_subject_occurrences.saturating_add(1);
                } else {
                    unmatched_headings.insert(normalized);
                    let entry = unmatched.entry(subject.name().to_owned()).or_default();
                    entry.0 = entry.0.saturating_add(1);
                    if entry.1.len() < 3 {
                        entry.1.push(context.clone());
                    }
                }
            }
        };

        for row in rows {
            let (source_id, edition_id, work_id, name, source, authority, code) = row?;
            let key = (source_id, edition_id, work_id);
            if current_book.as_ref().is_some_and(|current| current != &key) {
                audit_book(&subjects);
                subjects.clear();
            }
            current_book = Some(key);
            subjects.push(PublicationSubject::new(None, name, source, authority, code).map_err(|error| CatalogCacheError::new(error.to_string()))?);
        }
        audit_book(&subjects);
        drop(audit_book);

        let mut unmatched_dc_subjects = unmatched.into_iter().map(|(subject, (occurrences, examples))| CatalogUnmatchedDcSubject { subject, occurrences, examples }).collect::<Vec<_>>();
        unmatched_dc_subjects.sort_by(|left, right| right.occurrences.cmp(&left.occurrences).then_with(|| left.subject.cmp(&right.subject)));
        unmatched_dc_subjects.truncate(100);

        Ok(CatalogSubjectProjectionAudit {
            dc_subject_occurrences,
            dc_subject_occurrences_with_bisac_assignment: matched_dc_subject_occurrences,
            dc_subject_occurrences_without_bisac_assignment: dc_subject_occurrences.saturating_sub(matched_dc_subject_occurrences),
            distinct_dc_subject_headings: u64::try_from(dc_headings.len()).unwrap_or(u64::MAX),
            distinct_dc_subject_headings_without_bisac_assignment: u64::try_from(unmatched_headings.len()).unwrap_or(u64::MAX),
            bisac_book_subject_combinations: bisac_combinations,
            bisac_books,
            unmatched_dc_subjects,
        })
    }
}

pub(crate) fn replace_publication_subjects(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, subjects: &[PublicationSubject]) -> Result<(), CatalogCacheError> {
    let assignments = prepare_catalog_subject_assignments(subjects);
    replace_publication_subjects_prepared(transaction, source_id, generation, identity, subjects, &assignments)
}

pub(crate) fn replace_publication_subjects_prepared(
    transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, subjects: &[PublicationSubject], assignments: &[CatalogSubjectAssignment],
) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_publication_subject_node WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    transaction.execute("DELETE FROM catalog_publication_subject_assignment WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    transaction.execute("DELETE FROM catalog_publication_subject WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    for (position, subject) in subjects.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_publication_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                source_id.as_str(),
                generation,
                identity.edition_id,
                work_id,
                i64::try_from(position).map_err(|_| CatalogCacheError::new("subject position overflow"))?,
                subject.name(),
                subject.source(),
                subject.authority(),
                subject.code()
            ],
        )?;
    }
    insert_prepared_subject_assignments(transaction, source_id.as_str(), generation, &identity.edition_id, work_id, assignments)?;
    Ok(())
}

/// Replaces only catalogue-feed subject evidence. EPUB and external
/// enrichment evidence lives at positions above `ENRICHED_SUBJECT_POSITION_BASE`
/// and remains intact across immutable catalogue recrawls.
pub(crate) fn replace_feed_publication_subjects_prepared(
    transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, subjects: &[PublicationSubject], feed_assignments: &[CatalogSubjectAssignment],
) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "DELETE FROM catalog_publication_subject WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND position<?5",
        params![source_id.as_str(), generation, identity.edition_id, work_id, ENRICHED_SUBJECT_POSITION_BASE],
    )?;
    for (position, subject) in subjects.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_publication_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                source_id.as_str(),
                generation,
                identity.edition_id,
                work_id,
                i64::try_from(position).map_err(|_| CatalogCacheError::new("feed subject position overflow"))?,
                subject.name(),
                subject.source(),
                subject.authority(),
                subject.code()
            ],
        )?;
    }
    rebuild_subject_assignments(transaction, source_id, generation, identity, Some(feed_assignments))
}

pub(crate) fn rebuild_subject_assignments(
    transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: i64, identity: &CatalogPublicationIdentity, feed_assignments: Option<&[CatalogSubjectAssignment]>,
) -> Result<(), CatalogCacheError> {
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_publication_subject_node WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    transaction.execute("DELETE FROM catalog_publication_subject_assignment WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    let enriched = {
        let mut statement = transaction.prepare(
            "SELECT name,source,authority,code FROM catalog_publication_subject
             WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND position>=?5 ORDER BY position",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, identity.edition_id, work_id, ENRICHED_SUBJECT_POSITION_BASE], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))
        })?;
        let mut subjects = Vec::new();
        for row in rows {
            let (name, source, authority, code) = row?;
            subjects.push(PublicationSubject::new(None, name, source, authority, code).map_err(|error| CatalogCacheError::new(error.to_string()))?);
        }
        subjects
    };
    if enriched.is_empty() {
        if let Some(assignments) = feed_assignments {
            return insert_prepared_subject_assignments(transaction, source_id.as_str(), generation, &identity.edition_id, work_id, assignments);
        }
    }
    let mut all_subjects = {
        let mut statement = transaction.prepare(
            "SELECT name,source,authority,code FROM catalog_publication_subject
             WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND position<?5 ORDER BY position",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, identity.edition_id, work_id, ENRICHED_SUBJECT_POSITION_BASE], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))
        })?;
        let mut subjects = Vec::new();
        for row in rows {
            let (name, source, authority, code) = row?;
            subjects.push(PublicationSubject::new(None, name, source, authority, code).map_err(|error| CatalogCacheError::new(error.to_string()))?);
        }
        subjects
    };
    all_subjects.extend(enriched);
    let assignments = prepare_catalog_subject_assignments(&all_subjects);
    insert_prepared_subject_assignments(transaction, source_id.as_str(), generation, &identity.edition_id, work_id, &assignments)
}

pub(crate) fn replace_publication_toc(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, toc: &[library_model::PublicationTocEntry]) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_publication_toc WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    let mut titles = Vec::new();
    flatten_toc_titles(toc, &mut titles);
    for (position, title) in titles.into_iter().enumerate() {
        let normalized = normalize_search_text(title);
        if normalized.is_empty() {
            continue;
        }
        transaction.execute(
            "INSERT INTO catalog_publication_toc(source_id,generation,edition_id,work_id,position,normalized_title) VALUES(?1,?2,?3,?4,?5,?6)",
            params![source_id.as_str(), generation, identity.edition_id, work_id, i64::try_from(position).map_err(|_| CatalogCacheError::new("TOC position overflow"))?, normalized],
        )?;
    }
    Ok(())
}

pub(crate) fn flatten_toc_titles<'a>(entries: &'a [library_model::PublicationTocEntry], output: &mut Vec<&'a str>) {
    for entry in entries {
        output.push(&entry.title);
        flatten_toc_titles(&entry.children, output);
    }
}

pub(crate) fn insert_prepared_subject_assignments(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, assignments: &[CatalogSubjectAssignment]) -> Result<(), CatalogCacheError> {
    for assignment in assignments {
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_publication_subject_assignment(source_id,generation,edition_id,work_id,system_id,subject_path,matcher_version) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![source_id, generation, edition_id, work_id, assignment.system_id, assignment.subject_path, assignment.matcher_version],
        )?;
        insert_subject_browse_memberships(transaction, source_id, generation, edition_id, work_id, &assignment.system_id, &assignment.subject_path)?;
    }
    Ok(())
}

pub(crate) fn insert_subject_browse_memberships(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, system_id: &str, subject_path: &str) -> Result<(), CatalogCacheError> {
    if system_id != subject_projection::UNIFIED_SYSTEM_ID {
        return Ok(());
    }
    let titles = subject_path.split(" / ").collect::<Vec<_>>();
    if titles.is_empty() || titles.iter().any(|title| title.is_empty()) {
        return Err(CatalogCacheError::new("catalog subject assignment contains an empty hierarchy node"));
    }

    let mut parent_path = String::new();
    let mut node_path = String::new();
    for (position, title) in titles.iter().enumerate() {
        if !node_path.is_empty() {
            node_path.push_str(" / ");
        }
        node_path.push_str(title);
        transaction.execute(
            "INSERT INTO catalog_subject_node(system_id,node_path,parent_path,title) VALUES(?1,?2,?3,?4)
             ON CONFLICT(system_id,node_path) DO NOTHING",
            params![system_id, node_path, parent_path, title],
        )?;
        let exact_assignment = i64::from(position + 1 == titles.len());
        transaction.execute(
            "INSERT INTO catalog_publication_subject_node(source_id,generation,edition_id,work_id,system_id,node_path,exact_assignment)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(source_id,generation,edition_id,work_id,system_id,node_path)
             DO UPDATE SET exact_assignment=MAX(catalog_publication_subject_node.exact_assignment,excluded.exact_assignment)",
            params![source_id, generation, edition_id, work_id, system_id, node_path, exact_assignment],
        )?;
        parent_path.clone_from(&node_path);
    }
    Ok(())
}

pub(crate) fn refresh_unified_subject_assignments(connection: &mut Connection) -> Result<(), CatalogCacheError> {
    const BATCH_SIZE: i64 = 256;
    let stored_version = connection.query_row("SELECT unified_matcher_version FROM catalog_subject_projection_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).optional()?;
    if stored_version == Some(subject_projection::UNIFIED_MATCHER_VERSION) {
        return Ok(());
    }

    {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM catalog_subject_browse_index_metadata", [])?;
        transaction.execute("DELETE FROM catalog_publication_subject_node WHERE system_id=?1", [subject_projection::UNIFIED_SYSTEM_ID])?;
        transaction.execute("DELETE FROM catalog_publication_subject_assignment WHERE system_id=?1", [subject_projection::UNIFIED_SYSTEM_ID])?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
    }
    let mut cursor: Option<(String, i64, String, String)> = None;
    loop {
        let keys = {
            let (sql, parameters): (&str, Vec<rusqlite::types::Value>) = match &cursor {
                Some((source_id, generation, edition_id, work_id)) => (
                    "SELECT DISTINCT source_id,generation,edition_id,work_id
                       FROM catalog_publication_subject
                      WHERE (source_id,generation,edition_id,work_id) > (?1,?2,?3,?4)
                      ORDER BY source_id,generation,edition_id,work_id LIMIT ?5",
                    vec![source_id.clone().into(), (*generation).into(), edition_id.clone().into(), work_id.clone().into(), BATCH_SIZE.into()],
                ),
                None => (
                    "SELECT DISTINCT source_id,generation,edition_id,work_id
                       FROM catalog_publication_subject
                      ORDER BY source_id,generation,edition_id,work_id LIMIT ?1",
                    vec![BATCH_SIZE.into()],
                ),
            };
            let mut statement = connection.prepare(sql)?;
            let keys = statement.query_map(rusqlite::params_from_iter(parameters), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
            keys
        };
        if keys.is_empty() {
            break;
        }
        cursor = keys.last().cloned();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (source_id, generation, edition_id, work_id) in keys {
            let subjects = {
                let mut statement = transaction.prepare(
                    "SELECT name,source,authority,code FROM catalog_publication_subject
                      WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 ORDER BY position",
                )?;
                let rows = statement.query_map(params![source_id, generation, edition_id, work_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?)))?;
                let mut subjects = Vec::new();
                for row in rows {
                    let (name, source, authority, code) = row?;
                    subjects.push(PublicationSubject::new(None, name, source, authority, code).map_err(|error| CatalogCacheError::new(error.to_string()))?);
                }
                subjects
            };
            let assignments = subject_projection::project_unified_subjects(&subjects);
            insert_prepared_subject_assignments(&transaction, &source_id, generation, &edition_id, &work_id, &assignments)?;
        }
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
    }
    connection.execute(
        "INSERT INTO catalog_subject_projection_metadata(singleton,unified_matcher_version) VALUES(1,?1)
         ON CONFLICT(singleton) DO UPDATE SET unified_matcher_version=excluded.unified_matcher_version",
        [subject_projection::UNIFIED_MATCHER_VERSION],
    )?;
    Ok(())
}
