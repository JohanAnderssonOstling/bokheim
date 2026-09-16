use super::*;
use publication_model::PublicationSubject;

pub fn refresh_unified_subject_assignments(connection: &mut Connection) -> Result<(), CatalogStoreError> {
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
                    "SELECT DISTINCT subject.source_id,subject.generation,subject.edition_id,subject.work_id
                       FROM catalog_publication_subject subject
                       JOIN catalog_source active_source USING(source_id)
                       JOIN catalog_publication active_publication
                         ON active_publication.source_id=subject.source_id
                        AND active_publication.generation=subject.generation
                        AND active_publication.edition_id=subject.edition_id
                        AND active_publication.work_id=subject.work_id
                        AND active_publication.importable=1
                      WHERE active_source.active_generation=subject.generation
                        AND (subject.source_id,subject.generation,subject.edition_id,subject.work_id) > (?1,?2,?3,?4)
                      ORDER BY subject.source_id,subject.generation,subject.edition_id,subject.work_id LIMIT ?5",
                    vec![source_id.clone().into(), (*generation).into(), edition_id.clone().into(), work_id.clone().into(), BATCH_SIZE.into()],
                ),
                None => (
                    "SELECT DISTINCT subject.source_id,subject.generation,subject.edition_id,subject.work_id
                       FROM catalog_publication_subject subject
                       JOIN catalog_source active_source USING(source_id)
                       JOIN catalog_publication active_publication
                         ON active_publication.source_id=subject.source_id
                        AND active_publication.generation=subject.generation
                        AND active_publication.edition_id=subject.edition_id
                        AND active_publication.work_id=subject.work_id
                        AND active_publication.importable=1
                      WHERE active_source.active_generation=subject.generation
                      ORDER BY subject.source_id,subject.generation,subject.edition_id,subject.work_id LIMIT ?1",
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
                    subjects.push(PublicationSubject::new(None, name, source, authority, code).map_err(|error| CatalogStoreError::new(error.to_string()))?);
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


impl CatalogStore {
    /// Builds the external-content FTS indexes once, after the HTTP listener is
    /// available. Their database triggers keep them current across later
    /// crawls, enrichment, and rollback to an older server binary.
    pub fn refresh_search_index(&self) -> Result<bool, CatalogStoreError> {
        let mut connection = self.connection()?;
        let current = connection.query_row("SELECT index_version=?1 AND ready=1 FROM catalog_search_index_metadata WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION], |row| row.get::<_, bool>(0))?;
        if current {
            return Ok(false);
        }
        {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute("UPDATE catalog_search_index_metadata SET index_version=?1,ready=0 WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION])?;
            transaction.commit()?;
        }
        for statement in [
            "INSERT INTO catalog_publication_title_fts(catalog_publication_title_fts) VALUES('rebuild')",
            "INSERT INTO catalog_publication_author_fts(catalog_publication_author_fts) VALUES('rebuild')",
            "INSERT INTO catalog_publication_toc_fts(catalog_publication_toc_fts) VALUES('rebuild')",
        ] {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(statement, [])?;
            transaction.commit()?;
        }
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("UPDATE catalog_search_index_metadata SET ready=1 WHERE singleton=1", [])?;
        transaction.commit()?;
        Ok(true)
    }


    /// Rebuilds persisted unified taxonomy assignments in bounded batches.
    /// Servers should run this away from their health-critical startup path.
    pub fn refresh_unified_subject_assignments(&self) -> Result<(), CatalogStoreError> {
        let mut connection = self.connection()?;
        refresh_unified_subject_assignments(&mut connection)
    }

    /// Materializes every browsable assignment as one deduplicated membership
    /// per ancestor node. The rebuild publishes an explicit not-ready state
    /// and commits bounded batches so readers fall back to authoritative
    /// assignments instead of observing a partially populated tree.
    pub fn refresh_subject_browse_index(&self) -> Result<bool, CatalogStoreError> {
        let mut connection = self.connection()?;
        let current = connection.query_row("SELECT index_version=?1 FROM catalog_subject_browse_index_metadata WHERE singleton=1", [CATALOG_SUBJECT_BROWSE_INDEX_VERSION], |row| row.get::<_, bool>(0)).optional()?.unwrap_or(false);
        if current {
            return Ok(false);
        }

        const BATCH_SIZE: i64 = 512;
        // Publish an explicit not-ready state before clearing the old
        // materialization. Readers then use the authoritative assignment
        // tables while the rebuild proceeds, instead of observing a partial
        // browse index.
        {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute("UPDATE catalog_subject_browse_index_metadata SET index_version=0 WHERE singleton=1", [])?;
            transaction.execute("DELETE FROM catalog_publication_subject_node", [])?;
            transaction.execute("DELETE FROM catalog_subject_node", [])?;
            transaction.commit()?;
        }
        let mut rowid = 0_i64;
        loop {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let assignments = {
                let mut statement = transaction.prepare(
                    "SELECT assignment.rowid,assignment.source_id,assignment.generation,assignment.edition_id,assignment.work_id,assignment.system_id,assignment.subject_path
                       FROM catalog_publication_subject_assignment assignment
                       JOIN catalog_source active_source ON active_source.source_id=assignment.source_id
                       JOIN catalog_publication active_publication
                         ON active_publication.source_id=assignment.source_id
                        AND active_publication.generation=assignment.generation
                        AND active_publication.edition_id=assignment.edition_id
                        AND active_publication.work_id=assignment.work_id
                        AND active_publication.importable=1
                      WHERE assignment.rowid>?1 AND assignment.system_id=?2
                        AND active_source.active_generation=assignment.generation
                      ORDER BY assignment.rowid LIMIT ?3",
                )?;
                let rows = statement
                    .query_map(params![rowid, subject_projection::UNIFIED_SYSTEM_ID, BATCH_SIZE], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            if assignments.is_empty() {
                transaction.rollback()?;
                break;
            }
            rowid = assignments.last().expect("non-empty assignment batch").0;
            for (_, source_id, generation, edition_id, work_id, system_id, subject_path) in assignments {
                insert_subject_browse_memberships(&transaction, &source_id, generation, &edition_id, &work_id, &system_id, &subject_path)?;
            }
            transaction.commit()?;
        }
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
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
    pub fn subject_projection_audit(&self) -> Result<CatalogSubjectProjectionAudit, CatalogStoreError> {
        let connection = self.connection()?;
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
            subjects.push(PublicationSubject::new(None, name, source, authority, code).map_err(|error| CatalogStoreError::new(error.to_string()))?);
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
