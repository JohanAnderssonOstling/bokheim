use super::{normalize_search_text, params, prepare_catalog_subject_assignments, storage_i64, CatalogCacheError, CatalogPublicationIdentity, CatalogSourceId, CatalogSubjectAssignment, Transaction};
use publication_model::PublicationSubject;

#[doc(hidden)]
pub const ENRICHED_SUBJECT_POSITION_BASE: i64 = 1_000_000;

pub fn replace_publication_subjects(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, subjects: &[PublicationSubject]) -> Result<(), CatalogCacheError> {
    let assignments = prepare_catalog_subject_assignments(subjects);
    replace_publication_subjects_prepared(transaction, source_id, generation, identity, subjects, &assignments)
}

pub fn replace_publication_subjects_prepared(
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
pub fn replace_feed_publication_subjects_prepared(
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

#[doc(hidden)]
pub fn rebuild_subject_assignments(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: i64, identity: &CatalogPublicationIdentity, feed_assignments: Option<&[CatalogSubjectAssignment]>) -> Result<(), CatalogCacheError> {
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

pub fn replace_publication_toc(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, toc: &[publication_model::PublicationTocEntry]) -> Result<(), CatalogCacheError> {
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

pub(crate) fn flatten_toc_titles<'a>(entries: &'a [publication_model::PublicationTocEntry], output: &mut Vec<&'a str>) {
    for entry in entries {
        output.push(&entry.title);
        flatten_toc_titles(&entry.children, output);
    }
}

pub fn insert_prepared_subject_assignments(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, assignments: &[CatalogSubjectAssignment]) -> Result<(), CatalogCacheError> {
    for assignment in assignments {
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_publication_subject_assignment(source_id,generation,edition_id,work_id,system_id,subject_path,matcher_version) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![source_id, generation, edition_id, work_id, assignment.system_id, assignment.subject_path, assignment.matcher_version],
        )?;
        insert_subject_browse_memberships(transaction, source_id, generation, edition_id, work_id, &assignment.system_id, &assignment.subject_path)?;
    }
    Ok(())
}

pub fn insert_subject_browse_memberships(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, system_id: &str, subject_path: &str) -> Result<(), CatalogCacheError> {
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
