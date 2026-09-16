//! Native directory placement and durable filesystem reconciliation.
use super::{AssetStore, AssetStoreError, imp};
use crate::ContentHash;

/// Native directory placement is separate from shared asset byte access.
pub struct NativePlacement<'a> { pub(super) store: &'a AssetStore }

impl NativePlacement<'_> {
    pub fn root_available(&self) -> bool {
        imp::root_available(&self.store.0)
    }
    pub fn directory_available(&self, relative: &str) -> bool {
        imp::directory_available(&self.store.0, relative)
    }

    pub fn prepare_directory(&self, conn: &mut rusqlite::Connection, id: crate::DirId) -> Result<(), AssetStoreError> {
        imp::prepare_directory(&self.store.0, conn, id)
    }

    pub fn queue_book_placements(&self, conn: &rusqlite::Connection, hash: &ContentHash) -> Result<(), AssetStoreError> {
        imp::queue_book_placements(&self.store.0, conn, hash)
    }

    pub fn purge_trash(&self, hash: &ContentHash) -> Result<(), AssetStoreError> {
        imp::purge_trash(&self.store.0, hash)
    }
}

impl NativePlacement<'_> {
    /// Replay metadata-committed file work. Each step is idempotent; failures
    /// leave the record queued, including a restore awaiting server bytes.
    pub fn reconcile_file_work(&self, conn: &mut rusqlite::Connection) -> Result<(), AssetStoreError> {
        // Snapshot once: work queued during publication belongs to a later pass.
        let jobs = conn
            .prepare(include_str!("sql/native_placement/reconcile_file_work_select.sql"))?
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut failure = None;
        for (id, operation, hash, original_path) in jobs {
            if let Err(error) = self.reconcile_file_job(conn, id, operation, hash, original_path) {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn reconcile_file_job(&self, conn: &mut rusqlite::Connection, id: i64, operation: String, hash: String, original_path: String) -> Result<(), AssetStoreError> {
        // Deferred or failed work must not let a later operation retire this
        // book's source. The durable queue preserves ordering across passes.
        let preceding: bool = conn.query_row(include_str!("sql/native_placement/reconcile_file_job_select.sql"), rusqlite::params![hash, id], |row| row.get(0))?;
        if preceding {
            return Ok(());
        }
        let Some(_lease) = imp::lease(&self.store.0, &hash, true)? else { return Ok(()) };
        let hash = ContentHash::new(&hash);
        let original = crate::RelativeBookPath::parse(&original_path).map_err(|e| AssetStoreError(e.to_string()))?;
        let mut path = original_path.clone();
        let mut renamed = None;
        // Resolve filesystem collisions and stage publication bytes without
        // holding SQLite's writer. Metadata is rechecked before publication.
        let prepared = if operation == "trash" {
            let live: bool = conn.query_row(include_str!("sql/native_placement/reconcile_file_job_select_2.sql"), [hash.as_str()], |r| r.get(0))?;
            if live && client_library::assets::get_book_dir_entry_by_path(conn, &original)?.is_some_and(|(actual, _, _)| actual == hash) { None } else { Some(imp::prepare_trash(&self.store.0, &hash, &path)?) }
        } else if operation == "restore" {
            let live: bool = conn.query_row(include_str!("sql/native_placement/reconcile_file_job_select_3.sql"), [hash.as_str()], |r| r.get(0))?;
            let placement = if live { client_library::assets::get_book_dir_entry_by_path(conn, &original)? } else { None };
            if let Some((_, dir, name)) = placement.filter(|(actual, _, _)| *actual == hash) {
                let mut occupied = client_library::placements::occupied_file_names(conn, &dir.to_string())?;
                let parent = path.rsplit_once('/').map(|(parent, _)| parent.to_owned()).unwrap_or_default();
                while imp::placement_conflicts(&self.store.0, &hash, &path)? {
                    let chosen = library_model::unique_file_name(&name, occupied.iter().map(String::as_str));
                    occupied.push(chosen.clone());
                    path = format!("{parent}/{chosen}");
                    renamed = Some((dir, chosen));
                }
                let Some(prepared) = imp::prepare_restore(&self.store.0, &hash, &path, conn)? else { return Ok(()) };
                Some(prepared)
            } else {
                None
            }
        } else {
            return Err(AssetStoreError(format!("unknown file operation: {operation}")));
        };
        let tx = client_library::schema::library_write_transaction(conn)?;
        let current: bool =
            tx.query_row(include_str!("sql/native_placement/reconcile_file_job_select_5.sql"), rusqlite::params![id, operation, hash.as_str(), original_path], |row| row.get(0))?;
        if !current {
            return Ok(());
        }
        let live: bool = tx.query_row(include_str!("sql/native_placement/reconcile_file_job_select_3.sql"), [hash.as_str()], |r| r.get(0))?;
        let placed = live && client_library::assets::get_book_dir_entry_by_path(&tx, &original)?.is_some_and(|(actual, _, _)| actual == hash);
        let needed = if operation == "trash" { !(live && placed) } else { live && placed };
        if needed {
            // A previously obsolete job may have become relevant while
            // preparing another operation. Leave it queued for a fresh pass.
            let Some(prepared) = prepared else { return Ok(()) };
            if let Some((dir, chosen)) = renamed {
                let occupied: bool = tx.query_row(include_str!("sql/native_placement/reconcile_file_job_select_7.sql"), rusqlite::params![dir.to_string(), chosen], |r| r.get(0))?;
                if occupied {
                    return Ok(());
                }
                tx.execute(include_str!("sql/asset_store/publish_update.sql"), rusqlite::params![dir.to_string(), hash.as_str(), chosen])?;
            }
            prepared.publish()?;
            if operation == "trash" {
                tx.execute(include_str!("sql/native_placement/reconcile_file_job_delete.sql"), rusqlite::params![hash.as_str(), original_path])?;
            }
            if operation == "restore" {
                let relative = crate::RelativeBookPath::parse(&path).map_err(|e| AssetStoreError(e.to_string()))?;
                client_library::placements::set_book_downloaded(&tx, &relative, hash, true)?;
                client_library::assets::complete_asset_request(&tx, crate::BlobKind::Book, &hash)?;
                if !self.store.thumbnail_set_exists(&hash)? {
                    client_library::thumbnail_jobs::request_missing_thumbnail(&tx, &hash)?;
                }
                if let Some((_, dir, _)) = client_library::assets::get_book_dir_entry_by_path(&tx, &relative)? {
                    tx.execute(
                        include_str!("sql/asset_store/publish_insert.sql"),
                        rusqlite::params![hash.as_str(), dir.to_string(), path],
                    )?;
                }
            }
        }
        tx.execute(include_str!("sql/native_placement/reconcile_file_job_delete_2.sql"), [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Prepare recorded folder/book changes before replaying file operations.
    /// Folder moves rewrite remembered source paths, so a preparation failure
    /// must prevent source retirement until those dependencies are resolved.
    pub fn recover_files(&self, conn: &mut rusqlite::Connection) -> Result<(), AssetStoreError> {
        imp::prepare_directory_work(&self.store.0, conn)?;
        self.reconcile_file_work(conn)?;
        Ok(())
    }

    pub async fn run_jobs(&self, database: &impl crate::database::FileDatabase) -> Result<(), AssetStoreError> {
            let database = database.clone();
            let assets = self.store.clone();
            crate::executor::run_blocking(move || {
                let mut conn = database.get().map_err(|e| AssetStoreError(e.to_string()))?;
                let result = assets.placement().recover_files(&mut conn);
                let error = result.as_ref().err().map(ToString::to_string).unwrap_or_default();
                if client_library::sync_state::get_sync_metadata(&conn, "local_file_work_error")?.as_deref().unwrap_or_default() != error {
                    client_library::sync_state::set_sync_metadata(&conn, "local_file_work_error", &error)?;
                }
                result
            })
            .await
            .map_err(|error| AssetStoreError(error.to_string()))
            .and_then(|result| result)
    }
}
