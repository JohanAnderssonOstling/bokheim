use crate::catalog::queries::CatalogSql;
use crate::storage_values::invalid_text_column;
use catalog_model::{CatalogPublicationIdentity, CatalogSourceId};
use rusqlite::Connection;
use rusqlite::Transaction;
use sync_common::ContentHash;

pub fn record_catalog_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, publication: &CatalogPublicationIdentity, content_hash: &ContentHash) -> rusqlite::Result<()> {
    transaction.upsert_catalog_publication_book(source_id.as_str(), &publication.edition_id, publication.work_id.as_deref().unwrap_or_default(), content_hash.as_str())?;
    Ok(())
}

pub fn owned_catalog_publications(connection: &Connection, source_id: &CatalogSourceId, identities: &[CatalogPublicationIdentity]) -> rusqlite::Result<Vec<CatalogPublicationIdentity>> {
    let mut statement = connection.prepare_cached(include_str!("sql/catalog/owned_catalog_publications_select.sql"))?;

    let mut owned = Vec::new();
    for identity in identities {
        let exact = statement.query_row(rusqlite::params![source_id.as_str(), identity.edition_id, identity.work_id.as_deref().unwrap_or_default()], |row| row.get::<_, bool>(0))?;
        if exact {
            owned.push(identity.clone());
        }
    }
    Ok(owned)
}

pub fn all_owned_catalog_publications(connection: &Connection) -> rusqlite::Result<Vec<(CatalogSourceId, CatalogPublicationIdentity)>> {
    let mut statement = connection.prepare(include_str!("sql/catalog/all_owned_catalog_publications_select.sql"))?;
    statement
        .query_map([], |row| {
            let raw_source_id: String = row.get(0)?;
            let source_id = CatalogSourceId::parse(&raw_source_id).map_err(|error| invalid_text_column(0, error))?;
            let work_id: String = row.get(2)?;
            Ok((source_id, CatalogPublicationIdentity { edition_id: row.get(1)?, work_id: (!work_id.is_empty()).then_some(work_id) }))
        })?
        .collect()
}

pub(crate) mod queries;
