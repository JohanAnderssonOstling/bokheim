//! Orders enrichment phases and advances candidate pages after failures.
use super::{DESCRIPTION_PROVIDER, OPEN_LIBRARY_AUTHOR_PROVIDER, OPEN_LIBRARY_COVER_PROVIDER};
use crate::{BackendError, ContentHash, library::LibrarySession};
use client_library::external_metadata;
use client_library::external_metadata::AuthorEnrichmentCandidate;
use client_library::upload_pipeline;
use library_runtime::database::DatabaseError;

impl LibrarySession {
    pub(crate) async fn enrich_library_metadata(&self) -> usize {
        let batch = match self.db.command(crate::storage::storage_queue::Priority::Background, |conn| Ok(upload_pipeline::snapshot(conn)?)).await {
            Ok(batch) => batch,
            Err(error) => {
                log::warn!("could not snapshot enrichment: {error}");
                return 0;
            }
        };
        self.enrich_library_batch(&batch.hashes()).await.unwrap_or_else(|error| {
            log::warn!("enrichment pass will retry: {error}");
            0
        })
    }

    pub(crate) async fn enrich_library_batch(&self, scope: &std::collections::HashSet<ContentHash>) -> Result<usize, BackendError> {
        const PAGE: usize = 32;
        let scope_json = std::sync::Arc::new(serde_json::to_string(scope).map_err(BackendError::operation)?);
        let through = scope.iter().map(|hash| hash.as_str()).max();
        let mut updated = 0;
        // Keyset pages bound selection and application work. Advance even on a
        // failed request so one bad page cannot starve the rest of the self.
        // Each new scheduled pass starts over, retaining existing retry policy.
        macro_rules! pages {
            ($query:expr, $hash:expr, $apply:ident) => {{
                let mut after = String::new();
                loop {
                    if through.is_none_or(|last| !after.is_empty() && after.as_str() >= last) {
                        break;
                    }
                    let cursor = after.clone();
                    let scope_json = scope_json.clone();
                    let result = self.db.command(crate::storage::storage_queue::Priority::Background, move |conn| ($query)(conn, &cursor, &scope_json).map_err(DatabaseError::operation)).await;
                    let mut candidates = match result {
                        Ok(candidates) => candidates,
                        Err(error) => {
                            log::warn!("metadata candidate page failed: {error}");
                            // A schema or storage failure must not spin through
                            // every provider page and immediately be scheduled
                            // again by the background coordinator.
                            crate::executor::sleep(std::time::Duration::from_secs(1)).await;
                            return Err(BackendError::operation(error));
                        }
                    };
                    let Some(last) = candidates.last() else { break };
                    after = ($hash)(last).to_string();
                    candidates.retain(|item| scope.contains(&($hash)(item)));
                    if candidates.is_empty() {
                        continue;
                    }
                    match self.$apply(candidates).await {
                        Ok(count) => updated += count,
                        Err(error) => log::warn!("metadata page will retry on the next pass: {error}"),
                    }
                    crate::executor::sleep(std::time::Duration::from_millis(1)).await;
                }
            }};
        }
        // Existing identifiers can supply descriptions/authors before any book
        // enters expensive inspection. Subjects reuse these cached responses.
        pages!(
            |conn: &rusqlite::Connection, after: &str, scope: &str| external_metadata::description_candidates_after_scoped(conn, DESCRIPTION_PROVIDER, PAGE, after, Some(scope)),
            |item: &(ContentHash, Vec<String>)| item.0,
            enrich_description_candidates
        );
        pages!(
            |conn: &rusqlite::Connection, after: &str, scope: &str| external_metadata::author_enrichment_candidates_after_scoped(conn, OPEN_LIBRARY_AUTHOR_PROVIDER, PAGE, true, Some(after), Some(scope)),
            |item: &AuthorEnrichmentCandidate| item.content_hash,
            apply_author_candidates
        );
        updated += self.enrich_subject_batch(scope).await?;
        // Also consume ISBNs found on pages whose local LCC completed subjects.
        pages!(
            |conn: &rusqlite::Connection, after: &str, scope: &str| external_metadata::description_candidates_after_scoped(conn, DESCRIPTION_PROVIDER, PAGE, after, Some(scope)),
            |item: &(ContentHash, Vec<String>)| item.0,
            enrich_description_candidates
        );
        pages!(
            |conn: &rusqlite::Connection, after: &str, scope: &str| external_metadata::author_enrichment_candidates_after_scoped(conn, OPEN_LIBRARY_AUTHOR_PROVIDER, PAGE, true, Some(after), Some(scope)),
            |item: &AuthorEnrichmentCandidate| item.content_hash,
            apply_author_candidates
        );
        pages!(
            |conn: &rusqlite::Connection, after: &str, scope: &str| external_metadata::cover_enrichment_candidates_after_scoped(conn, OPEN_LIBRARY_COVER_PROVIDER, PAGE, Some(after), Some(scope)),
            |item: &(ContentHash, Vec<String>)| item.0,
            enrich_cover_candidates
        );
        Ok(updated)
    }
}
