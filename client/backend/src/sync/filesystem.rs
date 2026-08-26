use super::DbPool;
use crate::library::database as db;
use crate::{BlobKind, ContentHash, RelativeBookPath, RelativeDirPath};
use std::path::Path;
use sync_common::DirId;

/// Replays filesystem effects that were committed with a pulled sync page.
///
/// Keeping this separate from network exchange makes the durability rule
/// explicit: the database cursor and owed effects commit together, while this
/// projector performs idempotent disk work after that commit.
#[derive(Clone, Copy)]
pub(super) struct FilesystemProjector<'a> {
    db_pool: &'a DbPool,
    library_root: &'a Path,
}

impl<'a> FilesystemProjector<'a> {
    pub(super) fn new(db_pool: &'a DbPool, library_root: &'a Path) -> Self {
        Self { db_pool, library_root }
    }

    pub(super) fn run_owed_actions(self) {
        let Ok(conn) = self.db_pool.get() else {
            return;
        };
        let owed = match db::pending_fs_actions(&conn) {
            Ok(owed) => owed,
            Err(error) => {
                log::warn!("Could not read the pending filesystem actions: {error}");
                return;
            }
        };
        for (id, action) in owed {
            match action {
                Some(action) => {
                    if let Err(error) = self.run_action(&conn, &action) {
                        log::warn!("Synchronized filesystem action {action:?} did not complete: {error}");
                    }
                }
                None => log::error!("Discarding an unreadable pending filesystem action so the work queued behind it can proceed"),
            }
            if let Err(error) = db::complete_fs_action(&conn, id) {
                log::warn!("Could not retire a completed filesystem action: {error}");
                return;
            }
        }
    }

    pub(super) fn run_action(self, conn: &rusqlite::Connection, action: &db::FsAction) -> Result<(), Box<dyn std::error::Error>> {
        match action {
            db::FsAction::MaterializeDir { dir_id } => {
                DirId::parse_str(dir_id)?;
                db::materialize_dir_with_markers(conn, self.library_root, dir_id)?;
                Ok(())
            }
            db::FsAction::MoveDir { dir_id, old_path, new_path } => self.move_remote_dir(conn, dir_id, old_path, new_path),
            db::FsAction::RetirePlacementFile { dir_id, content_hash } => self.retire_placement_file(conn, &DirId::parse_str(dir_id)?, &content_hash.parse::<ContentHash>()?),
            db::FsAction::RemoveDir { path } => {
                self.remove_remote_dir(&RelativeDirPath::parse(path)?);
                Ok(())
            }
        }
    }

    fn move_remote_dir(self, conn: &rusqlite::Connection, dir_id: &str, old_path: &str, new_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        DirId::parse_str(dir_id)?;
        let old_abs = RelativeDirPath::parse(old_path)?.resolve_under(self.library_root);
        let new_abs = RelativeDirPath::parse(new_path)?.resolve_under(self.library_root);
        if !old_abs.exists() {
            db::materialize_dir_with_markers(conn, self.library_root, dir_id)?;
            return Ok(());
        }
        db::materialize_dir_parent_with_markers(conn, self.library_root, dir_id)?;
        std::fs::rename(&old_abs, &new_abs)?;
        db::materialize_dir_with_markers(conn, self.library_root, dir_id)?;
        Ok(())
    }

    pub(super) fn remove_remote_dir(self, old_path: &RelativeDirPath) {
        let abs = old_path.resolve_under(self.library_root);
        let marker = abs.join(".biblos_uuid");
        let marker_content = std::fs::read(&marker).ok();
        if marker_content.is_some() {
            match std::fs::remove_file(&marker) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!("Failed to remove marker {:?}: {}", marker, error),
            }
        }
        match std::fs::remove_dir(&abs) {
            Ok(()) => log::info!("Removed remote-deleted dir {:?}", abs),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                log::warn!("Failed to remove remote-deleted dir {:?}: {}", abs, error);
                if let Some(content) = marker_content {
                    let _ = std::fs::write(&marker, content);
                }
            }
        }
    }

    fn retire_placement_file(self, conn: &rusqlite::Connection, dir_id: &DirId, placement_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        let Some((rel_path, content_hash)) = db::resolve_removed_book_dir(conn, &dir_id.to_string(), *placement_hash)? else {
            return Ok(());
        };
        let abs_old = self.resolve_book_path(&rel_path);
        if !Path::new(&abs_old).exists() {
            return Ok(());
        }

        if let Some(content_hash) = content_hash {
            if let Some(target) = self.live_placement_missing_file(conn, &content_hash)? {
                let target_rel = target.rel_path;
                let abs_new = self.resolve_book_path(&target_rel);
                let mut target_ready = true;
                if let Some((_, dir_id, _)) = db::get_book_dir_entry_by_path(conn, &target_rel)? {
                    if let Err(error) = db::materialize_dir_with_markers(conn, self.library_root, &dir_id.to_string()) {
                        log::warn!("Failed to materialize relocation target {}: {}", target_rel, error);
                        target_ready = false;
                    }
                } else if let Some(parent) = Path::new(&abs_new).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if target_ready && std::fs::rename(&abs_old, &abs_new).is_ok() {
                    if let Err(error) = db::stamp_book_dir_local_hash_by_rel_path(conn, &target_rel, target.content_hash, Path::new(&abs_new)) {
                        log::warn!("Failed to stamp relocated fingerprint for {}: {}", abs_new, error);
                    }
                    db::upsert_blob_local_ref(conn, BlobKind::Book, &content_hash, &abs_new, true)?;
                    db::complete_asset_request(conn, BlobKind::Book, &content_hash)?;
                    db::set_book_downloaded(conn, &target_rel, target.content_hash, true)?;
                    log::info!("Relocated {abs_old} -> {abs_new} (book moved)");
                    return Ok(());
                }
            }
        }

        match std::fs::remove_file(&abs_old) {
            Ok(()) => log::info!("Removed {abs_old} after synchronized deletion"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!("Failed to remove {abs_old} after synchronized deletion: {error}"),
        }
        Ok(())
    }

    fn live_placement_missing_file(self, conn: &rusqlite::Connection, content_hash: &ContentHash) -> Result<Option<db::BookPath>, rusqlite::Error> {
        Ok(db::get_book_paths_by_content_hash(conn, content_hash)?.into_iter().find(|path| !Path::new(&self.resolve_book_path(&path.rel_path)).exists()))
    }

    fn resolve_book_path(self, rel_path: &RelativeBookPath) -> String {
        rel_path.resolve_under(self.library_root).to_string_lossy().into_owned()
    }
}
