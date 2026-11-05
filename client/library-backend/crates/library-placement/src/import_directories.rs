//! Materialize a selected import tree once, before opening its books.
use crate::{asset_workflow::prepare_directories, LibraryState};
use client_protocol::BackendError;
use library_database::CreatedDirectory;
use library_files::AssetStore;
use library_replica::{filenames::validate_component, portable_name_key};
use std::collections::{HashMap, HashSet};
use sync_common::DirId;

pub struct StagedImportDirectories {
    pub destinations: HashMap<Vec<String>, DirId>,
    pub created: Vec<CreatedDirectory>,
}

impl LibraryState {
    /// Paths are relative to the selected parent. Reuse managed folders; create
    /// new folders without replacing foreign entries, then flush and register
    /// the complete new tree with one filesystem boundary and one DB commit.
    pub async fn prepare_import_directories(&self, parent: DirId, paths: &[Vec<String>]) -> Result<HashMap<Vec<String>, DirId>, BackendError> {
        let assets = self.assets().for_import_batch();
        let staged = self.stage_import_directories(parent, paths, &assets).await?;
        assets.flush_import_batch().await?;
        self.database().register_created_directories(&staged.created).await?;
        Ok(staged.destinations)
    }

    /// Materialize a tree in the caller's durability scope. New identities stay
    /// in memory until the caller commits folders and books together.
    pub async fn stage_import_directories(&self, parent: DirId, paths: &[Vec<String>], assets: &AssetStore) -> Result<StagedImportDirectories, BackendError> {
        for path in paths {
            for component in path {
                validate_component(component)?;
                if component.starts_with('.') {
                    return Err(BackendError::message("dot-prefixed library directories are unsupported"));
                }
            }
        }
        let mut result = HashMap::new();
        if paths.is_empty() {
            return Ok(StagedImportDirectories { destinations: result, created: Vec::new() });
        }

        // Resolve existing destinations before recovery so a managed folder
        // renamed around a foreign entry still keeps its identity. Recover
        // their shared ancestors and intersecting moves only once.
        let mut existing = HashMap::new();
        let mut directories = vec![parent];
        for components in paths {
            let mut id = parent;
            for component in components {
                let key = (id, portable_name_key(component));
                let child = match existing.get(&key) {
                    Some(child) => *child,
                    None => {
                        let child = self.database().child_directory(&id, component).await?;
                        existing.insert(key, child);
                        child
                    }
                };
                let Some(child) = child else { break };
                directories.push(child);
                id = child;
            }
        }
        directories.sort_unstable();
        directories.dedup();
        prepare_directories(self.assets(), self.database(), &directories).await?;
        let base = self.database().native_book_path(parent, "placeholder").await?.ok_or("import parent is unavailable")?;
        let base = base.as_str().rsplit_once('/').map(|(parent, _)| parent.trim_start_matches('/').to_owned()).unwrap_or_default();
        let mut children = HashMap::<(DirId, String), (DirId, String)>::new();
        let mut new_ids = HashSet::new();
        let mut created = Vec::new();
        for components in paths {
            let (mut id, mut relative) = (parent, base.clone());
            for component in components {
                let key = (id, portable_name_key(component));
                let next = match children.get(&key) {
                    Some(next) => next.clone(),
                    None => {
                        let child = if new_ids.contains(&id) { None } else { existing.get(&key).copied().flatten() };
                        let next = if let Some(child) = child {
                            let path = self.database().native_book_path(child, "placeholder").await?.ok_or("import folder is unavailable")?;
                            (child, path.as_str().rsplit_once('/').map(|(parent, _)| parent.trim_start_matches('/').to_owned()).unwrap_or_default())
                        } else {
                            let requested = if relative.is_empty() { component.clone() } else { format!("{relative}/{component}") };
                            let mut recovered = None;
                            for (name, child) in assets.import_directory_candidates(&requested)? {
                                if !new_ids.contains(&child) && self.database().can_recover_import_directory(child).await? {
                                    let path = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
                                    assets.retain_import_directory(&path, child)?;
                                    recovered = Some((child, name));
                                    break;
                                }
                            }
                            let (child, name) = match recovered {
                                Some(recovered) => recovered,
                                None => {
                                    let child = DirId::new_v4();
                                    (child, assets.create_directory(&requested, child)?)
                                }
                            };
                            let path = if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") };
                            created.push(CreatedDirectory { id: child, parent_id: id, name, relative_path: path.clone() });
                            new_ids.insert(child);
                            (child, path)
                        };
                        children.insert(key, next.clone());
                        next
                    }
                };
                (id, relative) = next;
            }
            result.insert(components.clone(), id);
        }
        Ok(StagedImportDirectories { destinations: result, created })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_database::{executor::BackendExecutor, DatabaseClient};
    use library_files::AssetStore;
    use library_services::events::LibraryEventRouter;
    use std::fs;
    use sync_common::ROOT_DIR_ID;

    async fn placement(root: &std::path::Path) -> std::sync::Arc<tokio::sync::RwLock<LibraryState>> {
        let database = DatabaseClient::open(root.join("state.sqlite"), &BackendExecutor::new().unwrap()).await.unwrap();
        database.initialize_library().await.unwrap();
        let assets = AssetStore::open(root.to_str().unwrap()).unwrap();

        let (events, _) = LibraryEventRouter::new();
        std::sync::Arc::new(tokio::sync::RwLock::new(LibraryState::new(database, assets, events)))
    }

    #[tokio::test]
    async fn creates_the_whole_tree_and_reuses_it_after_reopen() {
        let root = tempfile::tempdir().unwrap();
        let paths = vec![vec!["Readest".into(), "Shelf".into()], vec!["Readest".into(), "Shelf".into(), "Rust".into()], vec!["Readest".into(), "Other".into()]];
        let first = {
            let placement = placement(root.path()).await;
            placement.write().await.prepare_import_directories(ROOT_DIR_ID, &paths).await.unwrap()
        };
        assert!(root.path().join("Readest/Shelf/Rust").is_dir());
        assert!(root.path().join("Readest/Other").is_dir());
        let reopened = placement(root.path()).await;
        assert_eq!(first, reopened.prepare_import_directories(ROOT_DIR_ID, &paths).await.unwrap());
        assert_eq!(first[&paths[1]], reopened.read().await.database().child_directory(&first[&paths[0]], "Rust").unwrap().unwrap());
    }

    #[tokio::test]
    async fn foreign_folders_are_preserved_and_invalid_paths_create_nothing() {
        let root = tempfile::tempdir().unwrap();
        let placement = placement(root.path()).await;
        let invalid = vec![vec!["Valid".into()], vec!["../escape".into()]];
        assert!(placement.write().await.prepare_import_directories(ROOT_DIR_ID, &invalid).await.is_err());
        assert!(!root.path().join("Valid").exists());
        fs::create_dir(root.path().join("Readest")).unwrap();
        fs::write(root.path().join("Readest/foreign"), b"keep").unwrap();
        let paths = vec![vec!["Readest".into(), "Shelf".into()]];
        let prepared = placement.write().await.prepare_import_directories(ROOT_DIR_ID, &paths).await.unwrap();
        let path = placement.read().await.database().native_book_path(prepared[&paths[0]], "book.epub").await.unwrap().unwrap();
        assert!(path.as_str().starts_with("/Readest 2/Shelf/"));
        assert_eq!(fs::read(root.path().join("Readest/foreign")).unwrap(), b"keep");
    }

    #[tokio::test]
    async fn preparing_multiple_destinations_recovers_their_shared_parent_move() {
        let root = tempfile::tempdir().unwrap();
        let placement = placement(root.path()).await;
        let paths = vec![vec!["Readest".into(), "One".into()], vec!["Readest".into(), "Two".into()]];
        let original = placement.write().await.prepare_import_directories(ROOT_DIR_ID, &paths).await.unwrap();
        let parent = placement.read().await.database().child_directory(&ROOT_DIR_ID, "Readest").await.unwrap().unwrap();
        fs::write(root.path().join("Readest/One/notes.txt"), b"keep").unwrap();
        placement.read().await.database().move_directory(&parent, None, Some("Renamed")).await.unwrap();
        let renamed = vec![vec!["Renamed".into(), "One".into()], vec!["Renamed".into(), "Two".into()]];
        let prepared = placement.write().await.prepare_import_directories(ROOT_DIR_ID, &renamed).await.unwrap();
        for (before, after) in paths.iter().zip(&renamed) {
            assert_eq!(original[before], prepared[after]);
        }
        assert!(!root.path().join("Readest").exists());
        assert_eq!(fs::read(root.path().join("Renamed/One/notes.txt")).unwrap(), b"keep");
        assert!(root.path().join("Renamed/Two").is_dir());
        assert!(placement.read().await.database().native_directory_replay_work().unwrap().is_empty());
    }

    #[tokio::test]
    async fn recovery_renaming_a_managed_folder_keeps_the_selected_identity() {
        let root = tempfile::tempdir().unwrap();
        let placement = placement(root.path()).await;
        let managed = placement.read().await.database().create_directory(&ROOT_DIR_ID, &"Readest".into()).await.unwrap().id;
        fs::create_dir(root.path().join("Readest")).unwrap();
        fs::write(root.path().join("Readest/foreign"), b"keep").unwrap();
        let paths = vec![vec!["Readest".into()], vec!["Readest".into(), "Shelf".into()]];
        let prepared = placement.write().await.prepare_import_directories(ROOT_DIR_ID, &paths).await.unwrap();
        assert_eq!(prepared[&paths[0]], managed);
        assert_eq!(placement.read().await.database().child_directory(&ROOT_DIR_ID, "Readest 2").unwrap(), Some(managed));
        assert!(root.path().join("Readest 2/Shelf").is_dir());
        assert_eq!(fs::read(root.path().join("Readest/foreign")).unwrap(), b"keep");
    }

    #[tokio::test]
    async fn staged_import_does_not_adopt_a_trashed_folder_marker() {
        let root = tempfile::tempdir().unwrap();
        let placement = placement(root.path()).await;
        let deleted = placement.write().await.create_directory(&ROOT_DIR_ID, "Readest").unwrap().id;
        placement.read().await.database().trash_directory(&deleted).await.unwrap();
        assert!(!placement.read().await.database().can_recover_import_directory(deleted).unwrap());
        let assets = placement.read().await.assets().for_import_batch();
        let paths = vec![vec!["Readest".into()]];
        let staged = placement.write().await.stage_import_directories(ROOT_DIR_ID, &paths, &assets).await.unwrap();
        assert_ne!(staged.destinations[&paths[0]], deleted);
        assert_eq!(staged.created[0].name, "Readest 2");
        assert!(placement.read().await.database().child_directory(&ROOT_DIR_ID, "Readest 2").unwrap().is_none());
    }

    #[tokio::test]
    async fn registration_rolls_back_the_whole_tree_on_a_stale_parent() {
        let root = tempfile::tempdir().unwrap();
        let placement = placement(root.path()).await;
        let directories = vec![
            CreatedDirectory { id: DirId::new_v4(), parent_id: ROOT_DIR_ID, name: "First".into(), relative_path: "First".into() },
            CreatedDirectory { id: DirId::new_v4(), parent_id: DirId::new_v4(), name: "Second".into(), relative_path: "Second".into() },
        ];
        assert!(placement.read().await.database().register_created_directories(&directories).is_err());
        assert!(placement.read().await.database().child_directory(&ROOT_DIR_ID, "First").unwrap().is_none());
    }
}
