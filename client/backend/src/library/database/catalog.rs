use super::*;
use crate::catalog::{CatalogPublicationIdentity, CatalogSourceId};

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
