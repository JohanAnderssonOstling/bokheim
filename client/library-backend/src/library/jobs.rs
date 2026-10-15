//! Explicit file-operation and thumbnail jobs, also available while signed out.
pub(crate) use super::LibrarySession;
pub(crate) use crate::BackendError;
pub(crate) use crate::ContentHash;
pub(crate) use crate::library::events::LibraryEvent;
use std::collections::HashSet;
use std::time::Duration;

impl LibrarySession {
    pub(crate) async fn run_file_jobs(&self) -> Result<(), BackendError> {
        let had_file_work = self.db.has_pending_file_work()?;
        #[cfg(not(target_arch = "wasm32"))]
        let torrents = if had_file_work || !self.db.native_directory_replay_work()?.is_empty() || self.db.has_pending_purge_work()? {
            Some(self.torrents.before_moves().await?)
        } else { None };
        let result = library_placement::run_file_jobs(&self.assets, &self.db).await;
        #[cfg(not(target_arch = "wasm32"))]
        drop(torrents);
        if had_file_work {
            // Replay completion refreshes views and sync, but is not another
            // request for replay. Durable work can remain queued (for example
            // while a book is leased); ContentsChanged would self-wake forever.
            if !self.directory_imports_active() {
                self.events.publish_update(library_model::LibraryUpdate::Contents);
                self.events.sync.wake();
            }
        }
        result.map_err(BackendError::operation)?;
        let snapshot = self.db.upload_preparation_snapshot()?;
        let scope = self.db.prepared_upload_batch(&snapshot.hashes())?.hashes();
        let sync = self.sync.clone();
        if let Err(error) = sync.record_remote_thumbnail_presence(scope.iter().copied().collect()).await {
            log::warn!("remote thumbnail presence will retry: {error}");
        }

        if self.assets.local_access_enabled() {
            // Freeze membership before preparation. Newly discovered files wait
            // for the next batch, including files added during slow inspection.
            self.refresh_catalog_audiobook_covers(&scope).await?;
            let thumbnails = self.db.upload_thumbnail_candidates(&scope).map_err(BackendError::operation)?;
            let _thumbnails = if thumbnails.is_empty() { None } else { Some(self.runtime.begin_thumbnail_work().ok_or_else(|| BackendError::message("local inspection is already running"))?) };
            // finish_file_work immediately admits and wakes enrichment, including
            // covers that extraction just classified as no_cover.
            self.run_thumbnail_members(thumbnails).await?;
        }
        Ok(())
    }

    /// Refresh approved catalog artwork for existing audiobooks once per
    /// library session, even when their old thumbnail work is already ready.
    async fn refresh_catalog_audiobook_covers(&self, hashes: &HashSet<ContentHash>) -> Result<(), BackendError> {
        use book_model::{AUTHOR_MARC_RELATOR_CODE, BookFormat};
        use metadata_contract::matching::{author_credits, matching_author_count};
        for hash in hashes {
            if self.db.thumbnail_book_format(*hash).map_err(BackendError::operation)? != Some(BookFormat::Mp3Folder) {
                continue;
            }
            let Some(record) = self.db.enrichment_cover_record(hash).map_err(BackendError::operation)? else { continue };
            let title = self.db.browse_book_detail(hash).map_err(BackendError::operation)?.book.title;
            let authors = author_credits(&record.contributors.iter().filter(|credit| credit.role_code() == AUTHOR_MARC_RELATOR_CODE).map(|credit| credit.name().to_owned()).collect::<Vec<_>>());
            if authors.is_empty() {
                continue;
            }
            let candidates = library_database::catalog::audiobook_candidates(&title).map_err(BackendError::operation)?;
            let mut matches = candidates.iter().filter(|book| {
                book.hash.is_none()
                    && book.format() == BookFormat::Mp3Folder
                    && book.title.trim().eq_ignore_ascii_case(title.trim())
                    && matching_author_count(&authors, &author_credits(&book.record.contributors.iter().filter(|credit| credit.role_code() == AUTHOR_MARC_RELATOR_CODE).map(|credit| credit.name().to_owned()).collect::<Vec<_>>())) > 0
                    && (record.book.languages.is_empty() || book.record.book.languages.is_empty() || record.book.languages.iter().any(|language| book.record.book.languages.contains(language)))
            });
            let Some(book) = matches.next() else { continue };
            // Do not choose between different recordings or languages by title.
            if matches.next().is_some() || !self.runtime.claim_catalog_cover_refresh(*hash) {
                continue;
            }
            if let Err(error) = self.import.refresh_external_catalog_audiobook_cover(*hash, book.key).await {
                self.runtime.retry_catalog_cover_refresh(hash);
                self.events.files.wake_after(Duration::from_secs(60));
                log::warn!("catalog audiobook thumbnail replacement will retry: {error}");
            }
        }
        Ok(())
    }

    pub(crate) async fn run_thumbnail_members(&self, hashes: Vec<ContentHash>) -> Result<(), BackendError> {
        use asset_transfer::thumbnail_jobs::{ThumbnailExtractor, ThumbnailJobs};
        if hashes.is_empty() {
            return Ok(());
        }
        let extractor = ThumbnailExtractor::new(self.cpu.clone());
        let jobs = ThumbnailJobs::new(&self.db, &self.assets, &extractor);
        let mut observer = ThumbnailEvents { library: self };
        jobs.run(hashes, &mut observer).await.map_err(BackendError::operation)
    }
}

pub(crate) struct ThumbnailEvents<'a> {
    pub(crate) library: &'a LibrarySession,
}
impl asset_transfer::thumbnail_jobs::ThumbnailObserver for ThumbnailEvents<'_> {
    fn completed(&mut self) {
        self.library.events.sync.wake();
    }
    fn ready(&mut self, content_hash: ContentHash) {
        self.library.emit(LibraryEvent::ThumbnailGenerated { content_hash });
    }
}
