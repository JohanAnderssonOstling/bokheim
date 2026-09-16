use super::{catalog_crawls_disabled, CatalogProcess, CatalogProcessAction, CatalogProcessStatus, CatalogService, CatalogServiceError, Ordering};

impl CatalogService {
    pub(crate) fn process_enabled(&self, process: CatalogProcess) -> bool {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_enabled.load(Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_enabled.load(Ordering::Relaxed),
            CatalogProcess::Enrichment => self.state.processes.enrichment_enabled.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn set_process_running(&self, process: CatalogProcess, running: bool) {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_running.store(running, Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_running.store(running, Ordering::Relaxed),
            CatalogProcess::Enrichment => {
                if running {
                    self.state.processes.enrichment_running.fetch_add(1, Ordering::Relaxed);
                } else {
                    let _ = self.state.processes.enrichment_running.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| Some(count.saturating_sub(1)));
                }
            }
        }
    }

    pub(crate) fn process_running(&self, process: CatalogProcess) -> bool {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_running.load(Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_running.load(Ordering::Relaxed),
            CatalogProcess::Enrichment => self.state.processes.enrichment_running.load(Ordering::Relaxed) != 0,
        }
    }

    pub(crate) async fn wait_for_process_wake(&self, process: CatalogProcess) {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_notify.notified().await,
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_notify.notified().await,
            CatalogProcess::Enrichment => self.state.processes.enrichment_notify.notified().await,
        }
    }

    pub(crate) fn wake_process(&self, process: CatalogProcess) {
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_notify.notify_one(),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_notify.notify_one(),
            CatalogProcess::Enrichment => self.state.processes.enrichment_notify.notify_waiters(),
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
                    // Retrying failed rows is maintenance work. Do not make
                    // the administrator wait behind a long crawl/index write
                    // just to wake an already-enabled worker.
                    let service = self.clone();
                    tokio::spawn(async move {
                        if let Err(error) = service.store_write_call_named("cover_retry", |cache| cache.retry_failed_cover_downloads()).await {
                            tracing::warn!(%error, "could not schedule failed cover retries");
                        }
                    });
                }
                self.wake_process(process);
            }
        }
        Ok(self.process_status(process))
    }

    async fn set_process_enabled(&self, process: CatalogProcess, enabled: bool) -> Result<(), CatalogServiceError> {
        let key = process.storage_key();
        // Apply the volatile control state first. Persisting the preference
        // is deliberately asynchronous: an admin pause/resume must not wait
        // behind a long-running catalogue writer transaction.
        match process {
            CatalogProcess::CatalogueCrawling => self.state.processes.catalogue_crawling_enabled.store(enabled, Ordering::Relaxed),
            CatalogProcess::CoverDownloads => self.state.processes.cover_downloads_enabled.store(enabled, Ordering::Relaxed),
            CatalogProcess::Enrichment => self.state.processes.enrichment_enabled.store(enabled, Ordering::Relaxed),
        }
        self.wake_process(process);
        let service = self.clone();
        let persist = async move { service.store_write_call_named("process_control", move |cache| cache.set_process_enabled(key, enabled)).await };
        match tokio::time::timeout(std::time::Duration::from_millis(250), persist).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => {
                let service = self.clone();
                tokio::spawn(async move {
                    if let Err(error) = service.store_write_call_named("process_control", move |cache| cache.set_process_enabled(key, enabled)).await {
                        tracing::warn!(process = key, %error, "could not persist catalogue process control state");
                    }
                });
            }
        }
        Ok(())
    }
}
