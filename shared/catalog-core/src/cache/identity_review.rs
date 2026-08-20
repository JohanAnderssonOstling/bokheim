use super::{
    decode_cached, params, sql_decode_error, storage_i64, stored_u64, unix_timestamp, CatalogCache, CatalogCacheError, CatalogEpubMetadata, CatalogIdentityReview, CatalogIdentityReviewInventory, CatalogPublicationIdentity, CatalogSourceId,
    Connection, OptionalExtension, Transaction, TransactionBehavior, CATALOG_METADATA_BACKFILL_VERSION,
};

impl CatalogCache {
    pub fn identity_review_inventory(&self) -> Result<CatalogIdentityReviewInventory, CatalogCacheError> {
        let connection = self.pool.get()?;
        identity_review_inventory_in(&connection, None)
    }

    pub fn identity_review_inventory_for_source(&self, source_id: &CatalogSourceId) -> Result<CatalogIdentityReviewInventory, CatalogCacheError> {
        let connection = self.pool.get()?;
        identity_review_inventory_in(&connection, Some(source_id))
    }

    pub fn identity_reviews(&self, source_id: Option<&CatalogSourceId>, status: Option<&str>, limit: usize) -> Result<Vec<CatalogIdentityReview>, CatalogCacheError> {
        if status.is_some_and(|value| !matches!(value, "ambiguous" | "no_match" | "insufficient_metadata" | "failed" | "resolved" | "rejected")) {
            return Err(CatalogCacheError::new("invalid catalogue identity review status"));
        }
        let limit = storage_i64(limit.clamp(1, 200) as u64, "identity review limit")?;
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT review.review_id,review.source_id,extraction.edition_id,extraction.work_id,extraction.metadata_payload,
                    review.status,review.detail,review.result_payload,review.candidates_truncated,review.updated_at
             FROM catalog_identity_review review
             JOIN catalog_epub_extraction extraction
               ON extraction.source_id=review.source_id AND extraction.acquisition_url=review.acquisition_url
             JOIN catalog_source source ON source.source_id=review.source_id
             JOIN catalog_publication publication
               ON publication.source_id=review.source_id AND publication.generation=source.active_generation
              AND publication.acquisition_url=review.acquisition_url
             WHERE (?1 IS NULL OR review.source_id=?1)
               AND ((?2 IS NULL AND review.status IN ('ambiguous','no_match','insufficient_metadata','failed')) OR review.status=?2)
             ORDER BY CASE review.status WHEN 'ambiguous' THEN 0 WHEN 'no_match' THEN 1 WHEN 'insufficient_metadata' THEN 2 WHEN 'failed' THEN 3 ELSE 4 END,
                      review.updated_at DESC,review.review_id LIMIT ?3",
        )?;
        let rows = statement.query_map(params![source_id.map(CatalogSourceId::as_str), status, limit], |row| {
            let source = row.get::<_, String>(1)?;
            let metadata_payload = row.get::<_, Vec<u8>>(4)?;
            let metadata: CatalogEpubMetadata = decode_cached(&metadata_payload).map_err(sql_decode_error)?;
            let result_payload = row.get::<_, Option<Vec<u8>>>(7)?;
            let result = result_payload.map(|payload| serde_json::from_slice::<metadata_contract::EditionIdentityResult>(&payload)).transpose().map_err(sql_decode_error)?;
            let publication_year = library_model::unique_publication_year(metadata.book.publication.dates.iter().map(|date| date.value()));
            let title = metadata.book.title.clone();
            let authors = metadata.book.authors().map(|author| author.name().to_owned()).collect();
            let publishers = metadata.book.publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect();
            Ok(CatalogIdentityReview {
                review_id: row.get(0)?,
                source_id: CatalogSourceId::parse(&source).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?,
                publication: CatalogPublicationIdentity { edition_id: row.get(2)?, work_id: row.get::<_, Option<String>>(3)?.filter(|value| !value.is_empty()) },
                title,
                authors,
                publishers,
                publication_year,
                status: row.get(5)?,
                detail: row.get(6)?,
                candidates: result.as_ref().map(|result| result.candidates.clone().into_iter().map(Into::into).collect()).unwrap_or_default(),
                candidates_truncated: row.get(8)?,
                updated_at: stored_u64(row.get(9)?, "identity review update timestamp")?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn identity_review_candidate_isbn(&self, review_id: &str, edition_id: &str, isbn13: &str) -> Result<Option<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let payload = connection.query_row("SELECT result_payload FROM catalog_identity_review WHERE review_id=?1 AND status='ambiguous'", [review_id], |row| row.get::<_, Option<Vec<u8>>>(0)).optional()?.flatten();
        let Some(payload) = payload else { return Ok(None) };
        let result: metadata_contract::EditionIdentityResult = serde_json::from_slice(&payload).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        Ok(result.candidates.into_iter().find(|candidate| candidate.open_library_edition_id == edition_id && candidate.canonical_isbn13 == isbn13).map(|candidate| candidate.canonical_isbn13))
    }

    pub fn resolve_identity_review_with_isbn(&self, review_id: &str, canonical_isbn13: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = transaction
            .query_row(
                "SELECT review.source_id,review.acquisition_url,extraction.metadata_payload
                 FROM catalog_identity_review review JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=review.source_id AND extraction.acquisition_url=review.acquisition_url
                 WHERE review.review_id=?1 AND review.status IN ('ambiguous','no_match','insufficient_metadata','failed')",
                [review_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)),
            )
            .optional()?;
        let Some((source_id, acquisition_url, payload)) = row else { return Ok(false) };
        let mut metadata: CatalogEpubMetadata = decode_cached(&payload)?;
        let provenance = library_model::PublicationIdentifierType::new("openlibrary-edition-match", Some("bokheim:provenance".to_owned())).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        let identifier = library_model::PublicationIdentifier::new(canonical_isbn13, library_model::PublicationIdentifierScheme::Isbn, vec![provenance]).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        metadata.book.publication.identifiers.retain(|existing| existing.scheme() != &library_model::PublicationIdentifierScheme::Isbn);
        metadata.book.publication.identifiers.push(identifier);
        transaction.execute("UPDATE catalog_epub_extraction SET metadata_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![source_id, acquisition_url, wire::encode(&metadata)?])?;
        transaction.execute(
            "INSERT INTO catalog_metadata_backfill(source_id,acquisition_url,version,status) VALUES(?1,?2,?3,'pending')
             ON CONFLICT(source_id,acquisition_url) DO UPDATE SET version=excluded.version,status='pending',last_error=NULL",
            params![source_id, acquisition_url, CATALOG_METADATA_BACKFILL_VERSION],
        )?;
        let now = unix_timestamp()?;
        transaction.execute("UPDATE catalog_identity_review SET status='resolved',selected_isbn=?2,resolved_at=?3,updated_at=?3 WHERE review_id=?1", params![review_id, canonical_isbn13, now])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn reject_identity_review(&self, review_id: &str) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let now = unix_timestamp()?;
        Ok(connection.execute("UPDATE catalog_identity_review SET status='rejected',resolved_at=?2,updated_at=?2 WHERE review_id=?1 AND status IN ('ambiguous','no_match','insufficient_metadata','failed')", params![review_id, now])? == 1)
    }

    pub fn retry_identity_review(&self, review_id: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = transaction
            .query_row("SELECT source_id,acquisition_url FROM catalog_identity_review WHERE review_id=?1 AND status IN ('ambiguous','no_match','insufficient_metadata','failed')", [review_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?;
        let Some((source_id, acquisition_url)) = row else { return Ok(false) };
        transaction.execute(
            "INSERT INTO catalog_metadata_backfill(source_id,acquisition_url,version,status) VALUES(?1,?2,?3,'pending')
             ON CONFLICT(source_id,acquisition_url) DO UPDATE SET version=excluded.version,status='pending',last_error=NULL",
            params![source_id, acquisition_url, CATALOG_METADATA_BACKFILL_VERSION],
        )?;
        transaction.execute("DELETE FROM catalog_identity_review WHERE review_id=?1", [review_id])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn retry_ambiguous_identity_reviews(&self) -> Result<usize, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection
            .execute(
                "UPDATE catalog_metadata_backfill SET version=?1,status='pending',last_error=NULL
                 WHERE status='succeeded'
                   AND EXISTS (
                       SELECT 1 FROM catalog_identity_review review
                       WHERE review.source_id=catalog_metadata_backfill.source_id
                         AND review.acquisition_url=catalog_metadata_backfill.acquisition_url
                         AND review.status='ambiguous'
                   )",
                [CATALOG_METADATA_BACKFILL_VERSION],
            )
            .map_err(Into::into)
    }
}

pub(crate) fn identity_review_inventory_in(connection: &Connection, source_id: Option<&CatalogSourceId>) -> Result<CatalogIdentityReviewInventory, CatalogCacheError> {
    let mut inventory = CatalogIdentityReviewInventory::default();
    let mut statement = connection.prepare("SELECT status,COUNT(*) FROM catalog_identity_review WHERE (?1 IS NULL OR source_id=?1) GROUP BY status")?;
    let rows = statement.query_map([source_id.map(CatalogSourceId::as_str)], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    for row in rows {
        let (status, count) = row?;
        let count = stored_u64(count, "identity review count")?;
        match status.as_str() {
            "ambiguous" => inventory.ambiguous = count,
            "no_match" => inventory.no_match = count,
            "insufficient_metadata" => inventory.insufficient_metadata = count,
            "failed" => inventory.failed = count,
            "resolved" => inventory.resolved = count,
            "rejected" => inventory.rejected = count,
            _ => return Err(CatalogCacheError::new("invalid stored identity review status")),
        }
    }
    Ok(inventory)
}

pub(crate) fn record_identity_review(transaction: &Transaction<'_>, source_id: &str, acquisition_url: &str, result: Option<&metadata_contract::EditionIdentityResult>) -> Result<(), CatalogCacheError> {
    match result {
        Some(result) if result.status != metadata_contract::EditionIdentityStatus::Matched => {
            let status = match result.status {
                metadata_contract::EditionIdentityStatus::Ambiguous => "ambiguous",
                metadata_contract::EditionIdentityStatus::NoMatch => "no_match",
                metadata_contract::EditionIdentityStatus::InsufficientMetadata => "insufficient_metadata",
                metadata_contract::EditionIdentityStatus::Matched => unreachable!(),
            };
            let review_id = blake3::hash(format!("{source_id}\n{acquisition_url}").as_bytes()).to_hex().to_string();
            let now = unix_timestamp()?;
            transaction.execute(
                "INSERT INTO catalog_identity_review(review_id,source_id,acquisition_url,status,result_payload,candidates_truncated,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?7)
                 ON CONFLICT(source_id,acquisition_url) DO UPDATE SET status=excluded.status,result_payload=excluded.result_payload,detail=NULL,candidates_truncated=excluded.candidates_truncated,updated_at=excluded.updated_at,resolved_at=NULL,selected_isbn=NULL",
                params![review_id, source_id, acquisition_url, status, serde_json::to_vec(result)?, result.candidates_truncated, now],
            )?;
        }
        _ => {
            transaction.execute("DELETE FROM catalog_identity_review WHERE source_id=?1 AND acquisition_url=?2 AND status IN ('ambiguous','no_match','insufficient_metadata','failed')", params![source_id, acquisition_url])?;
        }
    }
    Ok(())
}

pub(crate) fn record_identity_review_failure(transaction: &Transaction<'_>, source_id: &str, acquisition_url: &str, error: &str) -> Result<(), CatalogCacheError> {
    let review_id = blake3::hash(format!("{source_id}\n{acquisition_url}").as_bytes()).to_hex().to_string();
    let now = unix_timestamp()?;
    transaction
        .execute(
            "INSERT INTO catalog_identity_review(review_id,source_id,acquisition_url,status,detail,created_at,updated_at)
             VALUES(?1,?2,?3,'failed',?4,?5,?5)
             ON CONFLICT(source_id,acquisition_url) DO UPDATE SET status='failed',result_payload=NULL,detail=excluded.detail,candidates_truncated=0,updated_at=excluded.updated_at,resolved_at=NULL,selected_isbn=NULL
             WHERE catalog_identity_review.status NOT IN ('resolved','rejected')",
            params![review_id, source_id, acquisition_url, error, now],
        )
        .map(|_| ())
        .map_err(Into::into)
}
