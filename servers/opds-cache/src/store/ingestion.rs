use super::*;

impl CatalogCache {
    pub fn set_provider_epoch(&self, source_id: &CatalogSourceId, source_url: &str, provider_epoch: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let previous = transaction.query_row("SELECT provider_epoch FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| row.get::<_, Option<String>>(0))?;
        let changed = previous.as_deref().is_some_and(|previous| previous != provider_epoch);
        if changed {
            clear_source_rows(&transaction, source_id)?;
            transaction.execute("DELETE FROM catalog_epub_extraction WHERE source_id=?1", [source_id.as_str()])?;
            transaction.execute("DELETE FROM catalog_cover_download WHERE source_id=?1", [source_id.as_str()])?;
        }
        transaction.execute("UPDATE catalog_source SET provider_epoch=?2 WHERE source_id=?1", params![source_id.as_str(), provider_epoch])?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn put_page(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
        let prepared = Self::prepare_page_writes(vec![CatalogPageWrite { page: page.clone(), etag: etag.map(str::to_owned), last_modified: last_modified.map(str::to_owned), provider_paginated: false }])?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_prepared_page_rows(&transaction, source_id, generation, &prepared)?;
        refresh_active_counts(&transaction, source_id, generation)?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn put_pages(&self, source_id: &CatalogSourceId, source_url: &str, pages: &[CatalogPageWrite]) -> Result<(), CatalogCacheError> {
        self.put_prepared_pages(source_id, source_url, &Self::prepare_page_writes(pages.to_vec())?)
    }

    pub(crate) fn put_prepared_pages(&self, source_id: &CatalogSourceId, source_url: &str, pages: &[PreparedCatalogPageWrite]) -> Result<(), CatalogCacheError> {
        if pages.is_empty() {
            return Ok(());
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_prepared_page_rows(&transaction, source_id, generation, pages)?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(())
    }
}

fn put_prepared_page_rows(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, pages: &[PreparedCatalogPageWrite]) -> Result<(), CatalogCacheError> {
    for prepared_page in pages {
        let write = &prepared_page.write;
        put_page_row(transaction, source_id, generation, &write.page, &prepared_page.payload, write.etag.as_deref(), write.last_modified.as_deref(), write.provider_paginated)?;
        for publication in &prepared_page.publications {
            upsert_active_prepared_publication(transaction, source_id, generation, publication)?;
            queue_publication_extraction(transaction, source_id, generation, &publication.publication, false)?;
        }
    }
    Ok(())
}

pub(crate) fn upsert_active_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication) -> Result<(), CatalogCacheError> {
    let prepared = PreparedCatalogPublication::new(publication.clone())?;
    upsert_active_prepared_publication(transaction, source_id, generation, &prepared)
}

pub(crate) fn upsert_active_prepared_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, prepared: &PreparedCatalogPublication) -> Result<(), CatalogCacheError> {
    let publication = &prepared.publication;
    let extraction = publication_extraction(publication);
    let (edition_id, work_id) = publication_key(publication);
    let previous = transaction
        .query_row(
            "SELECT payload_hash, sequence, inventory_indexed FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, bool>(2)?)),
        )
        .optional()?;
    if previous.as_ref().is_some_and(|(previous_hash, _, inventory_indexed)| *inventory_indexed && previous_hash == &prepared.payload_hash) {
        return queue_publication_cover(transaction, source_id, generation, publication);
    }
    let sequence = match previous {
        Some((previous_hash, sequence, _)) if previous_hash == prepared.payload_hash => sequence,
        _ => allocate_sequence(transaction, source_id)?,
    };
    let entry_id = publication_entry_id(transaction, source_id, publication, &prepared.payload_hash)?;
    transaction.execute(
        "INSERT INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, provider_popularity, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload, entry_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(source_id, generation, edition_id, work_id) DO UPDATE SET entry_id=excluded.entry_id, sequence=excluded.sequence, payload_hash=excluded.payload_hash, normalized_title=excluded.normalized_title,
             provider_popularity=excluded.provider_popularity, book_format=excluded.book_format, importable=excluded.importable, has_upstream_cover=excluded.has_upstream_cover, inventory_indexed=1,
             acquisition_url=excluded.acquisition_url, extraction_format=excluded.extraction_format, cover_url=excluded.cover_url, payload=excluded.payload",
        params![
            source_id.as_str(),
            storage_i64(generation, "generation")?,
            edition_id,
            work_id,
            sequence,
            prepared.payload_hash,
            prepared.normalized_title,
            storage_i64(prepared.provider_popularity, "provider popularity")?,
            prepared.book_format,
            prepared.importable,
            prepared.has_upstream_cover,
            extraction.as_ref().map(|item| item.acquisition_url),
            extraction.as_ref().map(|item| book_format_storage(item.book_format)),
            publication.image_url,
            prepared.payload,
            entry_id
        ],
    )?;
    replace_prepared_publication_metadata(transaction, source_id, generation, prepared)?;
    queue_publication_cover(transaction, source_id, generation, &prepared.publication)
}

pub(crate) fn insert_staged_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, prepared: &PreparedCatalogPublication) -> Result<(), CatalogCacheError> {
    let publication = &prepared.publication;
    let extraction = publication_extraction(publication);
    let (edition_id, work_id) = publication_key(publication);
    let entry_id = publication_entry_id(transaction, source_id, publication, &prepared.payload_hash)?;
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, provider_popularity, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload, entry_id)
         VALUES(?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?13, ?14, ?15)",
        params![
            source_id.as_str(),
            storage_i64(generation, "generation")?,
            edition_id,
            work_id,
            prepared.payload_hash,
            prepared.normalized_title,
            storage_i64(prepared.provider_popularity, "provider popularity")?,
            prepared.book_format,
            prepared.importable,
            prepared.has_upstream_cover,
            extraction.as_ref().map(|item| item.acquisition_url),
            extraction.as_ref().map(|item| book_format_storage(item.book_format)),
            publication.image_url,
            prepared.payload,
            entry_id
        ],
    )?;
    if inserted > 0 {
        replace_prepared_publication_metadata(transaction, source_id, generation, prepared)?;
    }
    Ok(())
}

impl PreparedCatalogPageWrite {
    pub(super) fn new(write: CatalogPageWrite) -> Result<Self, CatalogCacheError> {
        let payload = wire::encode(&write.page)?;
        let publications = unique_page_publications(&write.page).into_iter().cloned().map(PreparedCatalogPublication::new).collect::<Result<Vec<_>, _>>()?;
        Ok(Self { write, payload, publications })
    }
}

impl PreparedCatalogPublication {
    fn new(publication: CatalogPublication) -> Result<Self, CatalogCacheError> {
        let payload = wire::encode(&publication)?;
        let payload_hash = blake3::hash(&payload).to_hex().to_string();
        let normalized_title = normalize_search_text(&publication.title);
        let provider_popularity = crate::opds_adapter::opds_publication_popularity(&publication);
        let normalized_authors = publication.authors.iter().map(|author| normalize_search_text(&author.name)).collect();
        let languages = publication.languages.iter().map(|language| language.as_str().to_owned()).collect();
        let formats = publication
            .actions
            .iter()
            .filter_map(|action| match action {
                CatalogPublicationAction::Import { request } => Some(book_format_storage(request.book_format)),
                CatalogPublicationAction::External { .. } => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let subjects = publication.metadata.subjects.clone();
        let subject_assignments = prepare_catalog_subject_assignments(&subjects);
        let inventory = publication_inventory(&publication);
        Ok(Self {
            publication,
            payload,
            payload_hash,
            normalized_title,
            provider_popularity,
            normalized_authors,
            languages,
            formats,
            subjects,
            subject_assignments,
            book_format: inventory.book_format,
            importable: inventory.importable,
            has_upstream_cover: inventory.has_upstream_cover,
        })
    }
}

fn unique_page_publications(page: &CatalogPage) -> Vec<&CatalogPublication> {
    let mut seen = HashSet::new();
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter(|publication| seen.insert((publication.identity.edition_id.as_str(), publication.identity.work_id.as_deref()))).collect()
}

fn replace_prepared_publication_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, prepared: &PreparedCatalogPublication) -> Result<(), CatalogCacheError> {
    let (edition_id, work_id) = publication_key(&prepared.publication);
    let generation = storage_i64(generation, "generation")?;
    transaction.execute("DELETE FROM catalog_publication_author WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for (position, normalized_name) in prepared.normalized_authors.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_publication_author(source_id, generation, edition_id, work_id, position, normalized_name) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![source_id.as_str(), generation, edition_id, work_id, i64::try_from(position).map_err(|_| CatalogCacheError::new("author position overflow"))?, normalized_name],
        )?;
    }
    transaction.execute("DELETE FROM catalog_publication_language WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for language in &prepared.languages {
        transaction.execute("INSERT OR IGNORE INTO catalog_publication_language(source_id, generation, edition_id, work_id, language) VALUES(?1, ?2, ?3, ?4, ?5)", params![source_id.as_str(), generation, edition_id, work_id, language])?;
    }
    transaction.execute("DELETE FROM catalog_publication_format WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for format in &prepared.formats {
        transaction.execute("INSERT INTO catalog_publication_format(source_id, generation, edition_id, work_id, book_format) VALUES(?1, ?2, ?3, ?4, ?5)", params![source_id.as_str(), generation, edition_id, work_id, format])?;
    }
    replace_feed_publication_subjects_prepared(transaction, source_id, generation as u64, &prepared.publication.identity, &prepared.subjects, &prepared.subject_assignments)
}
