use super::{params, CatalogBookFormat, CatalogPublication, CatalogPublicationAction, CatalogSourceId, CatalogStoreError, Transaction};

pub struct PublicationInventory {
    pub book_format: Option<&'static str>,
    pub importable: bool,
    pub has_upstream_cover: bool,
}

pub struct PublicationExtraction<'a> {
    pub acquisition_url: &'a str,
    pub book_format: CatalogBookFormat,
}

pub fn publication_key(publication: &CatalogPublication) -> (&str, &str) {
    (&publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or(""))
}

pub fn publication_extraction(publication: &CatalogPublication) -> Option<PublicationExtraction<'_>> {
    publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } if matches!(request.book_format, CatalogBookFormat::Epub | CatalogBookFormat::Pdf | CatalogBookFormat::Mobi) => {
            Some(PublicationExtraction { acquisition_url: &request.download_url, book_format: request.book_format })
        }
        _ => None,
    })
}

pub fn publication_inventory(publication: &CatalogPublication) -> PublicationInventory {
    let book_format = publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } => Some(book_format_storage(request.book_format)),
    });
    PublicationInventory { book_format, importable: book_format.is_some(), has_upstream_cover: publication.image_url.as_deref().is_some_and(|url| url.starts_with("https://") || url.starts_with("http://")) }
}

pub fn publication_entry_id(transaction: &Transaction<'_>, source_id: &CatalogSourceId, publication: &CatalogPublication, payload_hash: &str) -> Result<i64, CatalogStoreError> {
    let (edition_id, work_id) = publication_key(publication);
    transaction.execute("INSERT OR IGNORE INTO catalog_publication_entry(source_id,edition_id,work_id,payload_hash) VALUES(?1,?2,?3,?4)", params![source_id.as_str(), edition_id, work_id, payload_hash])?;
    transaction
        .query_row("SELECT entry_id FROM catalog_publication_entry WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND payload_hash=?4", params![source_id.as_str(), edition_id, work_id, payload_hash], |row| row.get(0))
        .map_err(Into::into)
}

pub fn book_format_storage(format: CatalogBookFormat) -> &'static str {
    match format {
        CatalogBookFormat::Epub => "epub",
        CatalogBookFormat::Pdf => "pdf",
        CatalogBookFormat::Mobi => "mobi",
        CatalogBookFormat::M4b => "m4b",
    }
}

pub fn catalog_book_format_from_storage(value: &str) -> Option<CatalogBookFormat> {
    match value {
        "epub" => Some(CatalogBookFormat::Epub),
        "pdf" => Some(CatalogBookFormat::Pdf),
        "mobi" => Some(CatalogBookFormat::Mobi),
        "m4b" => Some(CatalogBookFormat::M4b),
        _ => None,
    }
}
