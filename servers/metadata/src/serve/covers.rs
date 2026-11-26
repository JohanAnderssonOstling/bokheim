//! Resolved artwork persisted separately from the read-only metadata snapshot.
use crate::error;
use crate::query::run_database;
use crate::schema::now_ms;
use crate::{MetadataError, MetadataService};
use futures_util::StreamExt;
use metadata_contract::books::CoverSource;
use rusqlite::OptionalExtension;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

// Only request starts are paced. Slow bodies do not hold this lock.
static OPEN_LIBRARY: AsyncMutex<Option<tokio::time::Instant>> = AsyncMutex::const_new(None);
static DOWNLOADS: Semaphore = Semaphore::const_new(4);
static REQUESTS: Mutex<BTreeMap<String, Weak<AsyncMutex<()>>>> = Mutex::new(BTreeMap::new());
static HTTP: LazyLock<reqwest::Client> =
    LazyLock::new(|| http_response::client_builder().timeout(Duration::from_secs(8)).connect_timeout(Duration::from_secs(3)).user_agent("Bokheim/0.1 (cover enrichment)").build().expect("cover HTTP client"));
const MAX_BYTES: usize = 4 * 1024 * 1024;
const COVER_BUDGET: Duration = Duration::from_secs(12);
const MISS_TTL_MS: i64 = 24 * 60 * 60 * 1000;

fn request_lock(key: String) -> Result<Arc<AsyncMutex<()>>, MetadataError> {
    let mut requests = REQUESTS.lock().map_err(error)?;
    if let Some(lock) = requests.get(&key).and_then(Weak::upgrade) {
        return Ok(lock);
    }
    requests.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(AsyncMutex::new(()));
    requests.insert(key, Arc::downgrade(&lock));
    Ok(lock)
}

impl MetadataService {
    pub(crate) fn cached_cover(&self, id: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        self.state.demand.lock().map_err(error)?.query_row("SELECT bytes FROM resolved_cover WHERE id=?1", [id], |r| r.get(0)).optional().map_err(error)
    }

    // Presence checks avoid reading and allocating the cached image itself.
    async fn cover_status(&self, id: &str) -> Result<Option<bool>, MetadataError> {
        let id = id.to_owned();
        run_database(self.clone(), move |service| {
            let (found, missing): (bool, bool) = service
                .state
                .demand
                .lock()
                .map_err(error)?
                .query_row("SELECT EXISTS(SELECT 1 FROM resolved_cover WHERE id=?1),EXISTS(SELECT 1 FROM cover_miss WHERE id=?1 AND retry_after_ms>?2)", rusqlite::params![id, now_ms()], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(error)?;
            Ok(if found {
                Some(true)
            } else if missing {
                Some(false)
            } else {
                None
            })
        })
        .await
        .map_err(error)?
    }

    async fn save_cover_miss(&self, id: &str) -> Result<(), MetadataError> {
        let id = id.to_owned();
        run_database(self.clone(), move |service| {
            service
                .state
                .demand
                .lock()
                .map_err(error)?
                .execute("INSERT INTO cover_miss VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET retry_after_ms=excluded.retry_after_ms", rusqlite::params![id, now_ms().saturating_add(MISS_TTL_MS)])
                .map_err(error)?;
            Ok(())
        })
        .await
        .map_err(error)?
    }

    fn save_cover(&self, id: &str, bytes: Vec<u8>) -> Result<Option<CoverSource>, MetadataError> {
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err(MetadataError("invalid artwork size".into()));
        }
        self.state.demand.lock().map_err(error)?.execute("INSERT OR IGNORE INTO resolved_cover VALUES(?1,?2)", rusqlite::params![id, bytes]).map_err(error)?;
        Ok(Some(CoverSource { id: id.to_owned() }))
    }

    async fn persist_cover(&self, id: String, bytes: Vec<u8>) -> Result<Option<CoverSource>, MetadataError> {
        run_database(self.clone(), move |service| service.save_cover(&id, bytes)).await.map_err(error)?
    }

    pub(crate) async fn resolve_audible_cover(&self, asin: &str, region: &str) -> Result<Option<CoverSource>, MetadataError> {
        tokio::time::timeout(COVER_BUDGET, async {
            let id = format!("audible-{region}-{asin}");
            if let Some(found) = self.cover_status(&id).await? {
                return Ok(found.then_some(CoverSource { id }));
            }
            let lock = request_lock(id.clone())?;
            let _request = lock.lock().await;
            if let Some(found) = self.cover_status(&id).await? {
                return Ok(found.then_some(CoverSource { id }));
            }
            let _download = DOWNLOADS.acquire().await.map_err(error)?;
            match self.state.audible.cover(region, asin).await.map_err(MetadataError)? {
                Some(bytes) => self.persist_cover(id, bytes).await,
                None => {
                    self.save_cover_miss(&id).await?;
                    Ok(None)
                }
            }
        })
        .await
        .map_err(|_| MetadataError("cover lookup exceeded its time budget".into()))?
    }

    pub(crate) async fn resolve_isbn_cover(&self, isbns: &[String]) -> Result<Option<CoverSource>, MetadataError> {
        self.resolve_isbn_cover_at(isbns, "https://covers.openlibrary.org").await
    }

    pub(crate) async fn resolve_isbn_cover_at(&self, isbns: &[String], origin: &str) -> Result<Option<CoverSource>, MetadataError> {
        tokio::time::timeout(COVER_BUDGET, self.download_isbn_cover(isbns, origin)).await.map_err(|_| MetadataError("cover lookup exceeded its time budget".into()))?
    }

    async fn download_isbn_cover(&self, isbns: &[String], origin: &str) -> Result<Option<CoverSource>, MetadataError> {
        for isbn in isbns {
            if book_model::canonical_isbn(isbn).as_deref() != Some(isbn.as_str()) {
                return Err(MetadataError("invalid cover ISBN".into()));
            }
            let id = format!("isbn-{isbn}");
            if let Some(found) = self.cover_status(&id).await? {
                if found {
                    return Ok(Some(CoverSource { id }));
                }
                continue;
            }
            let lock = request_lock(format!("{origin}/{id}"))?;
            let _request = lock.lock().await;
            if let Some(found) = self.cover_status(&id).await? {
                if found {
                    return Ok(Some(CoverSource { id }));
                }
                continue;
            }
            let _download = DOWNLOADS.acquire().await.map_err(error)?;
            {
                let mut last = OPEN_LIBRARY.lock().await;
                if let Some(previous) = *last {
                    tokio::time::sleep(Duration::from_secs(1).saturating_sub(previous.elapsed())).await;
                }
                *last = Some(tokio::time::Instant::now());
            }
            let response = HTTP.get(format!("{origin}/b/isbn/{isbn}-L.jpg?default=false")).send().await.map_err(error)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                self.save_cover_miss(&id).await?;
                continue;
            }
            let response = response.error_for_status().map_err(error)?;
            if response.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
                return Err(MetadataError("artwork is too large".into()));
            }
            let mut bytes = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(error)?;
                if bytes.len() + chunk.len() > MAX_BYTES {
                    return Err(MetadataError("artwork is too large".into()));
                }
                bytes.extend_from_slice(&chunk);
            }
            return self.persist_cover(id, bytes).await;
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::tests::snapshot_from;

    #[tokio::test]
    async fn cached_artwork_does_not_wait_for_another_download() {
        let dir = tempfile::tempdir().unwrap();
        let input = snapshot_from(dir.path(), &[], &[], &[]);
        let answers = dir.path().join("answers.sqlite");
        crate::build_answers(&input, None, None, None, &answers).unwrap();
        let service = MetadataService::open(&answers, &dir.path().join("demand.sqlite")).unwrap();
        service.save_cover("isbn-9780306406157", b"retained cover".to_vec()).unwrap();
        let _download = OPEN_LIBRARY.lock().await;
        let cover = tokio::time::timeout(Duration::from_secs(1), service.resolve_isbn_cover_at(&["9780306406157".into()], "http://127.0.0.1:1")).await.unwrap().unwrap().unwrap();
        assert_eq!(cover.id, "isbn-9780306406157");
    }
}
