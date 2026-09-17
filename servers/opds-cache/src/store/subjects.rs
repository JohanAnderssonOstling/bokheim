use super::{normalize_search_text, params, prepare_catalog_subject_assignments, storage_i64, CatalogBookIdentity, CatalogSourceId, CatalogStoreError, CatalogSubjectAssignment, Transaction};
use book_model::BookSubject;

#[doc(hidden)]
pub const ENRICHED_SUBJECT_POSITION_BASE: i64 = 1_000_000;

pub fn replace_book_subjects_prepared(
    transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogBookIdentity, subjects: &[BookSubject], assignments: &[CatalogSubjectAssignment],
) -> Result<(), CatalogStoreError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_book_subject_node WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    transaction.execute("DELETE FROM catalog_book_subject_assignment WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    transaction.execute("DELETE FROM catalog_book_subject WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    for (position, subject) in subjects.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_book_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                source_id.as_str(),
                generation,
                identity.edition_id,
                work_id,
                i64::try_from(position).map_err(|_| CatalogStoreError::new("subject position overflow"))?,
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
pub fn replace_feed_book_subjects_prepared(
    transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogBookIdentity, subjects: &[BookSubject], feed_assignments: &[CatalogSubjectAssignment],
) -> Result<(), CatalogStoreError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "DELETE FROM catalog_book_subject WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND position<?5",
        params![source_id.as_str(), generation, identity.edition_id, work_id, ENRICHED_SUBJECT_POSITION_BASE],
    )?;
    for (position, subject) in subjects.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_book_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                source_id.as_str(),
                generation,
                identity.edition_id,
                work_id,
                i64::try_from(position).map_err(|_| CatalogStoreError::new("feed subject position overflow"))?,
                subject.name(),
                subject.source(),
                subject.authority(),
                subject.code()
            ],
        )?;
    }
    rebuild_subject_assignments(transaction, source_id, generation, identity, Some(feed_assignments))
}

#[doc(hidden)]
pub fn rebuild_subject_assignments(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: i64, identity: &CatalogBookIdentity, feed_assignments: Option<&[CatalogSubjectAssignment]>) -> Result<(), CatalogStoreError> {
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_book_subject_node WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    transaction.execute("DELETE FROM catalog_book_subject_assignment WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    let enriched = {
        let mut statement = transaction.prepare(
            "SELECT name,source,authority,code FROM catalog_book_subject
             WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND position>=?5 ORDER BY position",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, identity.edition_id, work_id, ENRICHED_SUBJECT_POSITION_BASE], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))
        })?;
        let mut subjects = Vec::new();
        for row in rows {
            let (name, source, authority, code) = row?;
            subjects.push(BookSubject::new(None, name, source, authority, code).map_err(|error| CatalogStoreError::new(error.to_string()))?);
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
            "SELECT name,source,authority,code FROM catalog_book_subject
             WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND position<?5 ORDER BY position",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, identity.edition_id, work_id, ENRICHED_SUBJECT_POSITION_BASE], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))
        })?;
        let mut subjects = Vec::new();
        for row in rows {
            let (name, source, authority, code) = row?;
            subjects.push(BookSubject::new(None, name, source, authority, code).map_err(|error| CatalogStoreError::new(error.to_string()))?);
        }
        subjects
    };
    all_subjects.extend(enriched);
    let assignments = prepare_catalog_subject_assignments(&all_subjects);
    insert_prepared_subject_assignments(transaction, source_id.as_str(), generation, &identity.edition_id, work_id, &assignments)
}

pub fn replace_book_toc(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogBookIdentity, toc: &[book_model::BookTocEntry]) -> Result<(), CatalogStoreError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_book_toc WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    let mut titles = Vec::new();
    flatten_toc_titles(toc, &mut titles);
    for (position, title) in titles.into_iter().enumerate() {
        let normalized = normalize_search_text(title);
        if normalized.is_empty() {
            continue;
        }
        transaction.execute(
            "INSERT INTO catalog_book_toc(source_id,generation,edition_id,work_id,position,normalized_title) VALUES(?1,?2,?3,?4,?5,?6)",
            params![source_id.as_str(), generation, identity.edition_id, work_id, i64::try_from(position).map_err(|_| CatalogStoreError::new("TOC position overflow"))?, normalized],
        )?;
    }
    Ok(())
}

pub(crate) fn flatten_toc_titles<'a>(entries: &'a [book_model::BookTocEntry], output: &mut Vec<&'a str>) {
    for entry in entries {
        output.push(&entry.title);
        flatten_toc_titles(&entry.children, output);
    }
}

pub fn insert_prepared_subject_assignments(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, assignments: &[CatalogSubjectAssignment]) -> Result<(), CatalogStoreError> {
    for assignment in assignments {
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_book_subject_assignment(source_id,generation,edition_id,work_id,system_id,subject_path,matcher_version) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![source_id, generation, edition_id, work_id, assignment.system_id, assignment.subject_path, assignment.matcher_version],
        )?;
        insert_subject_browse_memberships(transaction, source_id, generation, edition_id, work_id, &assignment.system_id, &assignment.subject_path)?;
        for source in &assignment.lcc_evidence {
            let ordinal = i64::try_from(source.position).map_err(|_| CatalogStoreError::new("LCC evidence position overflow"))?;
            let (position, original): (i64, String) = transaction.query_row(
                "SELECT position,COALESCE(code,name) FROM catalog_book_subject
                 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4
                 ORDER BY position LIMIT 1 OFFSET ?5",
                params![source_id, generation, edition_id, work_id, ordinal],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if original != source.original {
                return Err(CatalogStoreError::new("LCC evidence does not match its stored source"));
            }
            let evidence = source
                .evidence
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "method": match e.method {
                            subject_projection::LccMatchMethod::Exact => "exact",
                            subject_projection::LccMatchMethod::Normalized => "normalized",
                            subject_projection::LccMatchMethod::RecoveredComponent => "recovered_component",
                            subject_projection::LccMatchMethod::BroadFallback => "broad_fallback",
                        },
                        "code": e.code,
                        "supporting_concept_ids": e.supporting_concept_ids,
                        "interpretations": e.interpretations,
                    })
                })
                .collect::<Vec<_>>();
            let details = serde_json::json!({
                "matcher_version": assignment.matcher_version,
                "evidence": evidence,
                "unresolved_fragments": source.unresolved_fragments,
            })
            .to_string();
            transaction.execute(
                "INSERT INTO catalog_subject_lcc_evidence
                 (source_id,generation,edition_id,work_id,system_id,subject_path,source_position,concept_id,original,details_json)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(source_id,generation,edition_id,work_id,system_id,subject_path,source_position,concept_id)
                 DO UPDATE SET original=excluded.original,details_json=excluded.details_json,format_version=1",
                params![source_id, generation, edition_id, work_id, assignment.system_id, assignment.subject_path, position, source.concept_id, source.original, details],
            )?;
        }
    }
    Ok(())
}

pub fn insert_subject_browse_memberships(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, system_id: &str, subject_path: &str) -> Result<(), CatalogStoreError> {
    if system_id != subject_projection::UNIFIED_SYSTEM_ID {
        return Ok(());
    }
    let titles = subject_path.split(" / ").collect::<Vec<_>>();
    if titles.is_empty() || titles.iter().any(|title| title.is_empty()) {
        return Err(CatalogStoreError::new("catalog subject assignment contains an empty hierarchy node"));
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
            "INSERT INTO catalog_book_subject_node(source_id,generation,edition_id,work_id,system_id,node_path,exact_assignment)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(source_id,generation,edition_id,work_id,system_id,node_path)
             DO UPDATE SET exact_assignment=MAX(catalog_book_subject_node.exact_assignment,excluded.exact_assignment)",
            params![source_id, generation, edition_id, work_id, system_id, node_path, exact_assignment],
        )?;
        parent_path.clone_from(&node_path);
    }
    Ok(())
}
