#[cfg(test)]
use super::DbPool;
use super::error::SyncResult;
use super::library_sync::LibrarySync;
use super::transfer::{TransferJob, TransferOrigin};
#[cfg(test)]
use library_files::asset_store::AssetStore;
use library_replica::BlobKind;
use sync_common::ContentHash;

struct TransferPlanGuard(std::sync::Arc<asset_transfer::TransferQueue>, u64);
impl Drop for TransferPlanGuard {
    fn drop(&mut self) {
        self.0.cancel_reconcile(self.1);
    }
}

use asset_transfer::planning::{AssetPlan, PlanCursor};

impl LibrarySync {
    pub(super) async fn sync_assets(&self) -> SyncResult<()> {
        let user = client_library::sync_state::get_sync_metadata(&*self.database.get()?, "bound_user_id")?;
        let worker = self.transfer_worker();
        worker.queue.resume_authentication();
        let generation = worker.queue.begin_reconcile();
        let _plan = TransferPlanGuard(worker.queue.clone(), generation);
        let mut cursor = PlanCursor::default();
        while let Some(page) = AssetPlan::next_page(&self.database, &self.assets, &mut cursor).await? {
            let requested: Vec<_> = page.book_uploads.iter().chain(&page.thumbnail_uploads).copied().collect();
            let ready = client_library::upload_pipeline::ready_page(&*self.database.get()?, &requested)?;
            let mut page = page;
            page.book_uploads.retain(|hash| ready.contains(hash));
            page.thumbnail_uploads.retain(|hash| ready.contains(hash));
            let mut jobs = if worker.asset_storage_enabled() {
                let conn = self.database.get()?;
                page.book_upload_jobs(&conn)?
            } else {
                Vec::new()
            };
            for hashes in page.thumbnail_uploads.chunks(sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES) {
                jobs.extend(worker.prepare_thumbnail_uploads_with_refresh(hashes).await?);
            }
            if !page.requested_downloads.is_empty() {
                let assets = self.assets.clone();
                let user = user.clone();
                let requested = page.requested_downloads;
                let downloads = self
                    .database
                    .command(library_runtime::storage_queue::Priority::Background, move |conn| {
                        super::ensure_sync_account(conn, &user)?;
                        Ok(requested.iter().map(|hash| Self::prepare_requested_download(&assets, conn, hash)).collect::<SyncResult<Vec<_>>>())
                    })
                    .await??;
                jobs.extend(downloads.into_iter().flatten());
            }
            jobs.extend(page.thumbnail_downloads.into_iter().map(|hash| TransferJob::download_thumbnail(hash, TransferOrigin::Background)));
            super::ensure_sync_account(&*self.database.get()?, &user)?;
            if !jobs.is_empty() {
                worker.clone().run_page(generation, jobs);
            }
        }
        super::ensure_sync_account(&*self.database.get()?, &user)?;
        worker.finish_plan(generation);
        Ok(())
    }

    pub(super) fn prepare_requested_download(assets: &AssetStore, conn: &rusqlite::Connection, hash: &ContentHash) -> SyncResult<Option<TransferJob>> {
        Ok(asset_transfer::planning::prepare_requested_download(assets, conn, hash)?)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    mod snapshot;
    use super::*;
    use sync_common::api::assets::BlobManifestResponse;

    fn queue_local_version(database: &DbPool, hash: &ContentHash) {
        let mut conn = database.get().unwrap();
        let tx = conn.transaction().unwrap();
        client_library::assets::observe_local_placement_publication(&tx, &crate::ROOT_DIR_ID, hash, hash, 10).unwrap();
        tx.commit().unwrap();
    }

    fn fixture() -> (tempfile::TempDir, DbPool, AssetStore, ContentHash) {
        let root = tempfile::tempdir().unwrap();
        let database = DbPool::open_library(root.path().join("library.db").to_str().unwrap(), client_library::schema::configure_connection).unwrap();
        let conn = database.get().unwrap();
        client_library::schema::init_db(&conn).unwrap();
        let hash = test_support::fixture_content_hash(42);
        conn.execute(include_str!("sql/asset_sync/fixture_insert.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("sql/asset_sync/fixture_insert_2.sql"), [hash.as_str()]).unwrap();
        let assets = crate::storage::files::open(root.path().to_str().unwrap()).unwrap().with_database_path(database.path().to_path_buf());
        drop(conn);
        (root, database, assets, hash)
    }

    #[tokio::test]
    async fn received_assets_never_become_uploads_after_reopen() {
        let (root, database, assets, hash) = fixture();
        std::fs::write(root.path().join("book.epub"), b"downloaded publication").unwrap();
        assets.write(BlobKind::Thumbnail, &hash, b"downloaded cover").await.unwrap();
        {
            let mut conn = database.get().unwrap();
            let tx = conn.transaction().unwrap();
            client_library::assets::observe_downloaded_placement_publication(&tx, &crate::ROOT_DIR_ID, &hash, &hash, 22).unwrap();
            client_library::thumbnail_jobs::complete_downloaded_thumbnail(&tx, &hash).unwrap();
            tx.commit().unwrap();
        }
        // Even losing remote availability knowledge must not manufacture local authorship.
        let plan = AssetPlan::load_queued(&database, &assets).await.unwrap();
        assert!(plan.book_uploads.is_empty() && plan.thumbnail_uploads.is_empty());
        drop(database);
        let database = DbPool::open_library(root.path().join("library.db").to_str().unwrap(), client_library::schema::configure_connection).unwrap();
        client_library::schema::init_db(&database.get().unwrap()).unwrap();
        client_library::assets::enqueue_asset_work(&database.get().unwrap(), &hash).unwrap();
        let plan = AssetPlan::load_queued(&database, &assets).await.unwrap();
        assert!(plan.book_uploads.is_empty() && plan.thumbnail_uploads.is_empty());
    }

    #[tokio::test]
    async fn unrelated_file_work_does_not_retain_settled_assets() {
        let (root, database, assets, hash) = fixture();
        std::fs::write(root.path().join("book.epub"), b"local book").unwrap();
        assets.write(BlobKind::Thumbnail, &hash, b"cover").await.unwrap();
        client_library::thumbnail_jobs::complete_thumbnail_work(&database.get().unwrap(), &hash, true).unwrap();
        assets.write_thumbnail_variant(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, b"browse").await.unwrap();
        {
            let conn = database.get().unwrap();
            client_library::assets::record_remote_asset(&conn, BlobKind::Book, &hash).unwrap();
            client_library::assets::record_remote_asset(&conn, BlobKind::Thumbnail, &hash).unwrap();
            client_library::thumbnail_jobs::complete_thumbnail_work(&conn, &hash, true).unwrap();
            conn.execute_batch(include_str!("sql/asset_sync/unrelated_file_work_does_not_retain_settled_assets_delete.sql")).unwrap();
            conn.execute(include_str!("sql/asset_sync/unrelated_file_work_does_not_retain_settled_assets_insert.sql"), []).unwrap();
            assert!(client_library::file_work::has_pending_file_work(&conn).unwrap());
        }
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().is_empty());
        assert!(client_library::assets::asset_work_page(&database.get().unwrap(), 0, i64::MAX).unwrap().is_empty());
        // This book's pending operation must retain its candidate.
        client_library::file_work::queue_book_work(&database.get().unwrap(), &hash).unwrap();
        client_library::assets::enqueue_asset_work(&database.get().unwrap(), &hash).unwrap();
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().is_empty());
        assert_eq!(client_library::assets::asset_work_page(&database.get().unwrap(), 0, i64::MAX).unwrap().len(), 1);
        database.get().unwrap().execute(include_str!("sql/asset_sync/unrelated_file_work_does_not_retain_settled_assets_delete_2.sql"), []).unwrap();
        client_library::file_work::queue_directory_work(&database.get().unwrap(), &crate::ROOT_DIR_ID).unwrap();
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().is_empty());
        assert_eq!(client_library::assets::asset_work_page(&database.get().unwrap(), 0, i64::MAX).unwrap().len(), 1, "work on an ancestor still affects this book");
    }

    #[tokio::test]
    async fn pending_candidates_do_not_require_filesystem_availability() {
        let (_root, database, assets, hash) = fixture();
        queue_local_version(&database, &hash);
        database.get().unwrap().execute(include_str!("sql/asset_sync/pending_candidates_do_not_require_filesystem_availability_update.sql"), []).unwrap();
        client_library::thumbnail_jobs::complete_thumbnail_work(&database.get().unwrap(), &hash, false).unwrap();
        assert!(client_library::assets::has_pending_asset_work(&database.get().unwrap()).unwrap());
        assert!(!assets.exists(BlobKind::Book, &hash).unwrap());
        // The preliminary database hint is cheap; actual planning rejects a
        // stale local-file flag before creating any upload.
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().book_uploads.is_empty());
    }

    #[tokio::test]
    async fn queued_plan_reads_multiple_pages_without_consuming_requests() {
        let root = tempfile::tempdir().unwrap();
        let database = DbPool::open_library(root.path().join("library.db").to_str().unwrap(), client_library::schema::configure_connection).unwrap();
        let assets = crate::storage::files::open(root.path().to_str().unwrap()).unwrap().with_database_path(database.path().to_path_buf());
        let mut expected = Vec::new();
        {
            let conn = database.get().unwrap();
            client_library::schema::init_db(&conn).unwrap();
            for index in 0..95 {
                let hash = ContentHash::new(blake3::hash(index.to_string().as_bytes()).to_hex().as_str());
                conn.execute(include_str!("sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_insert.sql"), [hash.as_str()]).unwrap();
                conn.execute(include_str!("sql/asset_sync/queued_plan_reads_multiple_pages_without_consuming_requests_insert_2.sql"), rusqlite::params![hash.as_str(), format!("{index}.epub")]).unwrap();
                client_library::assets::add_asset_request(&conn, BlobKind::Book, &hash, TransferOrigin::UserInitiated).unwrap();
                expected.push(hash);
            }
        }
        let mut cursor = PlanCursor::default();
        let first = AssetPlan::next_page(&database, &assets, &mut cursor).await.unwrap().unwrap();
        assert_eq!(first.thumbnail_downloads.len(), 32);
        assert_eq!(first.requested_downloads.len(), 32);
        let first_requested = first.requested_downloads.clone();
        let queue = super::super::transfer::TransferQueue::default();
        let generation = queue.begin_reconcile();
        queue.reconcile_page(generation, first.thumbnail_downloads.into_iter().map(|hash| TransferJob::download_thumbnail(hash, TransferOrigin::Background)).collect());
        assert!(queue.claim_due().is_some(), "first-page transfers are executable before reading later pages");
        let second = AssetPlan::next_page(&database, &assets, &mut cursor).await.unwrap().unwrap();
        assert_eq!(second.requested_downloads.len(), 32);
        assert!(second.requested_downloads.iter().all(|hash| !first_requested.contains(hash)));
        let plan = AssetPlan::load_queued(&database, &assets).await.unwrap();
        assert_eq!(plan.thumbnail_downloads.len(), 95);
        assert!(plan.thumbnail_uploads.is_empty());
        expected.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let mut actual = plan.requested_downloads;
        actual.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        assert_eq!(actual, expected);
        assert_eq!(client_library::assets::get_asset_requests(&database.get().unwrap(), BlobKind::Book).unwrap().len(), 95);
    }

    #[tokio::test]
    async fn planning_preserves_requests_until_execution() {
        let (_root, database, assets, hash) = fixture();
        std::fs::write(_root.path().join("book.epub"), b"local book").unwrap();
        client_library::assets::request_book_downloads(&mut database.get().unwrap(), &[hash], TransferOrigin::UserInitiated).unwrap();
        let wrote = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = wrote.clone();
        database.observe_writes(move |_| observed.store(true, std::sync::atomic::Ordering::Relaxed));
        let plan = AssetPlan::load_queued(&database, &assets).await.unwrap();
        assert_eq!(plan.requested_downloads, vec![hash]);
        assert!(!plan.is_empty(), "even a satisfied request needs execution-time cleanup");
        assert!(plan.book_uploads.is_empty(), "file presence and download requests do not create upload intent");
        assert!(!wrote.load(std::sync::atomic::Ordering::Relaxed), "pending-work inspection must not mutate storage on any connection");
        assert!(client_library::assets::is_book_download_requested(&database.get().unwrap(), hash).unwrap());
    }

    #[tokio::test]
    async fn plan_respects_remote_presence_upload_rejection_and_cover_generation() {
        let (_root, database, assets, hash) = fixture();
        let plan = AssetPlan::load_queued(&database, &assets).await.unwrap();
        assert_eq!(plan.thumbnail_downloads, vec![hash]);
        client_library::thumbnail_jobs::request_thumbnail(&database.get().unwrap(), &hash).unwrap();
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().is_empty());
        std::fs::write(_root.path().join("book.epub"), b"local book").unwrap();
        client_library::assets::enqueue_asset_work(&database.get().unwrap(), &hash).unwrap();
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().book_uploads.is_empty());
        queue_local_version(&database, &hash);
        assert_eq!(AssetPlan::load_queued(&database, &assets).await.unwrap().book_uploads, vec![hash]);
        client_library::assets::record_rejected_asset_upload(&database.get().unwrap(), BlobKind::Book, &hash, "forbidden").unwrap();
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().is_empty());
        assets.write(BlobKind::Thumbnail, &hash, b"cover").await.unwrap();
        client_library::thumbnail_jobs::complete_thumbnail_work(&database.get().unwrap(), &hash, true).unwrap();
        assets.write_thumbnail_variant(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, b"browse cover").await.unwrap();
        assert_eq!(AssetPlan::load_queued(&database, &assets).await.unwrap().thumbnail_uploads, vec![hash]);
        client_library::assets::record_remote_asset(&database.get().unwrap(), BlobKind::Thumbnail, &hash).unwrap();
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().is_empty());
    }
    #[tokio::test]
    async fn plan_keeps_book_and_thumbnail_rejections_independent_and_expires_them() {
        let (root, database, assets, hash) = fixture();
        queue_local_version(&database, &hash);
        std::fs::write(root.path().join("book.epub"), b"local book").unwrap();
        assets.write(BlobKind::Thumbnail, &hash, b"cover").await.unwrap();
        client_library::thumbnail_jobs::complete_thumbnail_work(&database.get().unwrap(), &hash, true).unwrap();
        client_library::assets::record_rejected_asset_upload(&database.get().unwrap(), BlobKind::Thumbnail, &hash, "forbidden").unwrap();
        let plan = AssetPlan::load_queued(&database, &assets).await.unwrap();
        assert_eq!(plan.book_uploads, vec![hash]);
        assert!(plan.thumbnail_uploads.is_empty());
        client_library::assets::record_remote_asset(&database.get().unwrap(), BlobKind::Book, &hash).unwrap();
        assert_eq!(AssetPlan::load_queued(&database, &assets).await.unwrap().book_uploads, vec![hash], "remote availability alone does not complete a versioned job");
        {
            let mut conn = database.get().unwrap();
            let tx = conn.transaction().unwrap();
            let intent = client_library::assets::book_upload_intent(&tx, &hash).unwrap().unwrap();
            client_library::assets::complete_book_upload(&tx, &intent).unwrap();
            tx.commit().unwrap();
        }
        database.get().unwrap().execute(include_str!("sql/asset_sync/plan_keeps_book_and_thumbnail_rejections_independent_and_expires_them_update.sql"), []).unwrap();
        let plan = AssetPlan::load_queued(&database, &assets).await.unwrap();
        assert!(plan.book_uploads.is_empty());
        assert_eq!(plan.thumbnail_uploads, vec![hash]);

        // A known remote thumbnail can supply a missing browse variant, except
        // while generation is pending (including a delayed generation retry).
        client_library::assets::record_remote_asset(&database.get().unwrap(), BlobKind::Thumbnail, &hash).unwrap();
        assert_eq!(AssetPlan::load_queued(&database, &assets).await.unwrap().thumbnail_downloads, vec![hash]);
        client_library::thumbnail_jobs::retry_thumbnail_work(&database.get().unwrap(), &hash).unwrap();
        assert!(AssetPlan::load_queued(&database, &assets).await.unwrap().thumbnail_downloads.is_empty());
        client_library::thumbnail_jobs::complete_thumbnail_work(&database.get().unwrap(), &hash, false).unwrap();
        assert_eq!(AssetPlan::load_queued(&database, &assets).await.unwrap().thumbnail_downloads, vec![hash]);
    }

    #[tokio::test]
    async fn manifest_response_is_applied_only_to_its_bound_account() {
        use axum::{Router, body::Body, http::Response, routing::post};
        use std::sync::Arc;
        for (change_account, change_version) in [(false, false), (true, false), (false, true)] {
            let (root, database, assets, _) = fixture();
            let bytes = b"manifest account regression";
            let hash = ContentHash::new(blake3::hash(bytes).to_hex().as_str());
            {
                let conn = database.get().unwrap();
                conn.execute(include_str!("sql/asset_sync/manifest_response_is_applied_only_to_its_bound_account_update.sql"), [hash.as_str()]).unwrap();
                client_library::sync_state::set_sync_metadata(&conn, "bound_user_id", "original-user").unwrap();
                client_library::thumbnail_jobs::request_thumbnail(&conn, &hash).unwrap();
            }
            std::fs::write(root.path().join("book.epub"), bytes).unwrap();
            {
                let mut conn = database.get().unwrap();
                let tx = conn.transaction().unwrap();
                client_library::assets::observe_local_placement_publication(&tx, &crate::ROOT_DIR_ID, &hash, &hash, bytes.len() as u64).unwrap();
                tx.commit().unwrap();
            }
            let started = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let route_started = started.clone();
            let route_release = release.clone();
            let library_id = test_support::fixture_library_id("manifest-account");
            let route = format!("/api/libraries/{library_id}/blobs/negotiate");
            let app = Router::new().route(
                &route,
                post(move || {
                    let started = route_started.clone();
                    let release = route_release.clone();
                    async move {
                        started.notify_one();
                        release.notified().await;
                        let body = sync_common::transport::encode(&BlobManifestResponse { owned: vec![hash], ..Default::default() }).unwrap();
                        Response::builder().header("content-type", sync_common::transport::MEDIA_TYPE).body(Body::from(body)).unwrap()
                    }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let server_url = crate::ServerUrl::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let credentials = Arc::new(std::sync::RwLock::new(Some(sync_transport::SyncCredentials::new(server_url.clone(), "manifest-token"))));
            let sync = LibrarySync::new(super::super::library_sync::LibrarySyncConfig {
                account: None,
                notification_interest: Default::default(),
                database: database.clone(),
                assets,
                server_url,
                credentials,
                library_id,
                replica_id: sync_common::ReplicaId::parse_str("00000000-0000-4000-8000-000000000002").unwrap(),
                event_tx: None,
                transfer_history_path: root.path().join("history.msgpack"),
            });
            let change = async {
                started.notified().await;
                if change_account {
                    client_library::sync_state::rebind_sync_account(&database.get().unwrap(), "replacement-user").unwrap();
                }
                if change_version {
                    let mut conn = database.get().unwrap();
                    let tx = conn.transaction().unwrap();
                    let newer = test_support::fixture_content_hash(99);
                    client_library::assets::observe_local_placement_publication(&tx, &crate::ROOT_DIR_ID, &hash, &newer, bytes.len() as u64 + 1).unwrap();
                    tx.commit().unwrap();
                }
                release.notify_one();
            };
            let worker = sync.transfer_worker();
            let hashes = [hash];
            let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async { tokio::join!(worker.prepare_book_uploads(&hashes), change) }).await.unwrap();
            if change_account {
                assert!(result.unwrap_err().to_string().contains("account changed"));
            } else {
                result.unwrap();
            }
            assert_eq!(client_library::assets::get_remote_asset_hashes(&database.get().unwrap(), BlobKind::Book).unwrap().contains(&hash), !change_account && !change_version);
            assert_eq!(client_library::assets::book_upload_intent(&database.get().unwrap(), &hash).unwrap().is_some(), change_account || change_version, "only confirmation for the bound account and exact version completes durable intent");
            server.abort();
        }
    }
}

#[cfg(test)]
trait TestAssetPlan {
    fn is_empty(&self) -> bool;
    async fn load_queued(database: &DbPool, assets: &AssetStore) -> SyncResult<Self>
    where
        Self: Sized;
}
#[cfg(test)]
impl TestAssetPlan for AssetPlan {
    fn is_empty(&self) -> bool {
        self.book_uploads.is_empty() && self.thumbnail_uploads.is_empty() && self.thumbnail_downloads.is_empty() && self.requested_downloads.is_empty()
    }

    // Tests can collect the production pages to assert complete work sets.
    async fn load_queued(database: &DbPool, assets: &AssetStore) -> SyncResult<Self> {
        let mut cursor = PlanCursor::default();
        let mut plan = Self::default();
        while let Some(mut page) = Self::next_page(database, assets, &mut cursor).await? {
            plan.book_uploads.append(&mut page.book_uploads);
            plan.thumbnail_uploads.append(&mut page.thumbnail_uploads);
            plan.thumbnail_downloads.append(&mut page.thumbnail_downloads);
            plan.requested_downloads.append(&mut page.requested_downloads);
        }
        Ok(plan)
    }
}
