use super::*;
use app_contract::catalog::{CatalogPublicationIdentity, CatalogPublicationKey, CatalogSourceId};

pub fn record_catalog_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, publication: &CatalogPublicationIdentity, content_hash: &ContentHash) -> rusqlite::Result<()> {
    transaction.upsert_catalog_publication_book(source_id.as_str(), &publication.edition_id, publication.work_id.as_deref().unwrap_or_default(), content_hash.as_str())?;
    Ok(())
}

pub fn owned_catalog_publications(connection: &Connection, source_id: &CatalogSourceId, identities: &[CatalogPublicationIdentity]) -> rusqlite::Result<Vec<CatalogPublicationIdentity>> {
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

pub fn all_owned_catalog_publications(connection: &Connection) -> rusqlite::Result<Vec<CatalogPublicationKey>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT publication.source_id,publication.edition_id,publication.work_id
         FROM catalog_publication_book publication
         JOIN book ON book.row_id=publication.book_row_id
         WHERE book.deleted_at IS NULL
         ORDER BY publication.source_id,publication.edition_id,publication.work_id",
    )?;
    statement
        .query_map([], |row| {
            let raw_source_id: String = row.get(0)?;
            let source_id = CatalogSourceId::parse(&raw_source_id).map_err(|error| invalid_text_column(0, error))?;
            let work_id: String = row.get(2)?;
            Ok(CatalogPublicationKey::new(source_id, CatalogPublicationIdentity { edition_id: row.get(1)?, work_id: (!work_id.is_empty()).then_some(work_id) }))
        })?
        .collect()
}
