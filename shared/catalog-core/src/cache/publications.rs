use super::{
    normalize_search_text, params, prepare_catalog_subject_assignments, publication_key, replace_feed_publication_subjects_prepared, storage_i64, BTreeSet, CatalogBookFormat, CatalogCacheError, CatalogPublication, CatalogPublicationAction,
    CatalogSourceId, Transaction,
};
use publication_model::PublicationSubject;

pub struct PublicationInventory {
    pub book_format: Option<&'static str>,
    pub importable: bool,
    pub has_upstream_cover: bool,
}

pub struct PublicationExtraction<'a> {
    pub acquisition_url: &'a str,
    pub book_format: CatalogBookFormat,
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
        CatalogPublicationAction::External { .. } => None,
    });
    PublicationInventory { book_format, importable: book_format.is_some(), has_upstream_cover: publication.image_url.as_deref().is_some_and(|url| url.starts_with("https://") || url.starts_with("http://")) }
}

pub(crate) fn publication_formats(publication: &CatalogPublication) -> BTreeSet<&'static str> {
    publication
        .actions
        .iter()
        .filter_map(|action| match action {
            CatalogPublicationAction::Import { request } => Some(book_format_storage(request.book_format)),
            CatalogPublicationAction::External { .. } => None,
        })
        .collect()
}

pub fn publication_entry_id(transaction: &Transaction<'_>, source_id: &CatalogSourceId, publication: &CatalogPublication, payload_hash: &str) -> Result<i64, CatalogCacheError> {
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

pub fn replace_publication_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication) -> Result<(), CatalogCacheError> {
    let (edition_id, work_id) = publication_key(publication);
    let generation = storage_i64(generation, "generation")?;
    transaction.execute("DELETE FROM catalog_publication_author WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for (position, author) in publication.authors.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_publication_author(source_id, generation, edition_id, work_id, position, normalized_name) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![source_id.as_str(), generation, edition_id, work_id, i64::try_from(position).map_err(|_| CatalogCacheError::new("author position overflow"))?, normalize_search_text(&author.name)],
        )?;
    }
    transaction.execute("DELETE FROM catalog_publication_language WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for language in &publication.languages {
        transaction
            .execute("INSERT OR IGNORE INTO catalog_publication_language(source_id, generation, edition_id, work_id, language) VALUES(?1, ?2, ?3, ?4, ?5)", params![source_id.as_str(), generation, edition_id, work_id, language.as_str()])?;
    }
    transaction.execute("DELETE FROM catalog_publication_format WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for format in publication_formats(publication) {
        transaction.execute("INSERT INTO catalog_publication_format(source_id, generation, edition_id, work_id, book_format) VALUES(?1, ?2, ?3, ?4, ?5)", params![source_id.as_str(), generation, edition_id, work_id, format])?;
    }
    let subjects = publication.metadata.subjects.clone();
    let assignments = prepare_catalog_subject_assignments(&subjects);
    replace_feed_publication_subjects_prepared(transaction, source_id, generation as u64, &publication.identity, &subjects, &assignments)
}
