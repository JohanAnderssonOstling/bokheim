use super::{
    normalize_search_text, params, prepare_catalog_subject_assignments, project_provider_popularity, publication_key, rebuild_subject_assignments, replace_feed_publication_subjects_prepared, storage_i64, unique_page_publications, BTreeSet,
    CatalogBookFormat, CatalogCacheError, CatalogPageWrite, CatalogPublication, CatalogPublicationAction, CatalogPublicationIdentity, CatalogSourceId, CatalogSubjectAssignment, PreparedCatalogPageWrite, Transaction,
    ENRICHED_SUBJECT_POSITION_BASE,
};
use publication_model::PublicationSubject;

pub(crate) struct PublicationInventory {
    pub(crate) book_format: Option<&'static str>,
    pub(crate) importable: bool,
    pub(crate) has_upstream_cover: bool,
}

pub(crate) struct PublicationExtraction<'a> {
    pub(crate) acquisition_url: &'a str,
    pub(crate) book_format: CatalogBookFormat,
}

pub(crate) struct PreparedCatalogPublication {
    pub(crate) publication: CatalogPublication,
    pub(crate) payload: Vec<u8>,
    pub(crate) payload_hash: String,
    pub(crate) normalized_title: String,
    pub(crate) provider_popularity: u64,
    pub(crate) gutenberg_has_images: bool,
    pub(crate) normalized_authors: Vec<String>,
    pub(crate) languages: Vec<String>,
    pub(crate) formats: Vec<&'static str>,
    pub(crate) subjects: Vec<PublicationSubject>,
    pub(crate) subject_assignments: Vec<CatalogSubjectAssignment>,
    pub(crate) inventory: PublicationInventory,
}

impl PreparedCatalogPageWrite {
    pub(crate) fn new(write: CatalogPageWrite) -> Result<Self, CatalogCacheError> {
        let payload = wire::encode(&write.page)?;
        let publications = unique_page_publications(&write.page).into_iter().cloned().map(PreparedCatalogPublication::new).collect::<Result<Vec<_>, _>>()?;
        Ok(Self { write, payload, publications })
    }
}

impl PreparedCatalogPublication {
    pub(crate) fn new(publication: CatalogPublication) -> Result<Self, CatalogCacheError> {
        let payload = wire::encode(&publication)?;
        let payload_hash = blake3::hash(&payload).to_hex().to_string();
        let normalized_title = normalize_search_text(&publication.title);
        let normalized_authors = publication.authors.iter().map(|author| normalize_search_text(&author.name)).collect();
        let languages = publication.languages.iter().map(|language| language.as_str().to_owned()).collect();
        let formats = publication_formats(&publication).into_iter().collect();
        let subjects = publication.metadata.subjects.clone();
        let subject_assignments = prepare_catalog_subject_assignments(&subjects);
        let inventory = publication_inventory(&publication);
        let provider_popularity = project_provider_popularity(&publication).unwrap_or_default();
        let gutenberg_has_images = gutenberg_edition_has_images(&publication);
        Ok(Self { publication, payload, payload_hash, normalized_title, provider_popularity, gutenberg_has_images, normalized_authors, languages, formats, subjects, subject_assignments, inventory })
    }
}

pub(crate) fn gutenberg_edition_has_images(publication: &CatalogPublication) -> bool {
    publication.actions.iter().any(|action| {
        let CatalogPublicationAction::Import { request } = action else { return false };
        let download_url = request.download_url.to_ascii_lowercase();
        download_url.contains(".images") && !download_url.contains(".noimages")
    })
}

pub(crate) fn publication_extraction(publication: &CatalogPublication) -> Option<PublicationExtraction<'_>> {
    publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } if matches!(request.book_format, CatalogBookFormat::Epub | CatalogBookFormat::Pdf | CatalogBookFormat::Mobi) => {
            Some(PublicationExtraction { acquisition_url: &request.download_url, book_format: request.book_format })
        }
        _ => None,
    })
}

pub(crate) fn publication_inventory(publication: &CatalogPublication) -> PublicationInventory {
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

pub(crate) fn publication_entry_id(transaction: &Transaction<'_>, source_id: &CatalogSourceId, publication: &CatalogPublication, payload_hash: &str) -> Result<i64, CatalogCacheError> {
    let (edition_id, work_id) = publication_key(publication);
    transaction.execute("INSERT OR IGNORE INTO catalog_publication_entry(source_id,edition_id,work_id,payload_hash) VALUES(?1,?2,?3,?4)", params![source_id.as_str(), edition_id, work_id, payload_hash])?;
    transaction
        .query_row("SELECT entry_id FROM catalog_publication_entry WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND payload_hash=?4", params![source_id.as_str(), edition_id, work_id, payload_hash], |row| row.get(0))
        .map_err(Into::into)
}

pub(crate) fn book_format_storage(format: CatalogBookFormat) -> &'static str {
    match format {
        CatalogBookFormat::Epub => "epub",
        CatalogBookFormat::Pdf => "pdf",
        CatalogBookFormat::Mobi => "mobi",
        CatalogBookFormat::M4b => "m4b",
    }
}

pub(crate) fn catalog_book_format_from_storage(value: &str) -> Option<CatalogBookFormat> {
    match value {
        "epub" => Some(CatalogBookFormat::Epub),
        "pdf" => Some(CatalogBookFormat::Pdf),
        "mobi" => Some(CatalogBookFormat::Mobi),
        "m4b" => Some(CatalogBookFormat::M4b),
        _ => None,
    }
}

pub(crate) fn replace_publication_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication) -> Result<(), CatalogCacheError> {
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

pub(crate) fn copy_subject_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, from_generation: u64, to_generation: u64, identity: &CatalogPublicationIdentity) -> Result<(), CatalogCacheError> {
    if from_generation == to_generation {
        return Ok(());
    }
    let from_generation = storage_i64(from_generation, "generation")?;
    let to_generation = storage_i64(to_generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code)
         SELECT source_id,?2,edition_id,work_id,position,name,source,authority,code FROM catalog_publication_subject
         WHERE source_id=?1 AND generation=?3 AND edition_id=?4 AND work_id=?5 AND position>=?6",
        params![source_id.as_str(), to_generation, from_generation, identity.edition_id, work_id, ENRICHED_SUBJECT_POSITION_BASE],
    )?;
    rebuild_subject_assignments(transaction, source_id, to_generation, identity, None)
}

pub(crate) fn copy_toc_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, from_generation: u64, to_generation: u64, identity: &CatalogPublicationIdentity) -> Result<(), CatalogCacheError> {
    if from_generation == to_generation {
        return Ok(());
    }
    let from_generation = storage_i64(from_generation, "generation")?;
    let to_generation = storage_i64(to_generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication_toc(source_id,generation,edition_id,work_id,position,normalized_title)
         SELECT source_id,?2,edition_id,work_id,position,normalized_title FROM catalog_publication_toc
         WHERE source_id=?1 AND generation=?3 AND edition_id=?4 AND work_id=?5",
        params![source_id.as_str(), to_generation, from_generation, identity.edition_id, work_id],
    )?;
    Ok(())
}

pub(crate) fn copy_popularity_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, from_generation: u64, to_generation: u64, identity: &CatalogPublicationIdentity) -> Result<(), CatalogCacheError> {
    if from_generation == to_generation {
        return Ok(());
    }
    let from_generation = storage_i64(from_generation, "generation")?;
    let to_generation = storage_i64(to_generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication_popularity(
            source_id,generation,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
         ) SELECT source_id,?2,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
           FROM catalog_publication_popularity
          WHERE source_id=?1 AND generation=?3 AND edition_id=?4 AND work_id=?5",
        params![source_id.as_str(), to_generation, from_generation, identity.edition_id, work_id],
    )?;
    Ok(())
}
