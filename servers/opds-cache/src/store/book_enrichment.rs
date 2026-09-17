use super::*;

pub fn merge_catalog_subject_evidence(book: &mut CatalogBook, subjects: &[BookSubject]) {
    for subject in subjects {
        let candidate = subject.clone();
        if !book.metadata.subjects.contains(&candidate) {
            book.metadata.subjects.push(candidate);
        }
    }
}

pub(crate) fn merge_catalog_book_metadata(target: &mut catalog_model::CatalogBookMetadata, candidate: &catalog_model::CatalogBookMetadata) {
    if target.published.is_none() {
        target.published.clone_from(&candidate.published);
    }
    if target.modified.is_none() {
        target.modified.clone_from(&candidate.modified);
    }
    if target.subtitle.is_none() {
        target.subtitle.clone_from(&candidate.subtitle);
    }
    append_unique(&mut target.dates, &candidate.dates);
    append_unique(&mut target.publishers, &candidate.publishers);
    append_unique(&mut target.identifiers, &candidate.identifiers);
    append_unique(&mut target.subjects, &candidate.subjects);
    append_unique(&mut target.alternate_titles, &candidate.alternate_titles);
    append_unique(&mut target.collections, &candidate.collections);
    append_unique(&mut target.contributors, &candidate.contributors);
    append_unique(&mut target.rights, &candidate.rights);
    append_unique(&mut target.sources, &candidate.sources);
    append_unique(&mut target.types, &candidate.types);
    append_unique(&mut target.formats, &candidate.formats);
    append_unique(&mut target.relations, &candidate.relations);
    append_unique(&mut target.coverage, &candidate.coverage);
    append_unique(&mut target.properties, &candidate.properties);
    append_unique(&mut target.audiences, &candidate.audiences);
    append_unique(&mut target.age_ranges, &candidate.age_ranges);
    append_unique(&mut target.images, &candidate.images);
    append_unique(&mut target.ratings, &candidate.ratings);
    append_unique(&mut target.interactions, &candidate.interactions);
    append_unique(&mut target.popularity_ranks, &candidate.popularity_ranks);
}

fn append_unique<T: Clone + PartialEq>(target: &mut Vec<T>, candidates: &[T]) {
    for candidate in candidates {
        if !target.contains(candidate) {
            target.push(candidate.clone());
        }
    }
}

impl CatalogStore {
    pub fn enrich_page_from_epub_extractions(&self, source_id: &CatalogSourceId, page: &mut CatalogPage) -> Result<(), CatalogStoreError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT result_payload FROM catalog_epub_extraction
             WHERE source_id=?1 AND acquisition_url=?2 AND status='succeeded' AND result_payload IS NOT NULL",
        )?;
        for book in page.books.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.books.iter_mut())) {
            let Some(extraction) = book_extraction(book) else { continue };
            let payload = statement.query_row(params![source_id.as_str(), extraction.acquisition_url], |row| row.get::<_, Vec<u8>>(0)).optional()?;
            let Some(payload) = payload else { continue };
            let enriched: CatalogBook = decode_cached(&payload)?;
            book.title = enriched.title;
            book.authors = enriched.authors;
            book.languages = enriched.languages;
            book.description = enriched.description;
            book.image_url = enriched.image_url;
            merge_catalog_book_metadata(&mut book.metadata, &enriched.metadata);
        }
        Ok(())
    }

    pub fn active_book_for_cover(&self, source_id: &CatalogSourceId, cover_url: &str) -> Result<Option<CatalogBook>, CatalogStoreError> {
        let connection = self.connection()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(None) };
        let payload = connection
            .query_row(
                "SELECT payload FROM (
                     SELECT book.payload,book.edition_id,book.work_id,0 AS priority
                     FROM catalog_book book
                     WHERE book.source_id=?1 AND book.generation=?2 AND book.cover_url=?3
                     UNION ALL
                     SELECT book.payload,book.edition_id,book.work_id,1 AS priority
                     FROM catalog_cover_download cover
                     JOIN catalog_book book
                       ON book.source_id=cover.source_id AND book.generation=cover.catalog_generation
                      AND book.edition_id=cover.edition_id AND book.work_id=cover.work_id
                     WHERE cover.source_id=?1 AND cover.catalog_generation=?2 AND cover.image_url=?3
                 ) ORDER BY priority,edition_id,work_id LIMIT 1",
                params![source_id.as_str(), storage_i64(generation, "generation")?, cover_url],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        payload.map(|payload| decode_cached(&payload)).transpose().map_err(Into::into)
    }
}
