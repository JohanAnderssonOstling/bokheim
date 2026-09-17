//! Orchestration for the durable catalogue-enrichment pipeline.
//!
//! The individual stages deliberately live next to the storage operations
//! they use (`crawl`, `backfill`, and `extraction`).  This module is the
//! pipeline boundary: it establishes the order in which those stages run and
//! makes each pass restartable.  A stage owns its own bounded claims, so a
//! pause or an error leaves unfinished work durable for the next pass.

use crate::{CatalogProcess, CatalogService};
use book_enrichment::EnrichmentWave;

impl CatalogService {
    /// Runs one complete catalogue-enrichment pass.
    ///
    /// Index migration must precede metadata backfill because backfill claims
    /// are keyed by the compact metadata index.  Description enrichment is a
    /// separate pass and therefore follows metadata backfill. Serving changes
    /// are consumed independently by the background projector.
    pub(crate) async fn run_catalogue_enrichment_pass(&self, startup_repairs: bool) {
        if !self.process_enabled(CatalogProcess::Enrichment) {
            return;
        }

        if startup_repairs {
            self.refresh_search_index().await;
        }

        for wave in EnrichmentWave::ALL {
            match wave {
                EnrichmentWave::CheapMetadata => self.resume_metadata_index().await,
                EnrichmentWave::TitleAuthor => self.resume_metadata_backfill().await,
                EnrichmentWave::Ocr => self.resume_ocr_backfill().await,
            }
            if !self.process_enabled(CatalogProcess::Enrichment) {
                return;
            }
        }
        self.resume_description_backfill().await;
    }
}
