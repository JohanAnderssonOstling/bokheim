use super::*;

impl CatalogStore {
    pub(crate) fn put_prepared_pages(&self, source_id: &CatalogSourceId, source_url: &str, pages: &[PreparedCatalogPageWrite]) -> Result<(), CatalogStoreError> {
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

fn put_prepared_page_rows(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, pages: &[PreparedCatalogPageWrite]) -> Result<(), CatalogStoreError> {
    for prepared_page in pages {
        let write = &prepared_page.write;
        put_page_row(transaction, source_id, generation, &write.page, &prepared_page.payload, write.etag.as_deref(), write.last_modified.as_deref(), write.provider_paginated)?;
        for book in &prepared_page.books {
            upsert_active_prepared_book(transaction, source_id, generation, book)?;
            queue_book_extraction(transaction, source_id, generation, &book.book, false)?;
        }
    }
    Ok(())
}

pub(crate) fn upsert_active_book(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, book: &CatalogBook) -> Result<(), CatalogStoreError> {
    let prepared = PreparedCatalogBook::new(book.clone())?;
    upsert_active_prepared_book(transaction, source_id, generation, &prepared)
}

pub(crate) fn upsert_active_prepared_book(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, prepared: &PreparedCatalogBook) -> Result<(), CatalogStoreError> {
    let book = &prepared.book;
    let extraction = book_extraction(book);
    let (edition_id, work_id) = book_key(book);
    let previous = transaction
        .query_row(
            "SELECT payload_hash, sequence, inventory_indexed FROM catalog_book WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, bool>(2)?)),
        )
        .optional()?;
    if previous.as_ref().is_some_and(|(previous_hash, _, inventory_indexed)| *inventory_indexed && previous_hash == &prepared.payload_hash) {
        return queue_book_cover(transaction, source_id, generation, book);
    }
    let sequence = match previous {
        Some((previous_hash, sequence, _)) if previous_hash == prepared.payload_hash => sequence,
        _ => allocate_sequence(transaction, source_id)?,
    };
    let entry_id = book_entry_id(transaction, source_id, book, &prepared.payload_hash)?;
    transaction.execute(
        "INSERT INTO catalog_book(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, provider_popularity, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload, entry_id)
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
            book.image_url,
            prepared.payload,
            entry_id
        ],
    )?;
    replace_prepared_book_metadata(transaction, source_id, generation, prepared)?;
    queue_book_cover(transaction, source_id, generation, &prepared.book)
}

pub(crate) fn insert_staged_book(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, prepared: &PreparedCatalogBook) -> Result<(), CatalogStoreError> {
    let book = &prepared.book;
    let extraction = book_extraction(book);
    let (edition_id, work_id) = book_key(book);
    let entry_id = book_entry_id(transaction, source_id, book, &prepared.payload_hash)?;
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO catalog_book(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, provider_popularity, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload, entry_id)
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
            book.image_url,
            prepared.payload,
            entry_id
        ],
    )?;
    if inserted > 0 {
        replace_prepared_book_metadata(transaction, source_id, generation, prepared)?;
    }
    Ok(())
}

impl PreparedCatalogPageWrite {
    pub(super) fn new(write: CatalogPageWrite) -> Result<Self, CatalogStoreError> {
        let payload = wire::encode(&write.page)?;
        let books = unique_page_books(&write.page).into_iter().cloned().map(PreparedCatalogBook::new).collect::<Result<Vec<_>, _>>()?;
        Ok(Self { write, payload, books })
    }
}

impl PreparedCatalogBook {
    fn new(book: CatalogBook) -> Result<Self, CatalogStoreError> {
        let payload = wire::encode(&book)?;
        let payload_hash = blake3::hash(&payload).to_hex().to_string();
        let normalized_title = normalize_search_text(&book.title);
        let provider_popularity = crate::opds_adapter::opds_book_popularity(&book);
        let normalized_authors = book.authors.iter().map(|author| normalize_search_text(&author.name)).collect();
        let languages = book.languages.iter().map(|language| language.as_str().to_owned()).collect();
        let formats = book
            .actions
            .iter()
            .map(|action| match action {
                CatalogBookAction::Import { request } => book_format_storage(request.book_format),
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let subjects = book.metadata.subjects.clone();
        let subject_assignments = prepare_catalog_subject_assignments(&subjects);
        let inventory = book_inventory(&book);
        Ok(Self {
            book,
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

fn unique_page_books(page: &CatalogPage) -> Vec<&CatalogBook> {
    let mut seen = HashSet::new();
    page.books.iter().chain(page.groups.iter().flat_map(|group| group.books.iter())).filter(|book| seen.insert((book.identity.edition_id.as_str(), book.identity.work_id.as_deref()))).collect()
}

fn replace_prepared_book_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, prepared: &PreparedCatalogBook) -> Result<(), CatalogStoreError> {
    let (edition_id, work_id) = book_key(&prepared.book);
    let generation = storage_i64(generation, "generation")?;
    transaction.execute("DELETE FROM catalog_book_author WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for (position, normalized_name) in prepared.normalized_authors.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_book_author(source_id, generation, edition_id, work_id, position, normalized_name) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![source_id.as_str(), generation, edition_id, work_id, i64::try_from(position).map_err(|_| CatalogStoreError::new("author position overflow"))?, normalized_name],
        )?;
    }
    transaction.execute("DELETE FROM catalog_book_language WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for language in &prepared.languages {
        transaction.execute("INSERT OR IGNORE INTO catalog_book_language(source_id, generation, edition_id, work_id, language) VALUES(?1, ?2, ?3, ?4, ?5)", params![source_id.as_str(), generation, edition_id, work_id, language])?;
    }
    transaction.execute("DELETE FROM catalog_book_format WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for format in &prepared.formats {
        transaction.execute("INSERT INTO catalog_book_format(source_id, generation, edition_id, work_id, book_format) VALUES(?1, ?2, ?3, ?4, ?5)", params![source_id.as_str(), generation, edition_id, work_id, format])?;
    }
    replace_feed_book_subjects_prepared(transaction, source_id, generation as u64, &prepared.book.identity, &prepared.subjects, &prepared.subject_assignments)
}
