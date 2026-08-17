use super::*;
use crate::catalog::{CatalogPublicationIdentity, CatalogRecommendationRequest, CatalogSourceId, CatalogSubjectPreference};

pub(crate) fn record_catalog_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, publication: &CatalogPublicationIdentity, content_hash: &ContentHash) -> rusqlite::Result<()> {
    transaction.upsert_catalog_publication_book(source_id.as_str(), &publication.edition_id, publication.work_id.as_deref().unwrap_or_default(), content_hash.as_str())?;
    Ok(())
}

pub(crate) fn owned_catalog_publications(connection: &Connection, source_id: &CatalogSourceId, identities: &[CatalogPublicationIdentity]) -> rusqlite::Result<Vec<CatalogPublicationIdentity>> {
    let mut statement = connection.prepare_cached(
        "SELECT EXISTS(
             SELECT 1
             FROM catalog_publication_book AS publication
             JOIN book ON book.row_id=publication.book_row_id
             WHERE publication.source_id=?1
               AND publication.edition_id=?2
               AND publication.work_id=?3
               AND book.deleted_at IS NULL
         )",
    )?;

    let mut owned = Vec::new();
    for identity in identities {
        let exact = statement.query_row(rusqlite::params![source_id.as_str(), identity.edition_id, identity.work_id.as_deref().unwrap_or_default()], |row| row.get::<_, bool>(0))?;
        if exact {
            owned.push(identity.clone());
        }
    }
    Ok(owned)
}

pub(crate) fn catalog_recommendation_request(connection: &Connection, limit: u32) -> rusqlite::Result<CatalogRecommendationRequest> {
    let mut subject_statement = connection.prepare(
        "SELECT assignment.system_id,assignment.subject_path,
                SUM(CASE WHEN book.read_progress>0 THEN 3 ELSE 1 END) AS weight
         FROM book_subject_assignment assignment
         JOIN book ON book.row_id=assignment.book_row_id
         WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
           AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL)
         GROUP BY assignment.system_id,assignment.subject_path
         ORDER BY weight DESC,assignment.system_id,assignment.subject_path
         LIMIT 64",
    )?;
    let subjects = subject_statement
        .query_map([], |row| {
            let weight = row.get::<_, i64>(2)?;
            Ok(CatalogSubjectPreference { system_id: row.get(0)?, subject_path: row.get(1)?, weight: u32::try_from(weight).unwrap_or(u32::MAX) })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CatalogRecommendationRequest { subjects, supported_languages: Vec::new(), excluded_publications: Vec::new(), limit })
}

pub(crate) fn catalog_subject_recommendation_request(connection: &Connection, subject_path: &str, limit: u32) -> rusqlite::Result<CatalogRecommendationRequest> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT system_id
         FROM subject_node
         WHERE path=?1
           AND EXISTS(
             SELECT 1 FROM book_subject_assignment assignment
             WHERE assignment.system_id=subject_node.system_id
               AND (assignment.subject_path=?1 OR assignment.subject_path LIKE ?1 || ' / %')
           )
         ORDER BY system_id",
    )?;
    let subjects = statement.query_map([subject_path], |row| Ok(CatalogSubjectPreference { system_id: row.get(0)?, subject_path: subject_path.to_owned(), weight: 1 }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(CatalogRecommendationRequest { subjects, supported_languages: Vec::new(), excluded_publications: Vec::new(), limit })
}
