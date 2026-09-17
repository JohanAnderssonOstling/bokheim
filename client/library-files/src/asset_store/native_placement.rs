//! Native directory placement and durable filesystem reconciliation.
use super::{AssetStore, AssetStoreError, imp};
use sync_common::{ContentHash, DirId};

/// Native directory placement is separate from shared asset byte access.
pub struct NativePlacement<'a> { pub(super) store: &'a AssetStore }

impl NativePlacement<'_> {
    pub fn root_available(&self) -> bool {
        imp::root_available(&self.store.0)
    }
    pub fn directory_available(&self, relative: &str) -> bool {
        imp::directory_available(&self.store.0, relative)
    }

    /// Creates/moves one physical directory from a database-owned snapshot.
    pub async fn prepare_directory_with_database(&self, database: &library_database::Database, id: DirId) -> Result<(), AssetStoreError> {
        imp::prepare_directory_with_database(&self.store.0, database, id).await
    }

    pub fn purge_trash(&self, hash: &ContentHash) -> Result<(), AssetStoreError> {
        imp::purge_trash(&self.store.0, hash)
    }
}

impl NativePlacement<'_> {
    /// Replay metadata-committed file work. Database snapshots and
    /// acknowledgements are short; all filesystem work between them is
    /// connection-free.
    pub async fn reconcile_file_work(&self, database: &library_database::Database) -> Result<(), AssetStoreError> {
        let jobs = database.native_file_work().map_err(|error| AssetStoreError(error.to_string()))?;
        let movable = Self::movable_sources(&jobs);
        let mut failure = None;
        for work in jobs {
            if let Err(error) = self.reconcile_file_job(database, work, &movable).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Same as [`Self::reconcile_file_work`], scoped to one book so a reader
    /// never blocks on an unrelated large copy.
    pub async fn reconcile_file_work_for(&self, database: &library_database::Database, hash: &ContentHash) -> Result<(), AssetStoreError> {
        let jobs: Vec<_> = database.native_file_work().map_err(|error| AssetStoreError(error.to_string()))?.into_iter().filter(|work| &work.content_hash == hash).collect();
        let movable = Self::movable_sources(&jobs);
        let mut failure = None;
        for work in jobs {
            if let Err(error) = self.reconcile_file_job(database, work, &movable).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Paths a queued `trash` job already claims: safe for a paired
    /// `restore` to relocate instead of copy.
    fn movable_sources(jobs: &[library_database::NativeFileWork]) -> std::collections::HashMap<ContentHash, Vec<String>> {
        let mut movable = std::collections::HashMap::<ContentHash, Vec<String>>::new();
        for job in jobs.iter().filter(|job| job.operation == "trash") {
            movable.entry(job.content_hash).or_default().push(job.original_path.clone());
        }
        movable
    }

    async fn reconcile_file_job(&self, database: &library_database::Database, work: library_database::NativeFileWork, movable: &std::collections::HashMap<ContentHash, Vec<String>>) -> Result<(), AssetStoreError> {
        let Some(snapshot) = database.native_file_work_snapshot(work).map_err(|error| AssetStoreError(error.to_string()))? else { return Ok(()) };
        let Some(_lease) = imp::lease(&self.store.0, snapshot.work.content_hash.as_str(), true)? else { return Ok(()) };
        let hash = snapshot.work.content_hash;
        let mut path = snapshot.resolved_path.clone();
        let mut renamed = None;
        let prepared = if snapshot.work.operation == "trash" {
            if snapshot.live && snapshot.placement.is_some() { None } else { Some(imp::prepare_trash(&self.store.0, &hash, &path)?) }
        } else if snapshot.work.operation == "restore" {
            if let Some((dir, name)) = snapshot.placement {
                let mut occupied = snapshot.occupied_names;
                let parent = path.rsplit_once('/').map(|(parent, _)| parent.to_owned()).unwrap_or_default();
                while imp::placement_conflicts(&self.store.0, &hash, &path)? {
                    let chosen = library_replica::unique_file_name(&name, occupied.iter().map(String::as_str));
                    occupied.push(chosen.clone());
                    path = format!("{parent}/{chosen}");
                    renamed = Some((dir.clone(), chosen));
                }
                let empty = Vec::new();
                let movable_sources = movable.get(&hash).unwrap_or(&empty);
                let Some(prepared) = imp::prepare_restore(&self.store.0, &hash, &path, snapshot.expected_checksum.as_deref(), &snapshot.book_paths, movable_sources)? else { return Ok(()) };
                Some(prepared)
            } else {
                None
            }
        } else {
            return Err(AssetStoreError(format!("unknown file operation: {}", snapshot.work.operation)));
        };
        let published = if let Some(prepared) = prepared {
            prepared.publish()?;
            true
        } else {
            false
        };
        let acknowledgement = library_database::NativeFileWorkCompletion {
            work: snapshot.work,
            final_path: path,
            renamed,
            published,
            thumbnail_set_exists: self.store.thumbnail_set_exists(&hash)?,
        };
        let _ = database.acknowledge_native_file_work(acknowledgement).map_err(|error| AssetStoreError(error.to_string()))?;
        Ok(())
    }

    /// Prepare recorded folder/book changes before replaying file operations.
    /// Folder moves rewrite remembered source paths, so a preparation failure
    /// must prevent source retirement until those dependencies are resolved.
    pub async fn run_jobs(&self, database: &library_database::Database) -> Result<(), AssetStoreError> {
        self.run_directory_work(database).await?;
        self.reconcile_file_work(database).await
    }

    /// Replays every queued directory move/rename; cheap since each is a
    /// same-volume `fs::rename`, unlike file work.
    pub async fn run_directory_work(&self, database: &library_database::Database) -> Result<(), AssetStoreError> {
        let mut directories = database.native_directory_work_ids().map_err(|error| AssetStoreError(error.to_string()))?;
        directories.extend(database.native_book_work_directory_ids().map_err(|error| AssetStoreError(error.to_string()))?);
        directories.sort();
        directories.dedup();
        for directory in directories {
            self.prepare_directory_with_database(database, directory).await?;
        }
        Ok(())
    }

    // Small-schema regression fixtures predate the concrete database owner.
    // Keep their physical replay assertion local to tests; production callers
    // exclusively use the snapshot/acknowledgement API above.
    #[cfg(test)]
    pub fn reconcile_file_work_fixture(&self, conn: &mut rusqlite::Connection) -> Result<(), AssetStoreError> {
        let jobs = conn.prepare("SELECT id,operation,content_hash,relative_path FROM local_file_work ORDER BY id")?
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut failure = None;
        for (id, operation, hash, path) in jobs {
            let preceding: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM local_file_work WHERE content_hash=?1 AND id<?2)", rusqlite::params![hash, id], |row| row.get(0))?;
            if preceding { continue; }
            let Some(_lease) = imp::lease(&self.store.0, &hash, true)? else { continue; };
            if operation == "trash" {
                match imp::prepare_trash(&self.store.0, &ContentHash::new(&hash), &path).and_then(|prepared| prepared.publish()) {
                    Ok(()) => {
                        conn.execute("DELETE FROM local_file_projection WHERE content_hash=?1 AND relative_path=?2", rusqlite::params![hash, path])?;
                        conn.execute("DELETE FROM local_file_work WHERE id=?1", [id])?;
                    }
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
            } else {
                conn.execute("DELETE FROM local_file_work WHERE id=?1", [id])?;
            }
        }
        failure.map_or(Ok(()), Err)
    }
}
