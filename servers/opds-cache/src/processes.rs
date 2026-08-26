use super::{catalog_crawls_disabled, CatalogProcess, CatalogProcessAction, CatalogProcessStatus, CatalogService, CatalogServiceError, Ordering};

impl CatalogService {
    pub(crate) fn process_enabled(&self, process: CatalogProcess) -> bool {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_enabled.load(Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_enabled.load(Ordering::Relaxed),
            CatalogProcess::EpubEnrichment => self.state.processes.epub_enrichment_enabled.load(Ordering::Relaxed),
            CatalogProcess::CatalogueEnrichment => self.state.processes.catalogue_enrichment_enabled.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn set_process_running(&self, process: CatalogProcess, running: bool) {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_running.store(running, Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_running.store(running, Ordering::Relaxed),
            CatalogProcess::EpubEnrichment => self.state.processes.epub_enrichment_running.store(running, Ordering::Relaxed),
            CatalogProcess::CatalogueEnrichment => self.state.processes.catalogue_enrichment_running.store(running, Ordering::Relaxed),
        }
    }

    pub(crate) fn process_running(&self, process: CatalogProcess) -> bool {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_running.load(Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_running.load(Ordering::Relaxed),
            CatalogProcess::EpubEnrichment => self.state.processes.epub_enrichment_running.load(Ordering::Relaxed),
            CatalogProcess::CatalogueEnrichment => self.state.processes.catalogue_enrichment_running.load(Ordering::Relaxed),
        }
    }

    pub(crate) async fn wait_for_process_wake(&self, process: CatalogProcess) {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_notify.notified().await,
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_notify.notified().await,
            CatalogProcess::EpubEnrichment => self.state.processes.epub_enrichment_notify.notified().await,
            CatalogProcess::CatalogueEnrichment => self.state.processes.catalogue_enrichment_notify.notified().await,
        }
    }

    pub(crate) fn wake_process(&self, process: CatalogProcess) {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_notify.notify_one(),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_notify.notify_one(),
            CatalogProcess::EpubEnrichment => self.state.processes.epub_enrichment_notify.notify_one(),
            CatalogProcess::CatalogueEnrichment => self.state.processes.catalogue_enrichment_notify.notify_one(),
        }
    }

    pub(crate) fn process_status(&self, process: CatalogProcess) -> CatalogProcessStatus {
        let enabled = self.process_enabled(process);
        let running = self.process_running(process);
        CatalogProcessStatus { process, enabled, running, pause_pending: running && !enabled, blocked_by_configuration: process == CatalogProcess::CatalogueCrawling && catalog_crawls_disabled() }
    }

    pub(crate) fn process_statuses(&self) -> Vec<CatalogProcessStatus> {
        CatalogProcess::ALL.into_iter().map(|process| self.process_status(process)).collect()
    }

    pub(crate) async fn apply_process_action(&self, process: CatalogProcess, action: CatalogProcessAction) -> Result<CatalogProcessStatus, CatalogServiceError> {
        match action {
            CatalogProcessAction::Pause => self.set_process_enabled(process, false).await?,
            CatalogProcessAction::Resume => self.set_process_enabled(process, true).await?,
            CatalogProcessAction::RunNow => {
                if !self.process_enabled(process) {
                    return Err(CatalogServiceError("resume the catalogue process before requesting a run".to_owned()));
                }
                if process == CatalogProcess::CoverDownloads {
                    self.cache_write_call_named("cover_retry", |cache| cache.retry_failed_cover_downloads()).await?;
                }
                self.wake_process(process);
            }
        }
        Ok(self.process_status(process))
    }

    async fn set_process_enabled(&self, process: CatalogProcess, enabled: bool) -> Result<(), CatalogServiceError> {
        let key = process.storage_key();
        self.cache_write_call_named("process_control", move |cache| cache.set_process_enabled(key, enabled)).await?;
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_enabled.store(enabled, Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_enabled.store(enabled, Ordering::Relaxed),
            CatalogProcess::EpubEnrichment => self.state.processes.epub_enrichment_enabled.store(enabled, Ordering::Relaxed),
            CatalogProcess::CatalogueEnrichment => self.state.processes.catalogue_enrichment_enabled.store(enabled, Ordering::Relaxed),
        }
        self.wake_process(process);
        Ok(())
    }
}
