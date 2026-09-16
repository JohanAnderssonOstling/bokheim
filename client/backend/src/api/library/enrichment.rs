use super::LibraryClient;
use crate::runtime::library_requests;

impl LibraryClient {}

/// Book-level progress. Enriched means at least one successful metadata update,
/// not that every field is available. Remaining categories are disjoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnrichmentStatus {
    pub running: bool,
    pub total: u64,
    pub enriched: u64,
    pub pending: u64,
    pub no_match: u64,
    pub failed: u64,
}

impl LibraryClient {
    pub async fn enrichment_status(&self) -> Result<EnrichmentStatus, String> {
        self.request(library_requests::EnrichmentStatus).await
    }
}
