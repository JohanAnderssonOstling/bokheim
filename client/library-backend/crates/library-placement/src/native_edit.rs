//! Interactive placement I/O. Slow preparation never holds SQLite's writer.
use crate::{crosses_devices, LibraryState};
use client_protocol::BackendError;
use library_database::{executor::run_blocking, DatabaseError, NativePlacementEdit, NativePlacementPlan, NativePlacementResult};
use library_files::asset_store::{PreparedDirectory, PreparedPlacement};
use library_files::AssetStore;
use std::{mem, sync::Arc};
use sync_common::DirId;

struct PreparedEdit {
    directories: Vec<PreparedDirectory>,
    files: Vec<PreparedPlacement>,
    materialized_files: Vec<String>,
    materialized_directories: Vec<DirId>,
}

impl PreparedEdit {
    fn prepare(assets: &AssetStore, plan: &NativePlacementPlan, copy_move: bool) -> Result<Self, BackendError> {
        if !assets.root_available() {
            return Err(BackendError::message("library storage is unavailable"));
        }
        let mut prepared = Self { directories: Vec::new(), files: Vec::new(), materialized_files: Vec::new(), materialized_directories: Vec::new() };
        if let NativePlacementResult::CopiedDirectory(id) = plan.result {
            let root = plan.directories.iter().find(|directory| directory.id == id).ok_or("copied folder has no filesystem path")?;
            let directories = plan.directories.iter().map(|directory| (directory.id, directory.relative_path.as_str()));
            let books = plan.files.iter().filter(|file| file.work.operation == "materialize").map(|file| (file.work.book_id, &file.resolved_path, file.book_paths.as_slice()));
            let (tree, files) = assets.prepare_directory_copy(&root.relative_path, directories, books)?;
            prepared.files.push(tree);
            prepared.materialized_files = files;
            prepared.materialized_directories.extend(plan.directories.iter().map(|directory| directory.id));
            return Ok(prepared);
        }
        for directory in &plan.directories {
            prepared.directories.push(assets.inspect_directory(directory.id, &directory.relative_path, directory.projected_path.as_deref(), &directory.name, &directory.sibling_names)?);
            prepared.materialized_directories.push(directory.id);
        }
        for file in plan.files.iter().filter(|file| file.work.operation == "materialize") {
            let movable = plan.files.iter().filter(|other| !copy_move && other.work.operation == "remove" && other.work.source_root.is_none() && other.work.book_id == file.work.book_id).filter_map(|other| other.work.cleanup_path.as_ref());
            if let Some(output) = assets.prepare_placement(&file.work.book_id, &file.resolved_path, &file.book_paths, movable)? {
                prepared.files.push(output);
                prepared.materialized_files.push(file.resolved_path.to_string());
            }
        }
        Ok(prepared)
    }

    fn publish(self) -> Result<(), DatabaseError> {
        for directory in self.directories {
            match directory.publish_exact().map_err(DatabaseError::operation)? {
                Some(result) if result.renamed_to.is_none() => {}
                _ => return Err(DatabaseError::message("destination folder changed during the operation; please try again")),
            }
        }
        for file in self.files {
            file.publish_short().map_err(DatabaseError::operation)?;
        }
        Ok(())
    }
}

impl LibraryState {
    pub(super) async fn edit_native(&self, edit: NativePlacementEdit) -> Result<NativePlacementResult, BackendError> {
        let mut plan = self.database().plan_native_placement(edit).await?;
        // Leases coordinate the source bytes with background eviction and
        // replay while the actor remains available for queries and other work.
        let mut hashes = plan.files.iter().map(|file| file.work.book_id).collect::<Vec<_>>();
        hashes.sort();
        hashes.dedup();
        let mut leases = Vec::new();
        for hash in hashes {
            let copying = matches!(plan.result, NativePlacementResult::CopiedBook(_) | NativePlacementResult::CopiedDirectory(_) | NativePlacementResult::RestoredBookPlacement);
            leases.push(if copying { self.assets().lease_book(&hash).await? } else { self.assets().try_exclusive_lease(&hash)?.ok_or("book is currently in use; please try again")? });
        }
        // Blocking preparation and publication can outlive the caller. Keep
        // their root and book leases until each dispatched operation finishes.
        let access = Arc::new((self.assets().lock_root_shared().await, leases));
        for copy_move in [false, true] {
            let assets = self.assets().clone();
            let preparation_access = access.clone();
            let (mut prepared, returned_plan) = run_blocking(move || {
                let _access = preparation_access;
                PreparedEdit::prepare(&assets, &plan, copy_move).map(|prepared| (prepared, plan))
            })
            .await
            .map_err(BackendError::operation)??;
            plan = returned_plan;
            if !prepared.files.iter().any(PreparedPlacement::renames_source) {
                if let Some(source) = plan.move_source.clone() {
                    let assets = self.assets().clone();
                    let retirement_access = access.clone();
                    let retirement = run_blocking(move || {
                        let _access = retirement_access;
                        assets.prepare_move_source_staging(&source.book, &source.physical_path)
                    })
                    .await
                    .map_err(BackendError::operation)??;
                    if let Some((retirement, staged)) = retirement {
                        self.database().record_native_move_staging(&mut plan, &staged).await?;
                        // If publication or the final commit fails, the existing
                        // durable replay jobs restore the source instead.
                        self.events.files.wake();
                        retirement.publish_short()?;
                    }
                }
            }
            let files = mem::take(&mut prepared.materialized_files);
            let directories = mem::take(&mut prepared.materialized_directories);
            let assets = self.assets().clone();
            let publication_access = access.clone();
            match self
                .database()
                .commit_native_placement(plan.clone(), files, directories, move || {
                    let _access = publication_access;
                    if !assets.root_available() {
                        return Err(DatabaseError::message("library storage is unavailable"));
                    }
                    prepared.publish()
                })
                .await
            {
                Err(error) if !copy_move && crosses_devices(&error) && matches!(plan.result, NativePlacementResult::TransferredBook { .. }) => continue,
                result => {
                    let result = result?;
                    self.events.notify_contents_changed();
                    return Ok(result);
                }
            }
        }
        unreachable!("second attempt returns its result")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_inspection::InspectedBook;
    use book_model::{BookDetails, BookFormat, BookRecord};
    use library_database::{executor::BackendExecutor, DatabaseClient, Database, ImportCommit};
    use library_model::PublishedStagedBook;
    use library_services::events::LibraryEventRouter;
    use std::fs;
    use sync_common::{BookId, ROOT_DIR_ID};
    use tempfile::TempDir;

    const BYTES: &[u8] = b"verified local book bytes";

    async fn fixture() -> (TempDir, std::sync::Arc<tokio::sync::RwLock<LibraryState>>, BookId, DirId) {
        let root = TempDir::new().unwrap();
        let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let database = DatabaseClient::open(root.path().join("state.sqlite"), &BackendExecutor::new().unwrap()).await.unwrap();
        database.initialize_library().await.unwrap();

        let (events, _) = LibraryEventRouter::new();
        let placement = std::sync::Arc::new(tokio::sync::RwLock::new(LibraryState::new(database, assets, events)));
        let folder = placement.write().await.create_directory(&ROOT_DIR_ID, "Shelf").unwrap().id;
        let hash = BookId::from_bytes(*blake3::hash(BYTES).as_bytes());
        fs::write(root.path().join("book.epub"), BYTES).unwrap();
        placement
            .read()
            .await
            .database()
            .commit_import(ImportCommit {
                source_identity: None,
                byte_checksum: None,
                audiobook_content_hash: None,
                parent_id: ROOT_DIR_ID,
                file_name: "book.epub".into(),
                format: BookFormat::Epub,
                book_id: hash,
                size_bytes: BYTES.len() as u64,
                inspection_version: 1,
                inspection: Some(InspectedBook { pdf: None, audiobook: None, metadata: BookRecord { metadata: BookDetails { title: "Book".into(), ..Default::default() }, description: String::new() }, toc: vec![] }),
                published: PublishedStagedBook { name: "book.epub".into(), relative_path: "/book.epub".into(), published: true, fingerprint: None },
                request_thumbnail: false,
                restore_paths: vec![],
            })
            .await
            .unwrap();
        (root, placement, hash, folder)
    }

    #[tokio::test]
    async fn copy_staging_keeps_placement_unchanged_and_releases_database_writer() {
        let (root, placement, book, destination) = fixture().await;
        let original = placement.read().await.database().book_paths(&book).await.unwrap();
        let plan = placement.read().await.database().plan_native_placement(NativePlacementEdit::CopyBook { book, source: Some(ROOT_DIR_ID), destination }).await.unwrap();
        let staged = PreparedEdit::prepare(&placement.read().await.assets(), &plan, false).unwrap();
        assert_eq!(placement.read().await.database().book_paths(&book).unwrap(), original);
        assert!(!root.path().join("Shelf/book.epub").exists());
        // An independent writer succeeds while copied bytes are staged.
        let other = Database::open(root.path().join("state.sqlite")).unwrap();
        other.create_directory(&ROOT_DIR_ID, &"Concurrent".into()).unwrap();
        assert!(placement.read().await.database().commit_native_placement(&plan, &staged.materialized_files.clone(), &[], || staged.publish()).is_err());
        assert!(!root.path().join("Shelf/book.epub").exists());
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
        assert!(placement.read().await.database().native_file_work().unwrap().is_empty());
    }

    #[tokio::test]
    async fn copy_publishes_verified_bytes_before_reporting_a_new_placement() {
        let (root, placement, book, destination) = fixture().await;
        assert!(placement.write().await.copy_book_to_directory(&book, Some(&ROOT_DIR_ID), &destination).await.unwrap());
        assert_eq!(fs::read(root.path().join("Shelf/book.epub")).unwrap(), BYTES);
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
        assert!(placement.read().await.database().native_file_work().unwrap().is_empty());
        assert!(!placement.write().await.copy_book_to_directory(&book, Some(&ROOT_DIR_ID), &destination).await.unwrap());
    }

    #[tokio::test]
    async fn move_renames_before_committing_and_preserves_existing_destinations_for_undo() {
        let (root, placement, book, destination) = fixture().await;
        assert!(placement.write().await.move_book_placement(&book, &ROOT_DIR_ID, &destination).await.unwrap());
        assert!(!root.path().join("book.epub").exists());
        assert_eq!(fs::read(root.path().join("Shelf/book.epub")).unwrap(), BYTES);
        assert!(!placement.read().await.database().native_file_work().unwrap().iter().any(|work| work.operation == "materialize"));
        crate::run_file_jobs(&placement).await.unwrap();
        assert!(placement.write().await.copy_book_to_directory(&book, Some(&destination), &ROOT_DIR_ID).await.unwrap());
        assert!(!placement.write().await.move_book_placement(&book, &ROOT_DIR_ID, &destination).await.unwrap());
        assert_eq!(fs::read(root.path().join("Shelf/book.epub")).unwrap(), BYTES);
    }

    #[tokio::test]
    async fn undo_of_a_move_restores_bytes_before_restoring_the_source_placement() {
        let (root, placement, book, destination) = fixture().await;
        placement.write().await.move_book_placement(&book, &ROOT_DIR_ID, &destination).await.unwrap();
        assert!(!root.path().join("book.epub").exists());
        placement.write().await.restore_book_placement(&book, &ROOT_DIR_ID).await.unwrap();
        placement.write().await.remove_book_from_directory(&book, &destination).unwrap();
        crate::run_file_jobs(&placement).await.unwrap();
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
        assert!(placement.read().await.database().native_file_work().unwrap().is_empty());
        assert!(placement.read().await.database().book_paths(&book).unwrap().iter().any(|path| path.as_str() == "/book.epub"));
    }

    #[tokio::test]
    async fn copied_tree_includes_nested_books_and_empty_directories() {
        let (root, placement, book, shelf) = fixture().await;
        let nested = placement.write().await.create_directory(&shelf, "Nested").unwrap().id;
        placement.write().await.create_directory(&nested, "Empty").unwrap();
        placement.write().await.copy_book_to_directory(&book, Some(&ROOT_DIR_ID), &nested).await.unwrap();
        let copy = placement.write().await.copy_directory(&shelf, &ROOT_DIR_ID).await.unwrap();
        let path = placement.read().await.database().native_directory_snapshot(&copy).await.unwrap().unwrap().relative_path;
        let directory = root.path().join(path.trim_start_matches('/'));
        assert_eq!(fs::read(directory.join("Nested/book.epub")).unwrap(), BYTES);
        assert!(directory.join("Nested/Empty").is_dir());
        assert_eq!(fs::read_to_string(directory.join(".biblos_uuid")).unwrap(), copy.to_string());
        assert!(placement.read().await.database().native_file_work().unwrap().is_empty());
    }

    #[tokio::test]
    async fn folder_rename_moves_unmanaged_contents_with_its_projection() {
        let (root, placement, book, shelf) = fixture().await;
        placement.write().await.copy_book_to_directory(&book, Some(&ROOT_DIR_ID), &shelf).await.unwrap();
        fs::write(root.path().join("Shelf/notes.txt"), b"keep me").unwrap();
        assert_eq!(placement.write().await.queue_directory_move(&shelf, None, Some("Renamed")).await.unwrap(), "Renamed");
        assert!(!root.path().join("Shelf").exists());
        assert_eq!(fs::read(root.path().join("Renamed/book.epub")).unwrap(), BYTES);
        assert_eq!(fs::read(root.path().join("Renamed/notes.txt")).unwrap(), b"keep me");
        assert!(placement.read().await.database().book_paths(&book).unwrap().iter().any(|path| path.as_str() == "/Renamed/book.epub"));
    }

    #[tokio::test]
    async fn directory_replay_moves_nested_contents_and_acknowledges_pending_work() {
        let (root, placement, book, shelf) = fixture().await;
        let nested = placement.write().await.create_directory(&shelf, "Nested").unwrap().id;
        placement.write().await.copy_book_to_directory(&book, Some(&ROOT_DIR_ID), &nested).await.unwrap();
        fs::write(root.path().join("Shelf/Nested/notes.txt"), b"keep me").unwrap();
        placement.read().await.database().move_directory(&shelf, None, Some("Renamed")).await.unwrap();
        assert!(!placement.read().await.database().native_directory_replay_work().unwrap().is_empty());

        crate::replay_directories(&placement.read().await.database(), &placement.read().await.assets()).unwrap();

        assert!(!root.path().join("Shelf").exists());
        assert_eq!(fs::read(root.path().join("Renamed/Nested/book.epub")).unwrap(), BYTES);
        assert_eq!(fs::read(root.path().join("Renamed/Nested/notes.txt")).unwrap(), b"keep me");
        assert_eq!(fs::read_to_string(root.path().join("Renamed/Nested/.biblos_uuid")).unwrap(), nested.to_string());
        assert!(placement.read().await.database().native_directory_replay_work().unwrap().is_empty());
        crate::replay_directories(&placement.read().await.database(), &placement.read().await.assets()).unwrap();
        assert_eq!(fs::read(root.path().join("Renamed/Nested/book.epub")).unwrap(), BYTES);
    }

    #[tokio::test]
    async fn replay_recovers_a_staged_directory_hidden_before_its_location_was_saved() {
        let (root, placement, _, shelf) = fixture().await;
        fs::write(root.path().join("Shelf/notes.txt"), b"keep me").unwrap();
        placement.read().await.database().move_directory(&shelf, None, Some("Renamed")).await.unwrap();
        let staged = placement.read().await.assets().stage_directory(shelf, "/Shelf", Some("/Renamed")).unwrap().unwrap();
        // Simulate interruption before recording the staged location, then a
        // synchronized deletion that removes this folder's live ancestry.
        placement.read().await.database().trash_directory(&shelf).await.unwrap();
        assert!(placement.read().await.database().native_directory_snapshot(&shelf).unwrap().is_none());
        assert_eq!(placement.read().await.database().native_directory_projected_path(&shelf).unwrap().as_deref(), Some("/Shelf"));

        for _ in 0..2 {
            crate::replay_directories(&placement.read().await.database(), &placement.read().await.assets()).unwrap();
            assert_eq!(placement.read().await.database().native_directory_projected_path(&shelf).unwrap().as_deref(), Some(staged.as_str()));
            assert_eq!(fs::read(root.path().join(staged.trim_start_matches('/')).join("notes.txt")).unwrap(), b"keep me");
            assert!(!root.path().join("Shelf").exists());
            assert!(!root.path().join("Renamed").exists());
            assert!(placement.read().await.database().native_directory_replay_work().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn preparing_directory_leaves_an_unrelated_broken_move_pending() {
        let (root, placement, _, shelf) = fixture().await;
        let unrelated = placement.write().await.create_directory(&ROOT_DIR_ID, "Unrelated").unwrap().id;
        placement.read().await.database().move_directory(&unrelated, None, Some("Moved")).await.unwrap();
        fs::write(root.path().join("Unrelated/.biblos_uuid"), b"foreign identity").unwrap();
        placement.read().await.database().move_directory(&shelf, None, Some("Renamed")).await.unwrap();

        crate::prepare_directory(&placement.read().await.assets(), &placement.read().await.database(), shelf).await.unwrap();

        assert!(root.path().join("Renamed").is_dir());
        assert!(!root.path().join("Shelf").exists());
        assert!(root.path().join("Unrelated").is_dir());
        assert!(!root.path().join("Moved").exists());
        let work = placement.read().await.database().native_directory_replay_work().await.unwrap();
        assert!(work.iter().any(|(_, id)| *id == unrelated));
        assert!(!work.iter().any(|(_, id)| *id == shelf));
        assert!(crate::replay_directories(&placement.read().await.database(), &placement.read().await.assets()).is_err());
        assert!(!placement.read().await.database().native_directory_replay_work().unwrap().is_empty());

        fs::write(root.path().join("Unrelated/.biblos_uuid"), unrelated.to_string()).unwrap();
        crate::replay_directories(&placement.read().await.database(), &placement.read().await.assets()).unwrap();
        assert!(root.path().join("Moved").is_dir());
        assert!(!root.path().join("Unrelated").exists());
        assert!(placement.read().await.database().native_directory_replay_work().unwrap().is_empty());
    }

    #[tokio::test]
    async fn preparing_nested_directory_replays_transitively_connected_moves() {
        let (root, placement, _, shelf) = fixture().await;
        let nested = placement.write().await.create_directory(&shelf, "Nested").unwrap().id;
        let second = placement.write().await.create_directory(&ROOT_DIR_ID, "Second").unwrap().id;
        let third = placement.write().await.create_directory(&ROOT_DIR_ID, "Third").unwrap().id;
        let unrelated = placement.write().await.create_directory(&ROOT_DIR_ID, "ShelfExtra").unwrap().id;
        for (path, contents) in [("Shelf/Nested/notes.txt", "nested"), ("Second/notes.txt", "second"), ("Third/notes.txt", "third")] {
            fs::write(root.path().join(path), contents).unwrap();
        }
        placement.read().await.database().move_directory(&third, None, Some("Fourth")).await.unwrap();
        placement.read().await.database().move_directory(&second, None, Some("Third")).await.unwrap();
        placement.read().await.database().move_directory(&shelf, None, Some("Second")).await.unwrap();
        placement.read().await.database().move_directory(&unrelated, None, Some("Isolated")).await.unwrap();
        fs::write(root.path().join("ShelfExtra/.biblos_uuid"), b"foreign identity").unwrap();

        crate::prepare_directory(&placement.read().await.assets(), &placement.read().await.database(), nested).await.unwrap();

        for (path, contents) in [("Second/Nested/notes.txt", "nested"), ("Third/notes.txt", "second"), ("Fourth/notes.txt", "third")] {
            assert_eq!(fs::read_to_string(root.path().join(path)).unwrap(), contents);
        }
        for (path, id) in [("Second", shelf), ("Second/Nested", nested), ("Third", second), ("Fourth", third)] {
            assert_eq!(fs::read_to_string(root.path().join(path).join(".biblos_uuid")).unwrap(), id.to_string());
        }
        assert!(!root.path().join("Shelf").exists());
        assert!(root.path().join("ShelfExtra").is_dir());
        assert!(!root.path().join("Isolated").exists());
        let work = placement.read().await.database().native_directory_replay_work().await.unwrap();
        assert!(work.iter().any(|(_, id)| *id == unrelated));
        assert!(work.iter().all(|(_, id)| ![shelf, nested, second, third].contains(id)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn permission_denied_at_publication_rolls_back_the_placement() {
        use std::os::unix::fs::PermissionsExt;
        let (root, placement, book, destination) = fixture().await;
        let plan = placement.read().await.database().plan_native_placement(NativePlacementEdit::CopyBook { book, source: Some(ROOT_DIR_ID), destination }).await.unwrap();
        let staged = PreparedEdit::prepare(&placement.read().await.assets(), &plan, false).unwrap();
        let original = placement.read().await.database().book_paths(&book).await.unwrap();
        let folder = root.path().join("Shelf");
        let permissions = fs::metadata(&folder).unwrap().permissions();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let result = placement.read().await.database().commit_native_placement(&plan, &staged.materialized_files.clone(), &[], || staged.publish()).await;
        fs::set_permissions(&folder, permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(placement.read().await.database().book_paths(&book).unwrap(), original);
        assert!(placement.read().await.database().native_file_work().unwrap().is_empty());
        assert!(!folder.join("book.epub").exists());
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn denied_move_does_not_fall_back_to_copying_into_a_writable_destination() {
        use std::os::unix::fs::PermissionsExt;
        let (root, placement, book, destination) = fixture().await;
        let original = placement.read().await.database().book_paths(&book).await.unwrap();
        let permissions = fs::metadata(root.path()).unwrap().permissions();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let result = placement.write().await.move_book_placement(&book, &ROOT_DIR_ID, &destination).await;
        fs::set_permissions(root.path(), permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(placement.read().await.database().book_paths(&book).unwrap(), original);
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
        assert!(!root.path().join("Shelf/book.epub").exists());
    }

    #[tokio::test]
    async fn reading_progress_can_change_while_a_copy_is_staged() {
        let (root, placement, book, destination) = fixture().await;
        let plan = placement.read().await.database().plan_native_placement(NativePlacementEdit::CopyBook { book, source: Some(ROOT_DIR_ID), destination }).await.unwrap();
        let staged = PreparedEdit::prepare(&placement.read().await.assets(), &plan, false).unwrap();
        placement.read().await.database().update_reading_position(&book, "epubcfi(/6/2)", Some(42.0), None).await.unwrap();
        placement.read().await.database().commit_native_placement(&plan, &staged.materialized_files.clone(), &[], || staged.publish()).await.unwrap();
        assert_eq!(fs::read(root.path().join("Shelf/book.epub")).unwrap(), BYTES);
        assert_eq!(placement.read().await.database().browse_book_detail(&book).unwrap().book.progress, 42.0);
    }

    #[tokio::test]
    async fn copy_completes_an_existing_replay_job_before_committing_its_placement() {
        let (root, placement, book, destination) = fixture().await;
        placement.read().await.database().copy_book_to_directory(&book, Some(&ROOT_DIR_ID), &destination).await.unwrap();
        placement.read().await.database().remove_book_placement(&book, &destination).await.unwrap();
        assert!(!placement.read().await.database().native_file_work().unwrap().is_empty());
        assert!(placement.write().await.copy_book_to_directory(&book, Some(&ROOT_DIR_ID), &destination).await.unwrap());
        assert_eq!(fs::read(root.path().join("Shelf/book.epub")).unwrap(), BYTES);
        assert!(!placement.read().await.database().native_file_work().unwrap().iter().any(|work| work.operation == "materialize"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cross_device_move_stages_the_copy_and_retires_source_after_commit() {
        use std::os::unix::{
            fs::{symlink, MetadataExt},
            prelude::PermissionsExt,
        };
        let (root, placement, book, destination) = fixture().await;
        let other = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        if fs::metadata(root.path()).unwrap().dev() == fs::metadata(other.path()).unwrap().dev() {
            return; // This host does not provide two writable filesystems.
        }
        // A symlink models a mounted destination without requiring mount privileges.
        fs::rename(root.path().join("Shelf/.biblos_uuid"), other.path().join(".biblos_uuid")).unwrap_or_else(|_| {
            fs::copy(root.path().join("Shelf/.biblos_uuid"), other.path().join(".biblos_uuid")).unwrap();
            fs::remove_file(root.path().join("Shelf/.biblos_uuid")).unwrap();
        });
        fs::remove_dir(root.path().join("Shelf")).unwrap();
        symlink(other.path(), root.path().join("Shelf")).unwrap();
        assert!(placement.write().await.move_book_placement(&book, &ROOT_DIR_ID, &destination).await.unwrap());
        assert_eq!(fs::read(other.path().join("book.epub")).unwrap(), BYTES);
        assert!(!root.path().join("book.epub").exists(), "the source is retired before committing the move");
        assert!(placement.read().await.database().native_file_work().unwrap().iter().any(|work| work.operation == "remove" && work.cleanup_path.as_ref().is_some_and(|path| path.file_name().starts_with(".bokheim-move-"))));
        crate::run_file_jobs(&placement).await.unwrap();
        assert!(!root.path().join("book.epub").exists());
        // Permissions on the copy are ordinary file permissions, not a stored cache.
        assert_ne!(fs::metadata(other.path().join("book.epub")).unwrap().permissions().mode() & 0o400, 0);
    }

    #[tokio::test]
    async fn interrupted_long_move_restores_source_from_persisted_replay_work() {
        let (root, placement, book, destination) = fixture().await;
        let mut plan = placement.read().await.database().plan_native_placement(NativePlacementEdit::TransferBook { book, source: ROOT_DIR_ID, destination, moving: true }).await.unwrap();
        let copied = PreparedEdit::prepare(&placement.read().await.assets(), &plan, true).unwrap();
        let source = plan.move_source.clone().unwrap();
        let (retirement, staged) = placement.read().await.assets().prepare_move_source_staging(&book, &source.physical_path).unwrap().unwrap();
        placement.read().await.database().record_native_move_staging(&mut plan, &staged).await.unwrap();
        retirement.publish_short().unwrap();
        assert!(!root.path().join("book.epub").exists());
        assert_eq!(fs::read(root.path().join(staged.as_str().trim_start_matches('/'))).unwrap(), BYTES);
        drop(copied); // The app stops before publishing or committing the move.
        let reopened = DatabaseClient::open(root.path().join("state.sqlite"), &BackendExecutor::new().unwrap()).await.unwrap();
        reopened.initialize_library().await.unwrap();
        let reopened = std::sync::Arc::new(tokio::sync::RwLock::new(LibraryState::new(reopened, placement.read().await.assets().clone(), placement.read().await.events().clone())));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = fs::metadata(root.path()).unwrap().permissions();
            fs::set_permissions(root.path(), fs::Permissions::from_mode(0o555)).unwrap();
            let blocked = crate::run_file_jobs(&reopened).await;
            fs::set_permissions(root.path(), permissions).unwrap();
            assert!(blocked.is_err());
            assert_eq!(fs::read(root.path().join(staged.as_str().trim_start_matches('/'))).unwrap(), BYTES);
            assert!(reopened.read().await.database().native_file_work().unwrap().iter().any(|work| work.operation == "remove" && work.cleanup_path.as_ref() == Some(&staged)));
        }
        crate::run_file_jobs(&reopened).await.unwrap();
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
        assert!(!root.path().join("Shelf/book.epub").exists());
        assert!(!root.path().join(staged.as_str().trim_start_matches('/')).exists());
        assert!(reopened.read().await.database().native_file_work().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn denied_publication_after_source_retirement_recovers_the_original_placement() {
        use std::os::unix::fs::PermissionsExt;
        let (root, placement, book, destination) = fixture().await;
        let original = placement.read().await.database().book_paths(&book).await.unwrap();
        let mut plan = placement.read().await.database().plan_native_placement(NativePlacementEdit::TransferBook { book, source: ROOT_DIR_ID, destination, moving: true }).await.unwrap();
        let copied = PreparedEdit::prepare(&placement.read().await.assets(), &plan, true).unwrap();
        let source = plan.move_source.clone().unwrap();
        let (retirement, staged) = placement.read().await.assets().prepare_move_source_staging(&book, &source.physical_path).unwrap().unwrap();
        placement.read().await.database().record_native_move_staging(&mut plan, &staged).await.unwrap();
        retirement.publish_short().unwrap();
        let folder = root.path().join("Shelf");
        let permissions = fs::metadata(&folder).unwrap().permissions();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let result = placement.read().await.database().commit_native_placement(&plan, &copied.materialized_files.clone(), &[], || copied.publish()).await;
        fs::set_permissions(&folder, permissions).unwrap();
        assert!(result.is_err());
        crate::run_file_jobs(&placement).await.unwrap();
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
        assert!(!folder.join("book.epub").exists());
        assert_eq!(placement.read().await.database().book_paths(&book).unwrap(), original);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_background_materialization_keeps_the_source_until_destination_is_verified() {
        use std::os::unix::fs::PermissionsExt;
        let (root, placement, book, destination) = fixture().await;
        // Existing queued work, including work received through sync, can
        // encounter a read-only destination after its logical change commits.
        placement.read().await.database().move_book_placement(&book, &ROOT_DIR_ID, &destination).await.unwrap();
        let folder = root.path().join("Shelf");
        let permissions = fs::metadata(&folder).unwrap().permissions();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let failed = crate::run_file_jobs(&placement).await;
        fs::set_permissions(&folder, permissions).unwrap();
        assert!(failed.is_err());
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), BYTES);
        assert!(!folder.join("book.epub").exists());
        crate::run_file_jobs(&placement).await.unwrap();
        assert_eq!(fs::read(folder.join("book.epub")).unwrap(), BYTES);
        assert!(!root.path().join("book.epub").exists());
    }
}
