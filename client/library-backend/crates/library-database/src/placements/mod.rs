//! The stored collection: placements, discovery, physical paths.

use crate::SqlParam;
#[cfg(feature = "scanner")]
use crate::import::ImportObservation;
use crate::native::observations::forget_file;
use crate::projection::{place_book, project_dirty};
#[cfg(feature = "scanner")]
use crate::query_methods::optional_blob_column;
use crate::query_methods::{first_column, optional_text_column, query_last, query_rows, text_column, try_for_each_row};
pub(crate) use crate::shared_sql::SharedSql;
#[cfg(feature = "scanner")]
use crate::sync::apply::registers::canonical_body;
use crate::sync::apply::registers::record_local;
#[cfg(feature = "scanner")]
use crate::sync::parse_uuid_column;
use crate::transactions::WriteOutcome::Commit;
#[cfg(feature = "scanner")]
use client_platform_runtime::profiling::ProfileSpan;
use content_address::ContentHash;
use book_inspection_contract::{CachedPdfReader, InspectedBook, PdfSourceIdentity};
use pdf_view_common::PdfReaderMetadata;
use rusqlite::{Connection, Row, Transaction};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
// ---- placements ----
// Directory and physical book-placement persistence: moves, copies, and the
// Trash lifecycle (trash/restore/purge) for both books and folders.

pub(crate) use crate::{Database, DatabaseError};
pub(crate) use include_sqlite_sql::include_sql;
pub(crate) use library_model::TransferOrigin;
pub(crate) use library_model::{BookPlacement, RelativeBookPath};
pub(crate) use library_model::{Dir, LibraryFolderDestination, LibraryTrashView, TrashedBook, TrashedFolder};
pub(crate) use library_replica::{DirectoryLifecycleState, LibraryStateEntry};
pub(crate) use rusqlite::OptionalExtension;
pub(crate) use std::collections::HashSet;
pub(crate) use sync_common::{BookId, DirId, FileName};
#[cfg(feature = "scanner")]
use sync_common::{StateKey, wire};

/// A physical import folder awaiting registration, with parents before children.
#[derive(Clone, Debug)]
pub struct CreatedDirectory {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: String,
    pub relative_path: String,
}

pub(crate) fn unix_millis() -> Result<i64, DatabaseError> {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|duration| duration.as_millis() as i64).map_err(DatabaseError::operation)
}

pub(crate) fn parse_dir_id(raw: &str) -> Result<DirId, DatabaseError> {
    DirId::parse_str(raw).map_err(DatabaseError::operation)
}

/// Read command preconditions from canonical registers, never placeholder defaults.
#[derive(Default)]
pub(crate) struct CanonicalDirectory {
    pub(crate) name: Option<FileName>,
    pub(crate) parent: Option<DirId>,
    pub(crate) lifecycle: Option<DirectoryLifecycleState>,
}

pub(crate) fn canonical_directory(connection: &rusqlite::Connection, id: &DirId) -> Result<CanonicalDirectory, DatabaseError> {
    let mut state = CanonicalDirectory::default();

    for row in query_rows(|callback| connection.directory_read_sync_state_version_body(&(id.to_string()), callback), |row| row.get::<_, Vec<u8>>(0))? {
        match sync_common::wire::decode::<LibraryStateEntry>(&row, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
            LibraryStateEntry::DirectoryName { name, .. } => state.name = Some(name),
            LibraryStateEntry::DirectoryParent { parent_id, .. } => state.parent = Some(parent_id),
            LibraryStateEntry::DirectoryLifecycle { value, .. } => state.lifecycle = Some(value),
            _ => return Err(DatabaseError::message("invalid canonical directory state")),
        }
    }
    Ok(state)
}

pub(crate) fn book_is_trashed(connection: &rusqlite::Connection, book_id: &BookId) -> Result<bool, DatabaseError> {
    Ok(connection.placements_book_is_trashed(book_id, |row| row.get(0))?)
}

pub(crate) fn restore_placement_candidates(connection: &rusqlite::Connection, book_id: &BookId) -> Result<Vec<(String, String)>, DatabaseError> {
    let mut placements = Vec::new();
    connection.restore_book_record_with_parent_select_2(book_id, |row| {
        placements.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        Ok(())
    })?;
    if placements.is_empty() {
        // A restore with no remaining membership explicitly creates a root
        // placement. Planning is read-only; commit revalidates these candidates.
        connection.restore_book_fallback_name(book_id, |row| {
            placements.push((sync_common::ROOT_DIR_ID.to_string(), row.get::<_, String>(0)?));
            Ok(())
        })?;
    }
    Ok(placements)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPathComponent {
    pub id: DirId,
    pub name: FileName,
}

/// One step of a book restore: a live flag snapshot for
/// every directory the walk would visit, starting at the original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreAncestor {
    pub id: DirId,
    pub live: bool,
}

/// Database ancestry used to choose a live folder when restoring a book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlacement {
    /// Retained folder, or ROOT_DIR_ID as an explicit creation candidate when
    /// the book has no remaining placements. Planning does not create it.
    pub original: DirId,
    pub file_name: String,
    pub ancestry: Vec<RestoreAncestor>,
}

pub(crate) fn dir_path_components(connection: &rusqlite::Connection, dir_id: &str) -> Result<Vec<DirPathComponent>, DatabaseError> {
    // Reuse an existing transaction, or hold a read snapshot for the whole walk.
    let snapshot = connection.is_autocommit().then(|| connection.unchecked_transaction()).transpose()?;

    let root = sync_common::ROOT_DIR_ID.to_string();
    let mut current = dir_id.to_owned();
    let mut seen = HashSet::new();
    let mut components = Vec::new();
    loop {
        if !seen.insert(current.clone()) {
            components.clear();
            break;
        }
        let Some((parent, name)) = connection.directory_live_parent_and_name(&(&current), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).optional()? else {
            components.clear();
            break;
        };
        if current == root {
            break;
        }
        components.push(DirPathComponent { id: parse_dir_id(&current)?, name });
        current = parent;
    }
    components.reverse();
    if let Some(snapshot) = snapshot {
        snapshot.commit()?;
    }
    Ok(components)
}

/// Folder and placement paths resolved together for one operation.
pub(crate) struct FolderPaths<'c> {
    pub(crate) connection: &'c Connection,
    pub(crate) folders: HashMap<DirId, Option<String>>,
    complete: bool,
}

impl<'c> FolderPaths<'c> {
    pub(crate) fn new(connection: &'c Connection) -> Self {
        Self { connection, folders: HashMap::new(), complete: false }
    }

    /// Resolve selected folders together; keep a single folder iterative.
    pub(crate) fn for_directories<'d>(connection: &'c Connection, directories: impl IntoIterator<Item = &'d str>) -> Result<Self, DatabaseError> {
        let distinct = directories.into_iter().collect::<HashSet<_>>();
        let ids = distinct.into_iter().map(parse_dir_id).collect::<Result<HashSet<_>, _>>()?;
        if ids.len() <= 1 {
            return Ok(Self::new(connection));
        }
        let json = serde_json::to_string(&ids).map_err(DatabaseError::operation)?;
        Self::batch(connection, &json, "[]")
    }

    /// Resolve the placement folders for a bounded page without buffering placements.
    pub(crate) fn for_books(connection: &'c Connection, books: &str) -> Result<Self, DatabaseError> {
        Self::batch(connection, "[]", books)
    }

    fn read_nodes(query: impl FnOnce(&mut dyn FnMut(&Row<'_>) -> rusqlite::Result<()>) -> rusqlite::Result<()>) -> Result<HashMap<DirId, Option<(DirId, String)>>, DatabaseError> {
        let mut nodes = HashMap::new();
        try_for_each_row(query, |row| {
            let id = parse_dir_id(text_column(row, 0)?)?;
            let node = match optional_text_column(row, 1)? {
                Some(parent) => Some((parse_dir_id(parent)?, row.get::<_, String>(2)?)),
                None => None,
            };
            nodes.insert(id, node);
            Ok(())
        })?;
        Ok(nodes)
    }

    fn batch(connection: &'c Connection, directories: &str, books: &str) -> Result<Self, DatabaseError> {
        let root = sync_common::ROOT_DIR_ID;
        let root_text = root.to_string();
        let snapshot = connection.is_autocommit().then(|| connection.unchecked_transaction()).transpose()?;
        // Read the selected folders and their parents together. Only ancestors
        // missing from this batch need a recursive walk.
        let mut nodes = Self::read_nodes(|read| connection.directory_path_seeds(directories, books, &root_text, read))?;
        let missing = nodes.values().filter_map(|node| node.as_ref().map(|(parent, _)| *parent)).filter(|parent| !nodes.contains_key(parent)).collect::<HashSet<_>>();
        if !missing.is_empty() {
            let json = serde_json::to_string(&missing).map_err(DatabaseError::operation)?;
            nodes.extend(Self::read_nodes(|read| connection.directory_path_ancestors(&json, &root_text, read))?);
        }
        if let Some(snapshot) = snapshot {
            snapshot.commit()?;
        }
        let mut folders = HashMap::new();
        folders.insert(root, nodes.remove(&root).flatten().map(|_| String::new()));
        let starts = nodes.keys().copied().collect::<Vec<_>>();
        for id in starts {
            if folders.contains_key(&id) {
                continue;
            }
            let mut current = id;
            let mut chain = Vec::new();
            let mut path = loop {
                if let Some(path) = folders.get(&current) {
                    break path.clone();
                }
                // Consuming the node also terminates cycles without another set.
                let Some((parent, name)) = nodes.remove(&current).flatten() else {
                    folders.insert(current, None);
                    break None;
                };
                chain.push((current, name));
                current = parent;
            };
            let mut at_root = current == root;
            for (id, name) in chain.into_iter().rev() {
                path = path.map(|mut path| {
                    if !at_root {
                        path.push('/');
                    }
                    path.push_str(&name);
                    path
                });
                at_root = false;
                folders.insert(id, path.clone());
            }
        }
        // Direct root lookups remain valid before its row has been created.
        folders.insert(root, Some(String::new()));
        Ok(Self { connection, folders, complete: false })
    }

    /// Resolve the complete live tree in one recursive query for inventories.
    pub(crate) fn all(connection: &'c Connection) -> Result<Self, DatabaseError> {
        let root = sync_common::ROOT_DIR_ID;
        let mut folders = HashMap::new();
        try_for_each_row(
            |read| connection.directory_all_live_paths(&root.to_string(), read),
            |row| {
                folders.insert(parse_dir_id(text_column(row, 0)?)?, Some(row.get::<_, String>(1)?));
                Ok(())
            },
        )?;
        folders.insert(root, Some(String::new()));
        Ok(Self { connection, folders, complete: true })
    }

    pub(crate) fn folder(&mut self, id: &str) -> Result<Option<&str>, DatabaseError> {
        let id = parse_dir_id(id)?;
        if !self.folders.contains_key(&id) {
            let path = if self.complete { None } else { directory_relative_path(self.connection, &id)? };
            self.folders.insert(id, path);
        }
        Ok(self.folders[&id].as_deref())
    }

    pub(crate) fn placement(&mut self, id: &str, name: &str) -> Result<Option<RelativeBookPath>, DatabaseError> {
        self.folder(id)?.map(|folder| stored_placement_path(folder, name)).transpose()
    }
}

/// Stored folder and file names must still form a path inside the library.
pub(crate) fn stored_book_path(value: &str) -> Result<RelativeBookPath, DatabaseError> {
    RelativeBookPath::parse(value).map_err(|error| DatabaseError::message(format!("stored book path {value:?} is not readable: {error}")))
}

pub(crate) fn validate_stored_book_path(value: &str) -> Result<(), DatabaseError> {
    RelativeBookPath::validate(value).map_err(|error| DatabaseError::message(format!("stored book path {value:?} is not readable: {error}")))
}

pub(crate) fn validate_stored_placement(folder: &str, file_name: &str) -> Result<(), DatabaseError> {
    RelativeBookPath::validate_in_folder(folder, file_name).map_err(|error| {
        let path = format!("{folder}/{file_name}");
        DatabaseError::message(format!("stored book path {path:?} is not readable: {error}"))
    })
}

/// An empty folder denotes the library root.
pub(crate) fn stored_placement_path(folder: &str, file_name: &str) -> Result<RelativeBookPath, DatabaseError> {
    stored_book_path(&format!("{folder}/{file_name}"))
}

pub(crate) fn stored_book_path_column(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<RelativeBookPath> {
    stored_book_path(&row.get::<_, String>(column)?).map_err(|error| crate::sync::invalid_text_column(column, error.to_string()))
}

/// Folders reached from `roots` one level per query, roots included.
/// Child queries take a JSON list of parents and return their children.
/// Each folder is visited once, so an ancestry loop cannot walk forever.
pub(crate) fn walk_down(connection: &rusqlite::Connection, roots: Vec<String>, trashed_only: bool) -> Result<Vec<String>, DatabaseError> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut folders: Vec<String> = roots.into_iter().filter(|root| seen.insert(root.clone())).collect();
    let mut level_start = 0;
    while level_start < folders.len() {
        let level_end = folders.len();
        let parents = serde_json::to_string(&folders[level_start..level_end]).map_err(DatabaseError::operation)?;
        let children = query_rows(|callback| if trashed_only { connection.directory_trashed_children(&parents, callback) } else { connection.directory_children(&parents, callback) }, |row| row.get::<_, String>(0))?;
        folders.extend(children.into_iter().filter(|child| seen.insert(child.clone())));
        level_start = level_end;
    }
    Ok(folders)
}

/// Folders with queued physical work, and every folder below them: a book in
/// any of them may be about to move on disk.
pub(crate) fn pending_work_directories(connection: &rusqlite::Connection) -> Result<Vec<String>, DatabaseError> {
    let roots = query_rows(|callback| connection.directory_work_roots(callback), |row| row.get::<_, String>(0))?;
    walk_down(connection, roots, false)
}

/// Trashed folders below `roots`, following each folder's remembered original
/// parent so folders hidden from the display tree still come along.
pub(crate) fn trashed_subtree(connection: &rusqlite::Connection, roots: Vec<String>) -> Result<Vec<String>, DatabaseError> {
    let roots = serde_json::to_string(&roots).map_err(DatabaseError::operation)?;
    let roots = query_rows(|callback| connection.directory_trashed_roots(&roots, callback), |row| row.get::<_, String>(0))?;
    walk_down(connection, roots, true)
}

/// Materialized paths first, then the book's live placements.
pub(crate) fn book_paths_in(connection: &rusqlite::Connection, hash: &BookId) -> Result<Vec<RelativeBookPath>, DatabaseError> {
    let mut paths = query_rows(|read| connection.book_projection_paths(hash, read), |row| stored_book_path_column(row, 0))?;
    let placements = query_rows(|read| connection.shared_live_book_placements(hash, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut resolver = FolderPaths::for_directories(connection, placements.iter().map(|(id, _)| id.as_str()))?;
    for (dir_id, file_name) in placements {
        paths.extend(resolver.placement(&dir_id, &file_name)?);
    }
    Ok(paths)
}

/// A placement path's live folder and file name, walking folder names down
/// from the root. None when a folder on the way is missing or deleted.
pub(crate) fn resolve_placement_path(connection: &rusqlite::Connection, relative_path: &str) -> Result<Option<(String, String)>, DatabaseError> {
    let snapshot = connection.is_autocommit().then(|| connection.unchecked_transaction()).transpose()?;
    let result = resolve_placement_path_in(connection, relative_path)?;
    if let Some(snapshot) = snapshot {
        snapshot.commit()?;
    }
    Ok(result)
}

fn resolve_placement_path_in(connection: &rusqlite::Connection, relative_path: &str) -> Result<Option<(String, String)>, DatabaseError> {
    let root = sync_common::ROOT_DIR_ID.to_string();

    if connection.directory_live_parent_and_name(&(&root), |_| Ok(())).optional()?.is_none() {
        return Ok(None);
    }
    let Some((folders, file_name)) = relative_path.strip_prefix('/').and_then(|path| path.rsplit_once('/').or(Some(("", path)))) else {
        return Ok(None);
    };

    let mut current = root;
    for name in folders.split('/').filter(|name| !name.is_empty()) {
        let Some(child) = connection.directory_named_child(&(&current), &name, |row| row.get::<_, String>(0)).optional()? else {
            return Ok(None);
        };
        current = child;
    }
    Ok(Some((current, file_name.to_owned())))
}

/// The live book at a placement path, with the folder and file name the path
/// resolved to.
pub(crate) fn book_at_path(connection: &rusqlite::Connection, relative_path: &str) -> Result<Option<(BookId, DirId, String)>, DatabaseError> {
    // Include the final book lookup in the same snapshot as its folder walk.
    let snapshot = connection.is_autocommit().then(|| connection.unchecked_transaction()).transpose()?;
    let result = book_at_path_in(connection, relative_path)?;
    if let Some(snapshot) = snapshot {
        snapshot.commit()?;
    }
    Ok(result)
}

fn book_at_path_in(connection: &rusqlite::Connection, relative_path: &str) -> Result<Option<(BookId, DirId, String)>, DatabaseError> {
    let Some((dir_id, file_name)) = resolve_placement_path(connection, relative_path)? else {
        return Ok(None);
    };
    let Some(hash) = first_column::<BookId>(|read| connection.book_at_placement(&dir_id, &file_name, read))? else { return Ok(None) };
    Ok(Some((hash, parse_dir_id(&dir_id)?, file_name)))
}

/// Whether `directory` is `folder` or one of its ancestors, following current
/// parents through deleted folders too. A move that answers yes closes a loop.
pub(crate) fn is_ancestor_or_self(connection: &rusqlite::Connection, directory: &str, folder: &str) -> Result<bool, DatabaseError> {
    let root = sync_common::ROOT_DIR_ID.to_string();
    let mut current = folder.to_owned();
    let mut seen = HashSet::new();
    while seen.insert(current.clone()) {
        if current == directory {
            return Ok(true);
        }
        if current == root {
            break;
        }
        let Some(parent) = connection.directory_any_parent(&(&current), |row| row.get::<_, String>(0)).optional()? else { break };
        current = parent;
    }
    Ok(false)
}

pub(crate) fn directory_exists_in(connection: &rusqlite::Connection, directory_id: &str) -> Result<bool, DatabaseError> {
    Ok(connection.placements_live_directory_exists(directory_id, |row| row.get(0))?)
}

pub(crate) fn validate_library_directory_name(name: &str) -> Result<(), DatabaseError> {
    if name.starts_with('.') {
        return Err(DatabaseError::message("dot-prefixed library directories are unsupported"));
    }
    Ok(())
}

/// Live database names reserved in a destination. Physical collisions are
/// checked separately by the file store.
pub(crate) fn occupied_file_names_in(connection: &rusqlite::Connection, destination: &str) -> Result<Vec<String>, DatabaseError> {
    Ok(query_rows(|read| connection.shared_occupied_file_names(destination, read), |row| row.get::<_, String>(0))?)
}

/// The filename of a live placement, or `None` when it is absent.
pub(crate) fn live_file_name(connection: &rusqlite::Connection, directory: &DirId, hash: &BookId) -> Result<Option<String>, DatabaseError> {
    Ok(first_column::<String>(|read| connection.placements_live_file_name(&directory.to_string(), hash, read))?)
}

pub(crate) fn choose_move_folder_name(connection: &rusqlite::Connection, parent: &str, excluded: &str, requested: &str) -> Result<String, DatabaseError> {
    for number in 1..=8 {
        let candidate = if number == 1 { library_replica::project_folder_name(requested) } else { library_replica::numbered_folder_name(requested, number) };
        if !connection.placements_folder_name_occupied(parent, &library_replica::portable_name_key(&candidate), excluded, |row| row.get::<_, bool>(0))? {
            return Ok(candidate);
        }
    }
    let names = query_rows(|read| connection.placements_sibling_folder_names(parent, excluded, read), |row| row.get::<_, String>(0))?;
    Ok(library_replica::unique_folder_name(requested, names.iter().map(String::as_str)))
}

/// Records this directory into `dir`, validating it is neither the root nor
/// its own ancestor.
pub(crate) fn upsert_dir(transaction: &rusqlite::Transaction<'_>, dir_id: &DirId, parent_id: &DirId, name: &str) -> Result<(), DatabaseError> {
    record_directory(transaction, dir_id, parent_id, name)?;
    project_dirty(transaction)
}

fn record_directory(transaction: &rusqlite::Transaction<'_>, dir_id: &DirId, parent_id: &DirId, name: &str) -> Result<(), DatabaseError> {
    if *dir_id == sync_common::ROOT_DIR_ID {
        return Err(DatabaseError::message("the root directory is reserved"));
    }
    if dir_id == parent_id {
        return Err(DatabaseError::message("a directory cannot be its own parent"));
    }
    let closes_cycle = is_ancestor_or_self(transaction, &dir_id.to_string(), &parent_id.to_string())?;
    if closes_cycle {
        return Err(DatabaseError::message("directory move would create a cycle"));
    }
    record_local(transaction, LibraryStateEntry::DirectoryName { dir_id: *dir_id, name: name.to_owned() })?;
    record_local(transaction, LibraryStateEntry::DirectoryParent { dir_id: *dir_id, parent_id: *parent_id })?;
    record_local(transaction, LibraryStateEntry::DirectoryLifecycle { dir_id: *dir_id, value: DirectoryLifecycleState::Present })?;
    Ok(())
}

/// The relative path a live directory would materialize at, or `None` when
/// it is not reachable from the library root.
pub(crate) fn directory_relative_path(connection: &rusqlite::Connection, dir_id: &DirId) -> Result<Option<String>, DatabaseError> {
    if *dir_id == sync_common::ROOT_DIR_ID {
        return Ok(Some(String::new()));
    }
    let components = dir_path_components(connection, &dir_id.to_string())?;
    if components.is_empty() {
        return Ok(None);
    }
    Ok(Some(components.iter().map(|component| component.name.as_str()).collect::<Vec<_>>().join("/")))
}

/// Publishes a directory's queued physical projection to `local_directory_work`.
pub(crate) fn queue_directory_work(transaction: &rusqlite::Transaction<'_>, dir: &DirId) -> Result<(), DatabaseError> {
    #[cfg(target_arch = "wasm32")]
    let _ = (transaction, dir);
    #[cfg(not(target_arch = "wasm32"))]
    transaction.shared_placements_queue_directory_work(&dir.to_string())?;
    Ok(())
}

/// Publishes a book's queued local file operation. A no-op on wasm, which has
/// no separate physical file store to reconcile.
pub(crate) fn queue_materialization(transaction: &rusqlite::Connection, hash: &BookId, directory: &DirId) -> Result<(), DatabaseError> {
    #[cfg(target_arch = "wasm32")]
    let _ = (transaction, hash, directory);
    #[cfg(not(target_arch = "wasm32"))]
    transaction.shared_queue_materialization(hash, &directory.to_string())?;
    Ok(())
}

pub(crate) fn queue_file_removal(transaction: &rusqlite::Connection, hash: &BookId, path: &RelativeBookPath) -> Result<(), DatabaseError> {
    #[cfg(target_arch = "wasm32")]
    let _ = (transaction, hash, path);
    #[cfg(not(target_arch = "wasm32"))]
    transaction.shared_placements_queue_file_work(hash, path.as_str())?;
    Ok(())
}

/// The library-relative path of `name` in `dir`: "/book.epub" at the root,
/// "/a/b/book.epub" below it. Fails when the folder is not live.
pub(crate) fn placement_path(connection: &rusqlite::Connection, dir: &DirId, name: &str) -> Result<RelativeBookPath, DatabaseError> {
    let folder = directory_relative_path(connection, dir)?.ok_or_else(|| DatabaseError::message("placement directory has no path"))?;
    stored_placement_path(&folder, name)
}

/// Preserves upload provenance across a metadata-only copy/move. No upload work.
pub(crate) fn copy_book_baseline(transaction: &rusqlite::Transaction<'_>, source: &DirId, destination: &DirId, hash: &BookId) -> Result<(), DatabaseError> {
    transaction.placements_copy_book_baseline(&source.to_string(), &destination.to_string(), hash)?;
    Ok(())
}

pub(crate) fn can_recover_import_directory(connection: &rusqlite::Connection, id: DirId) -> Result<bool, DatabaseError> {
    let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1)", [id.to_string()], |row| row.get(0))?;
    if exists {
        return Ok(false);
    }
    let state = canonical_directory(connection, &id)?;
    Ok(state.name.is_none() && state.parent.is_none() && state.lifecycle.is_none())
}

pub(crate) fn register_created_directories(transaction: &Transaction<'_>, directories: &[CreatedDirectory]) -> Result<Vec<Dir>, DatabaseError> {
    use crate::native::NativeFileWorkSql;
    let mut result = Vec::new();
    for CreatedDirectory { id, parent_id, name, relative_path: path } in directories {
        if !directory_exists_in(transaction, &parent_id.to_string())? {
            return Err(DatabaseError::message("parent directory does not exist"));
        }
        let parent = directory_relative_path(transaction, parent_id)?.ok_or_else(|| DatabaseError::message("parent directory has no path"))?;
        let expected = if parent.is_empty() { name.to_owned() } else { format!("{parent}/{name}") };
        if path != &expected || choose_move_folder_name(transaction, &parent_id.to_string(), &id.to_string(), name)? != *name {
            return Err(DatabaseError::message("folder destination changed during creation"));
        }
        upsert_dir(transaction, id, parent_id, name)?;
        transaction.native_set_directory_projection(&id.to_string(), &format!("/{}", path.trim_start_matches('/')))?;
        transaction.directory_clear_work(&(id.to_string()))?;
        result.push(Dir::new(*id, name.to_owned(), 0, 0));
    }
    Ok(result)
}

impl Database {
    pub fn dir_path_components(&self, directory_id: &DirId) -> Result<Vec<DirPathComponent>, DatabaseError> {
        dir_path_components(&self.connection, &directory_id.to_string())
    }

    /// A destination is usable only when its complete ancestry is live and rooted.
    pub fn is_live_destination(&self, directory: &DirId) -> Result<bool, DatabaseError> {
        Ok(directory_relative_path(&self.connection, directory)?.is_some())
    }

    /// Whether a placement still addresses these bytes through live
    /// directories. A move, rename, or deletion that landed mid-transfer
    /// makes the finished bytes belong to a stale path.
    pub fn book_placement_expects_hash(&self, placement: &BookPlacement, book_id: &BookId) -> Result<bool, DatabaseError> {
        let Some((stored_hash, dir_id, file_name)) = book_at_path(&self.connection, placement.rel_path.as_str())? else {
            return Ok(false);
        };
        if stored_hash != placement.book_id || stored_hash != *book_id {
            return Ok(false);
        }
        self.connection.placement_expects_hash(&dir_id.to_string(), book_id, &file_name, |row| row.get(0)).map_err(DatabaseError::operation)
    }

    /// The live folder in `parent_id` that a folder named `name` would
    /// collide with, if any.
    pub fn child_directory(&self, parent_id: &DirId, name: &str) -> Result<Option<DirId>, DatabaseError> {
        let child = first_column::<String>(|read| self.connection.placements_child_directory(&parent_id.to_string(), name, read))?;
        child.as_deref().map(parse_dir_id).transpose()
    }

    /// Whether the folder already holds a live book under this file name,
    /// compared as file name collisions are.
    pub fn has_book_named(&self, dir_id: &DirId, file_name: &str) -> Result<bool, DatabaseError> {
        Ok(self.connection.placements_has_book_named(&dir_id.to_string(), file_name, |row| row.get(0))?)
    }

    /// Creates and validates a synchronized directory with a fresh id in one
    /// transaction.
    pub fn create_directory(&self, parent_id: &DirId, name: &FileName) -> Result<Dir, DatabaseError> {
        self.create_directory_with_id(&DirId::new_v4(), parent_id, name)
    }

    /// Creates and validates a synchronized directory in one transaction.
    /// An import job may retry the same directory id after losing its worker.
    pub fn create_directory_with_id(&self, dir_id: &DirId, parent_id: &DirId, name: &FileName) -> Result<Dir, DatabaseError> {
        validate_library_directory_name(name)?;
        self.write(|transaction| {
            if *dir_id == sync_common::ROOT_DIR_ID {
                return Err(DatabaseError::message("the library root cannot be created"));
            }
            let state = canonical_directory(transaction, dir_id)?;
            if state.name.as_ref().is_some_and(|existing| existing != name) || state.parent.is_some_and(|existing| existing != *parent_id) || state.lifecycle.is_some_and(|existing| existing != DirectoryLifecycleState::Present) {
                return Err(DatabaseError::message("import directory was changed or deleted"));
            }
            if state.name.is_none() || state.parent.is_none() || state.lifecycle.is_none() {
                if !directory_exists_in(transaction, &parent_id.to_string())? {
                    return Err(DatabaseError::message(format!("parent directory does not exist: {parent_id}")));
                }
                let missing = [
                    state.name.is_none().then(|| LibraryStateEntry::DirectoryName { dir_id: *dir_id, name: name.clone() }),
                    state.parent.is_none().then_some(LibraryStateEntry::DirectoryParent { dir_id: *dir_id, parent_id: *parent_id }),
                    state.lifecycle.is_none().then_some(LibraryStateEntry::DirectoryLifecycle { dir_id: *dir_id, value: DirectoryLifecycleState::Present }),
                ];
                for body in missing.into_iter().flatten() {
                    record_local(transaction, body)?;
                }
            }
            project_dirty(transaction)?;
            directory_relative_path(transaction, dir_id)?.ok_or_else(|| DatabaseError::message("created directory has no path"))?;
            let projected_name = transaction.directory_projected_name(&(dir_id.to_string()), |row| row.get::<_, String>(0))?;
            Ok(Dir::new(*dir_id, projected_name, 0, 0))
        })
    }

    /// Plan a new local folder without writing an intent to SQLite.
    pub fn directory_creation_path(&self, parent_id: &DirId, name: &str) -> Result<(String, String), DatabaseError> {
        validate_library_directory_name(name)?;
        if !directory_exists_in(&self.connection, &parent_id.to_string())? {
            return Err(DatabaseError::message("parent directory does not exist"));
        }
        let parent = directory_relative_path(&self.connection, parent_id)?.ok_or_else(|| DatabaseError::message("parent directory has no path"))?;
        let chosen = choose_move_folder_name(&self.connection, &parent_id.to_string(), "", name)?;
        let path = if parent.is_empty() { chosen.clone() } else { format!("{parent}/{chosen}") };
        Ok((chosen, path))
    }

    /// Register a folder already created on disk, including its projection.
    /// A crash before this commit is recovered by filesystem discovery.
    pub fn register_created_directory(&self, id: &DirId, parent_id: &DirId, name: &str, path: &str) -> Result<Dir, DatabaseError> {
        self.register_created_directories(&[CreatedDirectory { id: *id, parent_id: *parent_id, name: name.to_owned(), relative_path: path.to_owned() }])?.pop().ok_or_else(|| DatabaseError::message("created directory was not registered"))
    }

    /// Register an already-durable tree, parents before children, in one commit.
    pub fn register_created_directories(&self, directories: &[CreatedDirectory]) -> Result<Vec<Dir>, DatabaseError> {
        if directories.is_empty() {
            return Ok(Vec::new());
        }
        self.write(|transaction| register_created_directories(transaction, directories))
    }

    /// Unregistered filesystem markers can recover an interrupted Copy batch.
    /// Never adopt an identity already known here, including hidden or purged folders.
    pub fn can_recover_import_directory(&self, id: DirId) -> Result<bool, DatabaseError> {
        can_recover_import_directory(&self.connection, id)
    }

    pub fn folder_destinations(&self) -> Result<Vec<LibraryFolderDestination>, DatabaseError> {
        let root = sync_common::ROOT_DIR_ID;
        let root_id = root.to_string();
        let mut destinations = vec![LibraryFolderDestination { id: root, parent_id: None, label: "Library".to_owned(), path: "Library".to_owned() }];
        try_for_each_row(
            |read| self.connection.directory_folder_destinations(&root_id, read),
            |row| {
                let id = text_column(row, 0)?;
                if id != root_id {
                    destinations.push(LibraryFolderDestination { id: parse_dir_id(id)?, parent_id: Some(parse_dir_id(text_column(row, 2)?)?), label: row.get(3)?, path: row.get(1)? });
                }
                Ok(())
            },
        )?;
        destinations[1..].sort_by(|left, right| library_replica::portable_name_key(&left.path).cmp(&library_replica::portable_name_key(&right.path)).then_with(|| left.id.cmp(&right.id)));
        Ok(destinations)
    }

    pub fn move_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, DatabaseError> {
        self.write(|transaction| move_directory_in(transaction, directory_id, requested_parent, requested_name))
    }

    /// Duplicates a folder and everything under it beneath `destination_parent_id`,
    /// returning the copy's own directory id — what an undo needs to find what
    /// this call created.
    ///
    /// Books are content-addressed, so a copied book is a second placement of
    /// the same asset rather than a second copy of its bytes.
    pub fn copy_directory(&self, directory_id: &DirId, destination_parent_id: &DirId) -> Result<DirId, DatabaseError> {
        self.write(|transaction| copy_directory_in(transaction, directory_id, destination_parent_id, &mut DirId::new_v4))
    }

    /// Puts a book back in a folder it was removed from, reviving the book
    /// itself if that removal was what sent it to Trash. The tombstoned
    /// placement is reused where it still exists, so the book returns under
    /// the file name it had rather than a fresh unique one.
    pub fn restore_book_placement(&self, book_id: &BookId, directory_id: &DirId) -> Result<(), DatabaseError> {
        self.write(|transaction| restore_book_placement_in(transaction, book_id, directory_id, false))
    }

    /// Copies from an explicit folder, or any live placement for a flat listing.
    /// Returns whether a new destination placement was created, for undo.
    pub fn copy_book_to_directory(&self, book_id: &BookId, source_id: Option<&DirId>, destination_id: &DirId) -> Result<bool, DatabaseError> {
        self.write(|transaction| copy_book_to_directory_in(transaction, book_id, source_id, destination_id))
    }

    /// Atomically move a placement and report whether its destination was new.
    pub fn move_book_placement(&self, book: &BookId, source: &DirId, destination: &DirId) -> Result<bool, DatabaseError> {
        self.write(|tx| {
            let created = live_file_name(tx, destination, book)?.is_none();
            transfer_book_placement_in(tx, book, source, destination, true)?;
            Ok(created)
        })
    }

    pub fn remove_book_placement(&self, book_id: &BookId, source_id: &DirId) -> Result<bool, DatabaseError> {
        self.write(|transaction| remove_book_placement_in(transaction, book_id, source_id))
    }

    pub fn library_trash(&self) -> Result<LibraryTrashView, DatabaseError> {
        let books = query_rows(
            |read| self.connection.library_trash_select(read),
            |row| {
                Ok(TrashedBook {
                    book_id: row.get::<_, BookId>(0)?,
                    title: row.get(1)?,
                    author: row.get(2)?,
                    format: row.get::<_, Option<i64>>(3)?.and_then(BookFormat::from_code).map(|format| format.canonical_extension().to_owned()).unwrap_or_default(),
                    deleted_at: row.get(4)?,
                })
            },
        )?;
        let raw = query_rows(|read| self.connection.library_trash_select_2(read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, Option<String>>(3)?)))?;
        let mut folders = Vec::new();
        for (id, name, deleted_at, parent_name) in raw {
            folders.push(TrashedFolder { id: parse_dir_id(&id)?, name, parent_name, deleted_at });
        }
        Ok(LibraryTrashView { books, folders })
    }

    pub fn trash_book(&self, book_id: &BookId) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            let live: bool = transaction.directory_live_book_exists(&(&book_id), |row| row.get(0))?;
            if !live {
                return Err(DatabaseError::message("book does not exist or is already in Trash"));
            }
            record_book_lifecycle(transaction, book_id, library_replica::BookLifecycleState::Deleted { origin_folder_id: None })?;
            Ok(())
        })
    }

    /// Restore a trashed book using only the database's directory liveness.
    pub fn restore_book(&self, book_id: &BookId) -> Result<(), DatabaseError> {
        let choices = self
            .restore_book_plan(book_id)?
            .into_iter()
            .map(|candidate| {
                let target = candidate.ancestry.iter().find(|ancestor| ancestor.live).map(|ancestor| ancestor.id).unwrap_or(sync_common::ROOT_DIR_ID);
                (candidate.original, target)
            })
            .collect::<Vec<_>>();
        self.restore_book_commit(book_id, &choices)
    }

    pub(crate) fn restore_book_plan(&self, book_id: &BookId) -> Result<Vec<RestorePlacement>, DatabaseError> {
        if !book_is_trashed(&self.connection, book_id)? {
            return Err(DatabaseError::message("book is not in Trash"));
        }
        let recoverable = restore_placement_candidates(&self.connection, book_id)?;
        if recoverable.is_empty() {
            return Err(DatabaseError::message("book has no recoverable placement"));
        }
        let mut plan = Vec::with_capacity(recoverable.len());
        for (original, name) in recoverable {
            let mut ancestry = Vec::new();
            let original = parse_dir_id(&original)?;
            let mut target = original;
            let mut visited = HashSet::new();
            while target != sync_common::ROOT_DIR_ID {
                if !visited.insert(target) {
                    break;
                }
                let directory: Option<(String, bool)> = query_last(|read| self.connection.restore_book_record_with_parent_select_4(&target.to_string(), read), |row| Ok((row.get(0)?, row.get(1)?)))?;
                let Some((parent, live)) = directory else {
                    break;
                };
                ancestry.push(RestoreAncestor { id: target, live });
                target = parse_dir_id(&parent)?;
            }
            plan.push(RestorePlacement { original, file_name: name, ancestry });
        }
        Ok(plan)
    }

    pub(crate) fn restore_book_commit(&self, book_id: &BookId, choices: &[(DirId, DirId)]) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            if !book_is_trashed(transaction, book_id)? {
                return Err(DatabaseError::message("book is not in Trash"));
            }
            let recoverable: HashMap<DirId, String> = restore_placement_candidates(transaction, book_id)?.into_iter().map(|(original, name)| parse_dir_id(&original).map(|original| (original, name))).collect::<Result<_, _>>()?;
            if recoverable.is_empty() {
                return Err(DatabaseError::message("book has no recoverable placement"));
            }
            let mut originals = HashSet::new();
            if choices.len() != recoverable.len() || choices.iter().any(|(original, _)| !recoverable.contains_key(original) || !originals.insert(*original)) {
                return Err(DatabaseError::message("restore targets do not cover the recoverable placements"));
            }
            // Choosing another folder is an explicit move, not a side effect of
            // trashing. Retain originals that are also selected destinations.
            let targets = choices.iter().map(|(_, target)| *target).collect::<HashSet<_>>();
            for (original, _) in choices {
                if !targets.contains(original) {
                    remove_placement(transaction, original, book_id, None)?;
                }
            }
            record_book_lifecycle(transaction, book_id, library_replica::BookLifecycleState::Present)?;
            project_dirty(transaction)?;
            for (original, target) in choices {
                if *target != sync_common::ROOT_DIR_ID
                    && !{
                        let directory: Option<(String, bool)> = query_last(|read| transaction.restore_book_record_with_parent_select_4(&target.to_string(), read), |row| Ok((row.get(0)?, row.get(1)?)))?;
                        directory.is_some_and(|(_, live)| live)
                    }
                {
                    return Err(DatabaseError::message("restore target changed; retry the restore"));
                }
                let name = &recoverable[original];
                let existing = live_file_name(transaction, target, book_id)?;
                if existing.is_none() {
                    let occupied = occupied_file_names_in(transaction, &target.to_string())?;
                    let chosen = library_replica::unique_file_name(name, occupied.iter().map(String::as_str));
                    place_book(transaction, target, book_id, &chosen)?;
                    project_dirty(transaction)?;
                }
            }
            Ok(())
        })
    }

    /// Permanently removes a book in Trash. Sync intent and retryable file
    /// cleanup survive independently of the deleted database row.
    pub fn purge_book(&self, book_id: &BookId) -> Result<(), DatabaseError> {
        self.write(|transaction| purge_book_in(transaction, book_id))
    }

    pub fn trash_directory(&self, directory_id: &DirId) -> Result<(), DatabaseError> {
        self.write(|transaction| trash_directory_in(transaction, directory_id))
    }

    pub fn restore_directory(&self, directory_id: &DirId, requested_parent: Option<&DirId>) -> Result<String, DatabaseError> {
        self.write(|transaction| {
            let state = canonical_directory(transaction, directory_id)?;
            if state.lifecycle != Some(DirectoryLifecycleState::Deleted) {
                return Err(DatabaseError::message("folder is not in Trash"));
            }
            let (Some(stored_parent), Some(name)) = (state.parent, state.name) else {
                return Err(DatabaseError::message("folder state is incomplete; retry after sync"));
            };
            let requested_parent = requested_parent.copied().unwrap_or(stored_parent);
            let parent = if directory_exists_in(transaction, &requested_parent.to_string())? { requested_parent } else { sync_common::ROOT_DIR_ID };
            let occupied = query_rows(|read| transaction.placements_sibling_folder_names(&parent.to_string(), &directory_id.to_string(), read), |row| row.get::<_, String>(0))?;
            let chosen = library_replica::unique_folder_name(&name, occupied.iter().map(String::as_str));
            upsert_dir(transaction, directory_id, &parent, &chosen)?;
            let placements = query_rows(|callback| transaction.directory_placements_trashed_from(&(directory_id.to_string()), callback), |row| Ok((row.get::<_, String>(0)?, row.get::<_, BookId>(1)?)))?;
            for (directory, hash) in placements {
                record_local(transaction, LibraryStateEntry::Placement { dir_id: parse_dir_id(&directory)?, book_id: hash, present: true, origin_folder_id: None })?;
            }
            let books = query_rows(|callback| transaction.directory_books_trashed_from(&(directory_id.to_string()), callback), |row| row.get::<_, BookId>(0))?;
            for hash in &books {
                record_book_lifecycle(transaction, hash, library_replica::BookLifecycleState::Present)?;
            }
            // Restore the complete tree and book projections in the database.
            project_dirty(transaction)?;
            for hash in books {
                let paths = query_rows(|callback| transaction.directory_unobserved_placements(&(&hash), callback), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
                for (dir, _) in paths {
                    queue_materialization(transaction, &hash, &parse_dir_id(&dir)?)?;
                }
            }
            directory_relative_path(transaction, directory_id)?.ok_or_else(|| DatabaseError::message("restored directory has no path"))?;
            let projected_name = transaction.directory_projected_name(&(directory_id.to_string()), |row| row.get::<_, String>(0))?;
            Ok(projected_name)
        })
    }

    /// Permanently deletes the directory subtree and its placements. Canonical
    /// lifecycle tombstones survive independently for synchronization.
    pub fn purge_directory(&self, directory_id: &DirId) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            if canonical_directory(transaction, directory_id)?.lifecycle != Some(DirectoryLifecycleState::Deleted) {
                return Err(DatabaseError::message("folder is not in Trash"));
            }
            // Hidden descendants are detached in the display tree, so the walk
            // follows retained original parents through suppressed folders.
            let directories = trashed_subtree(transaction, vec![directory_id.to_string()])?;
            if directories.is_empty() {
                return Err(DatabaseError::message("folder is not in Trash"));
            }
            for id in directories {
                record_local(transaction, LibraryStateEntry::DirectoryLifecycle { dir_id: parse_dir_id(&id)?, value: DirectoryLifecycleState::Purged })?;
            }
            Ok(())
        })
    }

    /// Permanently purges every current Trash entry in one transaction.
    /// Folder purging deliberately does not imply book purging, so both
    /// collections are processed explicitly.
    pub fn empty_trash(&self) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            let roots = query_rows(|read| transaction.empty_trash_roots(read), |row| row.get::<_, String>(0))?;
            let directories = trashed_subtree(transaction, roots)?;
            for id in directories {
                record_local(transaction, LibraryStateEntry::DirectoryLifecycle { dir_id: parse_dir_id(&id)?, value: DirectoryLifecycleState::Purged })?;
            }
            let books = query_rows(|read| transaction.empty_trash_records_select(read), |row| row.get::<_, BookId>(0))?;
            for hash in books {
                purge_book_in(transaction, &hash)?;
            }
            Ok(())
        })
    }
}

pub(crate) fn restore_book_placement_in(transaction: &Transaction<'_>, book_id: &BookId, directory_id: &DirId, materialize_retained: bool) -> Result<(), DatabaseError> {
    if !directory_exists_in(transaction, &directory_id.to_string())? {
        return Err(DatabaseError::message("folder does not exist or is in Trash"));
    }
    let previous = first_column::<Option<String>>(|read| transaction.restore_book_placement_record_select(&directory_id.to_string(), book_id, read))?.flatten();
    let title = first_column::<Option<String>>(|read| transaction.restore_book_placement_record_select_2(book_id, read))?.flatten();
    let adding = previous.is_none();
    let requested = previous.or(title).ok_or_else(|| DatabaseError::message("book does not exist"))?;
    let occupied = query_rows(|read| transaction.restore_book_placement_record_select_3(&directory_id.to_string(), book_id, read), |row| row.get::<_, String>(0))?;
    let chosen = library_replica::unique_file_name(&requested, occupied.iter().map(String::as_str));
    record_book_lifecycle(transaction, book_id, library_replica::BookLifecycleState::Present)?;
    project_dirty(transaction)?;
    place_book(transaction, directory_id, book_id, &chosen)?;
    project_dirty(transaction)?;
    #[cfg(target_arch = "wasm32")]
    let _ = materialize_retained;
    #[cfg(not(target_arch = "wasm32"))]
    if adding || materialize_retained {
        queue_materialization(transaction, book_id, directory_id)?;
    }
    if adding {
        {
            let transaction: &rusqlite::Transaction<'_> = transaction;
            let hash: &BookId = book_id;
            transaction.shared_request_book_download(hash, TransferOrigin::UserInitiated.storage(), unix_millis()?)?;
        };
    }
    Ok(())
}

pub(crate) fn move_directory_in(transaction: &Transaction<'_>, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, DatabaseError> {
    if let Some(name) = requested_name {
        validate_library_directory_name(name)?;
    }
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(DatabaseError::message("the library root cannot be renamed or moved"));
    }
    let existing: Option<(String, String)> = query_last(|read| transaction.live_directory_location(&directory_id.to_string(), read), |row| Ok((row.get(0)?, row.get(1)?)))?;
    let (current_parent, current_name) = existing.ok_or_else(|| DatabaseError::message("folder does not exist or is in Trash"))?;
    let current_parent = parse_dir_id(&current_parent)?;
    let parent_id = requested_parent.copied().unwrap_or(current_parent);
    if !directory_exists_in(transaction, &parent_id.to_string())? {
        return Err(DatabaseError::message("destination folder does not exist"));
    }
    let requested = requested_name.unwrap_or(&current_name);
    let chosen = choose_move_folder_name(transaction, &parent_id.to_string(), &directory_id.to_string(), requested)?;
    let (canonical_parent, canonical_name): (String, String) = transaction.directory_canonical_location(&(directory_id.to_string()), |row| Ok((row.get(0)?, row.get(1)?)))?;
    let parent_changed = requested_parent.is_some() && canonical_parent != parent_id.to_string();
    let name_changed = (requested_name.is_some() || chosen != current_name) && canonical_name != chosen;
    if parent_changed || name_changed || parent_id != current_parent || chosen != current_name {
        let closes_cycle = is_ancestor_or_self(transaction, &directory_id.to_string(), &parent_id.to_string())?;
        if closes_cycle {
            return Err(DatabaseError::message("directory move would create a cycle"));
        }
        // A rename must not adopt a repaired parent; a move must not adopt
        // a numbered display name unless it needs a new name at its destination.
        if parent_changed {
            record_local(transaction, LibraryStateEntry::DirectoryParent { dir_id: *directory_id, parent_id })?;
        }
        if name_changed {
            record_local(transaction, LibraryStateEntry::DirectoryName { dir_id: *directory_id, name: chosen.clone() })?;
        }
        queue_directory_work(transaction, directory_id)?;
    }
    project_dirty(transaction)?;
    // Moving can remove a collision suffix while retaining the name intent.
    let projected_name = transaction.directory_projected_name(&(directory_id.to_string()), |row| row.get::<_, String>(0))?;
    Ok(projected_name)
}

pub(crate) fn copy_directory_in(transaction: &Transaction<'_>, directory_id: &DirId, destination_parent_id: &DirId, next_id: &mut impl FnMut() -> DirId) -> Result<DirId, DatabaseError> {
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(DatabaseError::message("the library root cannot be copied"));
    }
    let name = first_column::<Option<String>>(|read| transaction.copy_directory_record_select(&directory_id.to_string(), read))?.flatten();
    let name = name.ok_or_else(|| DatabaseError::message("folder does not exist or is in Trash"))?;
    if !directory_exists_in(transaction, &destination_parent_id.to_string())? {
        return Err(DatabaseError::message("destination folder does not exist"));
    }
    // The subtree is read as the copy is written, so a destination inside
    // it would be copying a folder into itself; the same query that
    // rejects a cycle-forming move answers this.
    if is_ancestor_or_self(transaction, &directory_id.to_string(), &destination_parent_id.to_string())? {
        return Err(DatabaseError::message("a folder cannot be copied into itself"));
    }
    let occupied = query_rows(|read| transaction.copy_directory_record_select_2(&destination_parent_id.to_string(), read), |row| row.get::<_, String>(0))?;
    let chosen = library_replica::unique_folder_name(&name, occupied.iter().map(String::as_str));

    let copy_id = next_id();
    let mut pending = vec![(*directory_id, copy_id, chosen, *destination_parent_id)];
    let mut visited = HashSet::new();
    while let Some((source_id, copy_id, copy_name, copy_parent)) = pending.pop() {
        if !visited.insert(source_id) {
            continue;
        }
        upsert_dir(transaction, &copy_id, &copy_parent, &copy_name)?;
        queue_directory_work(transaction, &copy_id)?;
        let placements = query_rows(|read| transaction.copy_directory_record_select_3(&source_id.to_string(), read), |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)))?;
        for (hash, file_name, downloaded) in placements {
            // Names are already unique within the source folder and the
            // copy is empty, so each placement keeps the name it had.
            place_book(transaction, &copy_id, &hash, &file_name)?;
            project_dirty(transaction)?;
            #[cfg(target_arch = "wasm32")]
            if downloaded {
                transaction.shared_set_book_hash_downloaded(&hash, 1)?;
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = downloaded;
            copy_book_baseline(transaction, &source_id, &copy_id, &hash)?;
            #[cfg(not(target_arch = "wasm32"))]
            queue_materialization(transaction, &hash, &copy_id)?;
        }
        let children = query_rows(|read| transaction.copy_directory_record_select_4(&source_id.to_string(), read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        for (child_id, child_name) in children {
            pending.push((parse_dir_id(&child_id)?, next_id(), child_name, copy_id));
        }
    }
    Ok(copy_id)
}

pub(crate) fn copy_book_to_directory_in(transaction: &Transaction<'_>, book_id: &BookId, source_id: Option<&DirId>, destination_id: &DirId) -> Result<bool, DatabaseError> {
    if !directory_exists_in(transaction, &destination_id.to_string())? {
        return Err(DatabaseError::message("destination folder does not exist"));
    }
    if live_file_name(transaction, destination_id, book_id)?.is_some() {
        return Ok(false);
    }
    let source_id = match source_id {
        Some(source) => *source,
        None => {
            let source = first_column::<String>(|read| transaction.placements_copy_source(book_id, read))?;
            parse_dir_id(source.as_deref().ok_or_else(|| DatabaseError::message("book has no live folder placement"))?)?
        }
    };
    transfer_book_placement_in(transaction, book_id, &source_id, destination_id, false)?;
    Ok(true)
}

pub(crate) fn transfer_book_placement_in(transaction: &Transaction<'_>, book_id: &BookId, source_id: &DirId, destination_id: &DirId, remove_source: bool) -> Result<String, DatabaseError> {
    if source_id == destination_id {
        return Err(DatabaseError::message("source and destination folders are the same"));
    }
    // Native stores must materialize the requested filesystem placement;
    // a hash-addressed browser store has no separate physical placement.
    #[cfg(not(target_arch = "wasm32"))]
    let old_path = {
        let name = live_file_name(transaction, source_id, book_id)?.ok_or_else(|| DatabaseError::message("book is not present in the source folder"))?;
        placement_path(transaction, source_id, &name)?
    };
    let source: Option<(String, bool)> = query_last(|read| transaction.transfer_book_placement_record_select(&source_id.to_string(), book_id, read), |row| Ok((row.get(0)?, row.get(1)?)))?;
    let (source_name, downloaded): (String, bool) = source.ok_or_else(|| DatabaseError::message("book is not present in the source folder"))?;
    if !directory_exists_in(transaction, &destination_id.to_string())? {
        return Err(DatabaseError::message("destination folder does not exist"));
    }
    let existing = live_file_name(transaction, destination_id, book_id)?;
    let destination_name = if let Some(existing) = existing {
        existing
    } else {
        let chosen = {
            let connection: &rusqlite::Connection = transaction;
            let destination: &str = &destination_id.to_string();
            let requested: &str = &source_name;
            {
                let mut chosen = None;
                for number in 1..=8 {
                    let candidate = if number == 1 { requested.to_owned() } else { library_replica::numbered_file_name(requested, number) };
                    let mut occupied = false;
                    connection.placements_file_name_occupied(destination, &library_replica::portable_name_key(&candidate), |row| {
                        occupied = row.get(0)?;
                        Ok(())
                    })?;
                    if !occupied {
                        chosen = Some(candidate);
                        break;
                    }
                }
                match chosen {
                    Some(name) => name,
                    None => {
                        let names = occupied_file_names_in(connection, destination)?;
                        library_replica::unique_file_name(requested, names.iter().map(String::as_str))
                    }
                }
            }
        };
        place_book(transaction, destination_id, book_id, &chosen)?;
        project_dirty(transaction)?;
        #[cfg(target_arch = "wasm32")]
        if downloaded {
            transaction.shared_set_book_hash_downloaded(&book_id, 1)?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = downloaded;
        copy_book_baseline(transaction, source_id, destination_id, book_id)?;
        chosen
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Publish the destination before retiring the source. Only this
        // book's paths need work; no whole-library projection is needed.
        queue_materialization(transaction, book_id, destination_id)?;
        if remove_source && source_id != destination_id {
            queue_file_removal(transaction, book_id, &old_path)?;
        }
    }
    if remove_source {
        remove_placement(transaction, source_id, book_id, None)?;
    }
    Ok(destination_name)
}

pub(crate) fn purge_book_in(transaction: &rusqlite::Transaction<'_>, book_id: &BookId) -> Result<(), DatabaseError> {
    let book_row_id = first_column::<Option<i64>>(|read| transaction.purge_book_record_select(book_id, read))?.flatten();
    let book_row_id = book_row_id.ok_or_else(|| DatabaseError::message("book is not in Trash"))?;

    // Publish placement tombstones before removing their disposable local
    // rows. Other replicas use these before the Purged lifecycle arrives.
    let directories = query_rows(|callback| transaction.directory_live_book_folders(&book_row_id, callback), |row| row.get::<_, String>(0))?;
    for directory in directories {
        remove_placement(transaction, &parse_dir_id(&directory)?, book_id, None)?;
    }
    // Publish the delete intent directly. Projection performs the same physical
    // row deletion for local commands and received lifecycle winners.
    record_book_lifecycle(transaction, book_id, library_replica::BookLifecycleState::Purged)?;
    Ok(())
}

include_sql!("src/placements/sql/schema/tables.sql");
include_sql!("src/placements/sql/schema/index.sql");
include_sql!("src/placements/sql/schema/views.sql");
include_sql!("src/placements/sql/placements.sql");

// ---- scanner ----
// Database-owned contracts for filesystem discovery.
//
// A scanner receives one immutable inventory snapshot and returns observations.
// It never owns a connection, emits application events, or decides when work
// is retried. The library session owns that orchestration.

pub(crate) use book_model::BookFormat;

#[derive(Clone, Debug)]
pub struct ScanSnapshot {
    pub work_revision: i64,
    pub scan_id: i64,
    pub excluded_paths: Vec<String>,
    pub directories: Vec<ScanKnownDirectory>,
    pub books: Vec<ScanKnownBook>,
    pub placements: Vec<ScanPlacement>,
    pub tombstoned_files: Vec<(DirId, String)>,
}

/// Local placement state captured before filesystem I/O, including rows whose
/// fingerprint is empty (for example, a freshly downloaded book).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanPlacement {
    pub book_id: BookId,
    pub directory: DirId,
    pub name: String,
    pub path: String,
    pub fingerprint: String,
    pub downloaded: bool,
    pub projected_path: Option<String>,
}

#[cfg(feature = "scanner")]
pub(crate) fn scan_placements(connection: &rusqlite::Connection) -> Result<Vec<ScanPlacement>, DatabaseError> {
    let mut folders = FolderPaths::all(connection)?;
    scan_placements_with_folders(connection, &mut folders)
}

#[cfg(feature = "scanner")]
fn scan_placements_with_folders(connection: &rusqlite::Connection, folders: &mut FolderPaths<'_>) -> Result<Vec<ScanPlacement>, DatabaseError> {
    // A placement in a folder that is not live has no path on disk to scan.
    let mut placements = Vec::new();
    try_for_each_row(
        |read| connection.scanner_placements(read),
        |row| {
            let book_id = row.get::<_, BookId>(0)?;
            let directory = text_column(row, 1)?;
            let name = text_column(row, 2)?;
            let fingerprint = text_column(row, 3)?;
            let downloaded = row.get(4)?;
            let projected_path = optional_text_column(row, 5)?;
            let Some(folder) = folders.folder(directory)? else { return Ok(()) };
            placements.push(ScanPlacement {
                book_id,
                directory: parse_dir_id(directory)?,
                path: if folder.is_empty() { name.to_owned() } else { format!("{folder}/{name}") },
                name: name.to_owned(),
                fingerprint: fingerprint.to_owned(),
                downloaded,
                projected_path: projected_path.map(str::to_owned),
            });
            Ok(())
        },
    )?;
    Ok(placements)
}

#[cfg(feature = "scanner")]
fn scan_pending_paths(connection: &Connection, folders: &mut FolderPaths<'_>) -> Result<Vec<String>, DatabaseError> {
    let mut excluded_paths = Vec::new();
    let pending = serde_json::to_string(&pending_work_directories(connection)?).map_err(DatabaseError::operation)?;
    let pending_directories = query_rows(|read| connection.scanner_pending_directories(&pending, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)))?;
    for (id, remembered) in pending_directories {
        let live = folders.folder(&id)?;
        excluded_paths.extend(live.into_iter().chain(remembered.as_deref()).map(|path| path.trim_matches('/').to_owned()));
    }
    let mut pending_file_paths = query_rows(|read| connection.scanner_pending_files(read), |row| row.get::<_, String>(0))?;
    let pending_placements = query_rows(|read| connection.scanner_pending_placements(read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    for (dir_id, file_name) in pending_placements {
        pending_file_paths.extend(folders.placement(&dir_id, &file_name)?.map(|path| path.to_string()));
    }
    excluded_paths.extend(pending_file_paths.into_iter().map(|path| path.trim_matches('/').to_owned()));
    Ok(excluded_paths)
}

#[cfg(feature = "scanner")]
fn scan_directories(connection: &Connection, folders: &mut FolderPaths<'_>) -> Result<Vec<ScanKnownDirectory>, DatabaseError> {
    let directories = query_rows(|read| connection.scanner_known_directories(read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, bool>(3)?)))?;
    let directories = directories
        .into_iter()
        .map(|(id, parent_id, name, live)| {
            Ok(ScanKnownDirectory {
                path: if live { folders.folder(&id)?.map(str::to_owned) } else { None },
                id: DirId::parse_str(&id).map_err(DatabaseError::operation)?,
                parent_id: DirId::parse_str(&parent_id).map_err(DatabaseError::operation)?,
                name,
                deleted: !live,
            })
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;
    Ok(directories)
}

/// Keep concurrent edits out of this pass without abandoning unrelated files.
#[cfg(feature = "scanner")]
fn changed_scan_paths(connection: &Connection, snapshot: &ScanSnapshot, placements: &[ScanPlacement], folders: &mut FolderPaths<'_>) -> Result<Vec<String>, DatabaseError> {
    let mut paths = scan_pending_paths(connection, folders)?;
    let old = snapshot.placements.iter().map(|placement| ((placement.directory, placement.book_id), placement)).collect::<HashMap<_, _>>();
    let current = placements.iter().map(|placement| ((placement.directory, placement.book_id), placement)).collect::<HashMap<_, _>>();
    for (before, after) in [(&old, &current), (&current, &old)] {
        for (key, placement) in before {
            if after.get(key) != Some(placement) {
                paths.push(placement.path.clone());
                paths.extend(placement.projected_path.iter().cloned());
            }
        }
    }
    let directories = scan_directories(connection, folders)?;
    let old = snapshot.directories.iter().map(|directory| (directory.id, directory)).collect::<HashMap<_, _>>();
    let current = directories.iter().map(|directory| (directory.id, directory)).collect::<HashMap<_, _>>();
    for (before, after) in [(&old, &current), (&current, &old)] {
        for (key, directory) in before {
            if after.get(key) != Some(directory) {
                paths.extend(directory.path.iter().cloned());
            }
        }
    }
    // A newly trashed book may no longer have any live placement to compare.
    let tombstoned = query_rows(|read| connection.scanner_tombstoned_files(read), |row| Ok((parse_uuid_column(row.get(0)?, 0)?, row.get::<_, String>(1)?)))?;
    for (directory, name) in tombstoned {
        if !snapshot.tombstoned_files.contains(&(directory, name.clone())) {
            paths.extend(folders.placement(&directory.to_string(), &name)?.map(|path| path.to_string()));
        }
    }
    Ok(paths)
}

#[cfg(feature = "scanner")]
fn scan_path_in(excluded_paths: &[String], path: &str) -> bool {
    let path = Path::new(path.trim_matches('/'));
    excluded_paths.iter().any(|excluded| {
        let excluded = Path::new(excluded.trim_matches('/'));
        path.starts_with(excluded) || excluded.starts_with(path)
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanKnownDirectory {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: String,
    pub path: Option<String>,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub struct ScanKnownBook {
    pub book_id: BookId,
    pub directory: DirId,
    pub name: String,
    pub fingerprint: u64,
    pub inspection_version: Option<i64>,
    /// Complete playback facts for these verified bytes, including facts
    /// received through sync. This does not claim a local inspection occurred.
    pub cached_audiobook_format: Option<BookFormat>,
    pub cached_pdf: Option<Arc<CachedPdfReader>>,
}

#[derive(Clone, Debug, Default)]
pub struct FilesystemScan {
    pub directories: Vec<ScanDirectoryObservation>,
    pub files: Vec<ScanFileObservation>,
    pub readable: bool,
}

#[derive(Clone, Debug)]
pub struct ScanDirectoryObservation {
    pub id: DirId,
    pub parent_id: DirId,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct ScanFileObservation {
    pub directory: DirId,
    pub name: String,
    pub path: String,
    pub fingerprint: u64,
}

#[derive(Clone, Debug, Default)]
pub struct InspectedFilesystemScan {
    pub books: Vec<ScannedBook>,
    pub readable: bool,
}

#[derive(Clone, Debug)]
pub struct ScannedBook {
    pub source_identity: Option<PdfSourceIdentity>,
    pub audiobook_content_hash: Option<ContentHash>,
    pub byte_checksum: Option<ContentHash>,
    pub directory: DirId,
    pub name: String,
    pub path: String,
    pub fingerprint: u64,
    pub book_id: BookId,
    pub size_bytes: u64,
    pub extension: String,
    /// The book's own metadata (title, authors, TOC, ...), extracted the
    /// same way an explicit import extracts it. The scanner omits a book
    /// from a pass entirely rather than including one with no inspection:
    /// a bare `content_hash`/no metadata here would be indistinguishable
    /// from "reuse the cached inspection," which does not exist yet for a
    /// book discovered for the first time.
    pub inspection: InspectedBook,
    pub inspection_version: i64,
}

impl Database {
    /// Takes the immutable inventory view required by filesystem discovery. The
    /// connection is released before the caller walks or hashes any path.
    /// The scanner supplies its extractor version to select reusable PDF facts.
    #[cfg(feature = "scanner")]
    pub fn scan_snapshot(&self, pdf_inspection_version: i64) -> Result<ScanSnapshot, DatabaseError> {
        self.with_write_transaction(|transaction| {
            let connection = &*transaction;
            let work_revision = connection.scanner_file_work_revision(|row| row.get(0))?;
            let sequence = first_column::<String>(|read| connection.shared_scanner_sequence(read))?;
            let current = sequence.map(|value| value.parse::<i64>().map_err(DatabaseError::operation)).transpose()?.unwrap_or(0);
            let scan_id = current.checked_add(1).ok_or_else(|| DatabaseError::message("scan sequence overflow"))?;
            connection.scanner_set_sequence(&scan_id.to_string())?;
            let mut folders = FolderPaths::all(connection)?;
            let excluded_paths = scan_pending_paths(connection, &mut folders)?;
            let mut books = Vec::new();
            connection.scanner_known_books(|row| {
                let Ok(fingerprint) = row.get::<_, String>(0)?.parse::<u64>() else { return Ok(()) };
                let book_id = row.get::<_, BookId>(1)?;
                let mut inspection_version = row.get::<_, Option<i64>>(4)?;
                let format = BookFormat::from_code(row.get(5)?);
                let needs_content_hash = format == Some(BookFormat::Mp3Folder) && !connection.query_row("SELECT EXISTS(SELECT 1 FROM local_audiobook_content WHERE book_id=?1)", [book_id], |row| row.get::<_, bool>(0))?;
                if needs_content_hash {
                    inspection_version = None;
                }
                let cached_audiobook_format = if !needs_content_hash && inspection_version.is_none() && matches!(format, Some(BookFormat::M4b | BookFormat::Mp3Folder)) {
                    crate::books::audiobook::reusable_playback_format(book_id, format, row.get(6)?, optional_blob_column(row, 7)?, optional_blob_column(row, 8)?)
                } else {
                    None
                };
                books.push(ScanKnownBook { book_id, directory: parse_uuid_column(row.get(2)?, 2)?, name: row.get(3)?, fingerprint, inspection_version, cached_audiobook_format, cached_pdf: None });
                Ok(())
            })?;
            for book in &mut books {
                if BookFormat::from_file_name(&book.name) == Some(BookFormat::Pdf) && book.inspection_version != Some(pdf_inspection_version) && book.inspection_version.is_none_or(|version| version >= 0) {
                    if let Some(checksum) = self.local_content_hash(book.book_id, book.directory)? {
                        book.cached_pdf = self.cached_pdf_reader(BookId::from(checksum))?.map(Arc::new);
                    }
                }
            }
            let directories = scan_directories(connection, &mut folders)?;
            let tombstoned_files = query_rows(|read| connection.scanner_tombstoned_files(read), |row| Ok((parse_uuid_column(row.get(0)?, 0)?, row.get::<_, String>(1)?)))?;
            // The inventory above already resolved live folders. Reuse those
            // paths instead of walking their ancestry again per folder.
            let placements = scan_placements_with_folders(connection, &mut folders)?;
            ({
                use crate::assets::ActivitySql;

                connection.activity_begin_scan()?;
            });
            Ok(Commit(ScanSnapshot { work_revision, scan_id, excluded_paths, directories, books, placements, tombstoned_files }))
        })
    }

    /// Publish inspected books incrementally. Only the final, readable batch
    /// may reconcile missing files. Concurrent edits exclude affected paths and
    /// request one follow-up scan; unrelated observations still commit.
    #[cfg(feature = "scanner")]
    pub fn apply_filesystem_scan_batch(
        &self, snapshot: &mut ScanSnapshot, discovery: &FilesystemScan, inspected: InspectedFilesystemScan, complete: bool, matcher: Option<crate::ContributorNameMatcher>,
    ) -> Result<ScanApplyResult, DatabaseError> {
        let _call = ProfileSpan::new("scan_database_apply", "batch");
        // Normalize metadata and serialize inspection data before acquiring
        // the writer. Check concurrent edits under the writer so affected
        // paths can wait for a follow-up without interrupting this pass.
        let reconcile_missing = complete && discovery.readable && inspected.readable;
        let preparation = ProfileSpan::new("database_batch_preparation", "batch");
        let prepared_books = inspected
            .books
            .into_iter()
            .map(|book| {
                let format = BookFormat::from_file_name(&format!("book.{}", book.extension)).ok_or_else(|| DatabaseError::message(format!("unsupported scanned extension: {}", book.extension)))?;
                let metadata = ProfileSpan::new("metadata_normalization", &book.name);
                let prepared = crate::import::prepare_inspection(book.inspection, &book.book_id, format)?;
                drop(metadata);
                let checksum = book.byte_checksum.or_else(|| if book.audiobook_content_hash.is_none() { book.book_id.content_hash() } else { None });
                let identity_profile = ProfileSpan::new("database_identity_lookup", &book.name);
                let identity = if let Some(content_hash) = book.audiobook_content_hash {
                    let unchanged = snapshot.books.iter().find(|known| known.directory == book.directory && known.name == book.name && known.fingerprint == book.fingerprint).map(|known| known.book_id);
                    let known = match checksum {
                        Some(checksum) => self.book_for_local_content(checksum)?.or(unchanged),
                        None => unchanged,
                    };
                    self.resolve_audiobook_content(content_hash, known)?
                } else {
                    self.resolve_scanned_content(checksum.ok_or_else(|| DatabaseError::message("scanned bytes require a content checksum"))?, book.source_identity)?
                };
                drop(identity_profile);
                let prepared = prepared.with_identity(identity);
                let catalog = ProfileSpan::new("catalog_lookup", &book.name);
                let catalog_record = crate::catalog::import_record(book.book_id, format)?;
                drop(catalog);
                let import = crate::import::ImportCommit {
                    source_identity: book.source_identity,
                    byte_checksum: checksum,
                    audiobook_content_hash: book.audiobook_content_hash,
                    parent_id: book.directory,
                    file_name: book.name.clone(),
                    format,
                    book_id: identity,
                    size_bytes: book.size_bytes,
                    inspection_version: book.inspection_version,
                    inspection: None,
                    published: library_model::PublishedStagedBook { name: book.name, relative_path: book.path, published: true, fingerprint: Some(book.fingerprint.to_string()) },
                    request_thumbnail: true,
                    restore_paths: Vec::new(),
                };
                Ok((import, prepared, catalog_record, checksum))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        drop(preparation);
        let mut next_guard = None;
        let waiting = ProfileSpan::new("database_writer_wait", "batch");
        let result = self.with_write_transaction(|transaction| {
            drop(waiting);
            let _body = ProfileSpan::new("scan_transaction_body", "batch");
            let guard = ProfileSpan::new("placement_guard", "batch");
            let current_revision: i64 = transaction.scanner_file_work_revision(|row| row.get(0))?;
            let current_sequence = first_column::<String>(|read| transaction.shared_scanner_sequence(read))?.unwrap_or_default();
            let placements = scan_placements(transaction)?;
            let mut needs_rescan = current_revision != snapshot.work_revision || current_sequence != snapshot.scan_id.to_string() || placements != snapshot.placements;
            let mut excluded_paths = snapshot.excluded_paths.clone();
            if needs_rescan {
                let mut folders = FolderPaths::all(transaction)?;
                excluded_paths.extend(changed_scan_paths(transaction, snapshot, &placements, &mut folders)?);
                excluded_paths.sort_unstable();
                excluded_paths.dedup();
                log::debug!(target: "library_add_profile", "follow_up_scan=file_work_or_placements expected_revision={} actual_revision={current_revision} excluded_paths={}", snapshot.work_revision, excluded_paths.len());
            }
            // Import projection also queues work for requested filesystem
            // changes. A scan observes files already in place, so acknowledge
            // only the new work its own admission creates, preserving commands
            // that were queued before this transaction acquired the writer.
            let pending_book_work = query_rows(|read| transaction.scanner_pending_book_work(read), |row| row.get::<_, BookId>(0))?.into_iter().collect::<HashSet<_>>();
            drop(guard);
            let directories = ProfileSpan::new("directory_updates", "batch");
            for directory in &discovery.directories {
                if excluded_paths.iter().any(|excluded| Path::new(&directory.path).starts_with(Path::new(excluded))) {
                    continue;
                }
                if canonical_directory(transaction, &directory.id)?.lifecycle.is_some_and(|state| state != DirectoryLifecycleState::Present) {
                    excluded_paths.push(directory.path.clone());
                    needs_rescan = true;
                    continue;
                }
                // A retained physical path can be a local collision alias after
                // relocation. Observing it again must not rewrite synced intent.
                let retained: bool = transaction.scanner_directory_projection_retained(&directory.id.to_string(), &directory.parent_id.to_string(), &directory.path, |row| row.get(0))?;
                if !retained {
                    let existing: bool = transaction.scanner_directory_exists(&directory.id.to_string(), |row| row.get(0))?;
                    if existing {
                        // Existing-directory edits keep sequential cycle checks:
                        // their parents must include earlier admissions here.
                        project_dirty(transaction)?;
                        upsert_dir(transaction, &directory.id, &directory.parent_id, &directory.name)?;
                    } else {
                        // Discovery already supplies unique IDs in an acyclic
                        // physical tree. Admit new folders together, then build
                        // the directory projection once before importing books.
                        record_directory(transaction, &directory.id, &directory.parent_id, &directory.name)?;
                    }
                }
            }
            project_dirty(transaction)?;
            drop(directories);
            let mut refreshed = HashSet::new();
            let mut pending = Vec::with_capacity(prepared_books.len());
            let mut pending_books = HashSet::new();
            let mut pending_names = HashSet::new();
            let flush = |pending: &mut Vec<(ImportObservation, BookId, DirId, Option<ContentHash>)>| -> Result<(), DatabaseError> {
                if pending.is_empty() {
                    return Ok(());
                }
                // File observations require live placement rows. Project the
                // admitted metadata together before recording those observations.
                project_dirty(transaction)?;
                for (observation, book_id, directory, checksum) in pending.drain(..) {
                    observation.record(transaction)?;
                    if !pending_book_work.contains(&book_id) {
                        transaction.scanner_acknowledge_observed_book(&book_id)?;
                    }
                    let observation = ProfileSpan::new("checksum_observation", "book");
                    if let Some(checksum) = checksum {
                        crate::content_identity::observe_content(transaction, book_id, directory, checksum)?;
                    }
                    drop(observation);
                    if checksum.is_some() && crate::content_identity::expected_hash(transaction, book_id)?.is_none() {
                        transaction.execute("UPDATE book_transfer SET upload_checksum=?2 WHERE content_hash=?1 AND upload_id IS NOT NULL", rusqlite::params![book_id, checksum])?;
                    }
                }
                Ok(())
            };
            'books: for (import, prepared, catalog_record, checksum) in prepared_books {
                let _book = ProfileSpan::new("book_import", &import.file_name);
                if scan_path_in(&excluded_paths, &import.published.relative_path) {
                    continue;
                }
                if directory_relative_path(transaction, &import.parent_id)?.is_none() {
                    needs_rescan = true;
                    continue;
                }
                if let Some((body, _)) = canonical_body(transaction, "book_lifecycle", &StateKey::Book(import.book_id), "")? {
                    let state: LibraryStateEntry = wire::decode(&body, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
                    if matches!(state, LibraryStateEntry::BookLifecycle { value, .. } if value != library_replica::BookLifecycleState::Present) {
                        needs_rescan = true;
                        continue;
                    }
                }
                // Scanned books commit through the same identity-resolution and
                // placement logic explicit import uses, so a scan-discovered book
                // gets its title, authors, and subjects just like an imported one.
                let book_id = import.book_id;
                if reconcile_missing {
                    refreshed.insert((import.parent_id, book_id));
                }
                let name = (import.parent_id, library_replica::portable_name_key(&import.published.name));
                // Repeated identities need their original projected facts;
                // repeated destination names need live collision checks.
                if pending_books.contains(&book_id) || pending_names.contains(&name) {
                    flush(&mut pending)?;
                    pending_books.clear();
                    pending_names.clear();
                }
                // Only scanning may replace the old identity at a path already
                // observed on disk. Ordinary imports retain their collision check.
                for old in snapshot.placements.iter().filter(|old| old.directory == import.parent_id && old.name == import.file_name && old.book_id != book_id) {
                    if scan_path_in(&excluded_paths, &old.path) {
                        continue 'books;
                    }
                    // Finish observations before removing a placement that an
                    // earlier book in this batch may still refer to.
                    flush(&mut pending)?;
                    pending_books.clear();
                    pending_names.clear();
                    remove_placement(transaction, &old.directory, &old.book_id, None)?;
                    transaction.directory_clear_placement_hash(&(old.directory.to_string()), &(&old.book_id))?;
                    project_dirty(transaction)?;
                    forget_file(transaction, &old.directory.to_string(), &old.book_id)?;
                }
                let directory = import.parent_id;
                let admission = ProfileSpan::new("book_admission", "book");
                let (_, observation) = crate::import::admit_import(transaction, import, Some(prepared), matcher, None)?;
                drop(admission);
                pending.push((observation, book_id, directory, checksum));
                pending_books.insert(book_id);
                pending_names.insert(name);
                if let Some(record) = catalog_record {
                    crate::enrichment::facts::record_catalog_answer(transaction, &book_id, record)?;
                }
            }
            flush(&mut pending)?;
            let finalization = ProfileSpan::new("scan_reconciliation", "batch");
            if complete {
                let known: HashMap<_, _> = snapshot.placements.iter().map(|old| ((old.directory, old.name.as_str()), old)).collect();
                for file in &discovery.files {
                    if scan_path_in(&excluded_paths, &file.path) {
                        continue;
                    }
                    if let Some(old) = known.get(&(file.directory, file.name.as_str())).filter(|old| old.fingerprint == file.fingerprint.to_string())
                        && (!old.downloaded || old.projected_path.as_deref().map(|path| path.trim_matches('/')) != Some(file.path.trim_matches('/')))
                    {
                        crate::native::observations::record_file(transaction, &file.directory.to_string(), &old.book_id, &file.path, Some(&old.fingerprint), crate::native::observations::Revision::Retained)?;
                    }
                }
                if reconcile_missing && !needs_rescan {
                    let observed = discovery.files.iter().map(|file| (file.directory, file.name.as_str())).collect::<HashSet<_>>();
                    let observed_paths = discovery.files.iter().map(|file| file.path.trim_matches('/')).collect::<HashSet<_>>();
                    for old in &snapshot.placements {
                        if !old.downloaded && old.projected_path.is_none() && old.fingerprint.is_empty() {
                            continue;
                        }
                        if observed.contains(&(old.directory, old.name.as_str()))
                            || refreshed.contains(&(old.directory, old.book_id))
                            || scan_path_in(&excluded_paths, &old.path)
                            || old.projected_path.as_deref().is_some_and(|path| scan_path_in(&excluded_paths, path) || observed_paths.contains(path.trim_matches('/')))
                        {
                            continue;
                        }
                        // Absence is local availability, not a synchronized deletion.
                        forget_file(transaction, &old.directory.to_string(), &old.book_id)?;
                    }
                }
            }
            drop(finalization);
            if complete {
                ({
                    use crate::assets::ActivitySql;
                    let connection: &rusqlite::Connection = transaction;
                    let readable: bool = discovery.readable;

                    connection.activity_finish_scan(if readable { "idle" } else { "failed" })?;
                });
            }
            // Include our own writes in the next guard, retaining exclusions for
            // concurrent edits until the follow-up walk sees their final paths.
            project_dirty(transaction)?;
            #[cfg(not(target_arch = "wasm32"))]
            for directory in &discovery.directories {
                if excluded_paths.iter().any(|excluded| Path::new(&directory.path).starts_with(Path::new(excluded))) {
                    continue;
                }
                if directory_relative_path(transaction, &directory.id)?.as_deref() == Some(directory.path.trim_matches('/')) {
                    use crate::native::NativeFileWorkSql;
                    transaction.native_set_directory_projection(&directory.id.to_string(), &directory.path)?;
                    transaction.directory_clear_work(&(directory.id.to_string()))?;
                }
            }
            let _refresh = ProfileSpan::new("scan_guard_refresh", "batch");
            let mut folders = FolderPaths::all(transaction)?;
            next_guard = Some((transaction.scanner_file_work_revision(|row| row.get(0))?, scan_placements_with_folders(transaction, &mut folders)?, scan_directories(transaction, &mut folders)?, excluded_paths));
            Ok(Commit(if needs_rescan { ScanApplyResult::AppliedNeedsRescan } else { ScanApplyResult::Applied }))
        })?;
        if let Some((revision, placements, directories, excluded_paths)) = next_guard {
            snapshot.work_revision = revision;
            snapshot.placements = placements;
            snapshot.directories = directories;
            snapshot.excluded_paths = excluded_paths;
        }
        Ok(result)
    }
}

// Only filesystem scanning reads and writes through these queries.
#[cfg(feature = "scanner")]
include_sql!("src/placements/sql/scanner.sql");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanApplyResult {
    Applied,
    /// Unaffected observations committed; concurrent edits need a later walk.
    AppliedNeedsRescan,
}

// ---- paths ----
// Read-only book metadata used to resolve physical storage.
//
// The filesystem layer receives the resulting values; it never opens the
// library database to discover them.

/// Database facts required to open a locally materialized book.
/// Filesystem code receives only `path`; it never consults SQLite itself.
pub struct LocalBookInfo {
    pub format: book_model::BookFormat,
    pub path: Option<RelativeBookPath>,
}

impl Database {
    pub fn pdf_metadata(&self, hash: &BookId) -> Result<Option<PdfReaderMetadata>, DatabaseError> {
        let bytes = first_column::<Vec<u8>>(|read| self.connection.stored_pdf_metadata(hash, read))?;
        Ok(bytes.and_then(|bytes| crate::stored_document::decode::<PdfReaderMetadata>(&bytes).ok()).filter(|metadata| metadata.valid_for(&hash.to_hex())))
    }

    pub fn local_book_info(&self, hash: &BookId) -> Result<LocalBookInfo, DatabaseError> {
        let connection = &self.connection;
        let format = first_column::<Option<i64>>(|read| connection.shared_book_format(hash, read))?;
        let format = format.flatten().and_then(BookFormat::from_code).ok_or_else(|| DatabaseError::message("book format missing"))?;
        let placements = query_rows(|read| connection.readable_placements(hash, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)))?;
        // Prefer a placement with local bytes, so the reader opens bytes that
        // are present instead of failing on an arbitrarily chosen empty one.
        let mut folders = FolderPaths::new(connection);
        let mut candidates = Vec::new();
        for (dir_id, file_name, downloaded) in placements {
            #[cfg(not(target_arch = "wasm32"))]
            if downloaded {
                let projected = connection.query_row("SELECT relative_path FROM local_file_projection WHERE content_hash=?1 AND dir_id=?2", (hash, &dir_id), |row| row.get::<_, String>(0)).optional()?;
                if let Some(path) = projected {
                    candidates.push((true, stored_book_path(&path)?));
                    continue;
                }
            }
            if let Some(path) = folders.placement(&dir_id, &file_name)? {
                candidates.push((downloaded, path));
            }
        }
        let path = candidates.into_iter().min_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1))).map(|(_, path)| path);
        Ok(LocalBookInfo { format, path })
    }

    /// Every remembered native path for a book, including a source
    /// retained while a move is pending.
    pub fn book_paths(&self, hash: &BookId) -> Result<Vec<RelativeBookPath>, DatabaseError> {
        book_paths_in(&self.connection, hash)
    }

    /// Directory identities whose physical markers belong to this library,
    /// including retained and trashed folders.
    pub fn registered_library_directory_ids(&self) -> Result<HashSet<DirId>, DatabaseError> {
        Ok(self.connection.prepare_cached("SELECT id FROM dir")?.query_map([], |row| row.get::<_, String>(0))?.map(|row| parse_dir_id(&row?)).collect::<Result<_, _>>()?)
    }

    /// Registered physical files, including retained and trashed placements.
    /// A root move must never infer ownership from a directory listing.
    pub fn registered_library_files(&self) -> Result<Vec<(BookId, RelativeBookPath)>, DatabaseError> {
        let connection = &self.connection;
        let snapshot = connection.is_autocommit().then(|| connection.unchecked_transaction()).transpose()?;
        let mut files = query_rows(|read| connection.registered_file_projection_paths(read), |row| Ok((row.get::<_, BookId>(0)?, stored_book_path_column(row, 1)?)))?;
        let mut folders = FolderPaths::all(connection)?;
        try_for_each_row(
            |read| connection.registered_file_placements(read),
            |row| {
                if let Some(path) = folders.placement(text_column(row, 1)?, text_column(row, 2)?)? {
                    files.push((row.get(0)?, path));
                }
                Ok(())
            },
        )?;
        if let Some(snapshot) = snapshot {
            snapshot.commit()?;
        }
        Ok(files)
    }

    /// Paths that contribute to local book storage usage.
    pub fn local_book_paths(&self) -> Result<Vec<RelativeBookPath>, DatabaseError> {
        let connection = &self.connection;
        let mut paths = query_rows(|read| connection.local_book_projection_paths(read), |row| stored_book_path_column(row, 0))?;
        let placements = query_rows(|read| connection.local_book_placements(read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut resolver = FolderPaths::for_directories(connection, placements.iter().map(|(id, _)| id.as_str()))?;
        for (dir_id, file_name) in placements {
            paths.extend(resolver.placement(&dir_id, &file_name)?);
        }
        let mut seen = HashSet::new();
        paths.retain(|path| seen.insert(path.clone()));
        Ok(paths)
    }

    /// Load cached PDF metadata for the selected placement without checking the file.
    pub fn local_pdf_metadata(&self, hash: &BookId, relative_path: &str) -> Result<Option<pdf_view_common::PdfReaderMetadata>, DatabaseError> {
        let Some((current, directory, _)) = book_at_path(&self.connection, relative_path)? else { return Ok(None) };
        if current != *hash {
            return Ok(None);
        }
        if let Some(local) = self.local_content_hash(*hash, directory)? {
            if let Some(canonical) = self.canonical_content_hash(*hash)? {
                if local != canonical {
                    return Ok(None);
                }
            } else {
                // Before the first upload, an edited file may have outgrown
                // its immutable import metadata. Let the reader inspect it.
                let revisions: i64 = self.connection.query_row("SELECT count(*) FROM local_content_alias WHERE book_id=?1", [hash], |row| row.get(0))?;
                if revisions > 1 {
                    return Ok(None);
                }
            }
        }
        self.pdf_metadata(hash)
    }

    /// Live file names already occupied in a destination folder. Physical
    /// name collisions are checked separately by the file store.
    pub fn shared_occupied_file_names(&self, destination: &DirId) -> Result<Vec<String>, DatabaseError> {
        occupied_file_names_in(&self.connection, &destination.to_string())
    }
}

include_sql!("src/placements/sql/book_paths.sql");

fn record_book_lifecycle(connection: &rusqlite::Connection, hash: &BookId, value: library_replica::BookLifecycleState) -> Result<(), DatabaseError> {
    record_local(connection, LibraryStateEntry::BookLifecycle { book_id: *hash, value })
}

fn remove_placement(connection: &rusqlite::Connection, directory: &DirId, hash: &BookId, origin_folder_id: Option<String>) -> Result<(), DatabaseError> {
    connection.directory_clear_placement_download(&(directory.to_string()), &(&hash))?;
    record_local(connection, LibraryStateEntry::Placement { dir_id: *directory, book_id: *hash, present: false, origin_folder_id })
}

include_sqlite_sql::include_sql!("src/placements/sql/directory_operations.sql");

/// Transaction-scoped source removal shared with cross-library moves.
pub(crate) fn remove_book_placement_in(transaction: &rusqlite::Transaction<'_>, book_id: &BookId, source_id: &DirId) -> Result<bool, DatabaseError> {
    if !transaction.remove_book_placement_record_select(&source_id.to_string(), book_id, |row| row.get::<_, bool>(0))? {
        return Err(DatabaseError::message("book is not present in this folder"));
    }
    let orphaned = transaction.remove_book_placement_record_select_2(book_id, &source_id.to_string(), |row| row.get(0))?;
    if orphaned {
        // Removing the last copy trashes the book, retaining its placement
        // so restore does not have to reconstruct membership from clocks.
        record_book_lifecycle(transaction, book_id, library_replica::BookLifecycleState::Deleted { origin_folder_id: Some(source_id.to_string()) })?;
    } else {
        remove_placement(transaction, source_id, book_id, None)?;
    }
    Ok(orphaned)
}

/// Transaction-scoped source removal shared with cross-library moves.
pub(crate) fn trash_directory_in(transaction: &rusqlite::Transaction<'_>, directory_id: &DirId) -> Result<(), DatabaseError> {
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(DatabaseError::message("the library root cannot be moved to Trash"));
    }
    if !directory_exists_in(transaction, &directory_id.to_string())? {
        return Err(DatabaseError::message("folder does not exist or is already in Trash"));
    }
    let subtree = serde_json::to_string(&walk_down(transaction, vec![directory_id.to_string()], false)?).map_err(DatabaseError::operation)?;
    let hashes = query_rows(|read| transaction.trash_subtree_only_books(&subtree, read), |row| row.get::<_, BookId>(0))?;
    for hash in hashes {
        record_book_lifecycle(transaction, &hash, library_replica::BookLifecycleState::Deleted { origin_folder_id: Some(directory_id.to_string()) })?;
    }
    // Only books still present elsewhere need independent placement removals.
    project_dirty(transaction)?;
    let removed = query_rows(|callback| transaction.directory_live_subtree_placements(&(&subtree), callback), |row| Ok((row.get::<_, String>(0)?, row.get::<_, BookId>(1)?)))?;
    for (directory, hash) in removed {
        remove_placement(transaction, &parse_dir_id(&directory)?, &hash, Some(directory_id.to_string()))?;
    }
    record_local(transaction, LibraryStateEntry::DirectoryLifecycle { dir_id: *directory_id, value: DirectoryLifecycleState::Deleted })?;
    Ok(())
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn dir_path_components(directory_id: &DirId) -> Vec<DirPathComponent>;
    arguments (DirId,) = (directory_id.clone(),);
    execute database, arguments => database.dir_path_components(&arguments.0);

    fn is_live_destination(directory: &DirId) -> bool;
    arguments (DirId,) = (directory.clone(),);
    execute database, arguments => database.is_live_destination(&arguments.0);

    fn book_placement_expects_hash(placement: &BookPlacement, book_id: &BookId) -> bool;
    arguments (BookPlacement, BookId,) = (placement.clone(), book_id.clone(),);
    execute database, arguments => database.book_placement_expects_hash(&arguments.0, &arguments.1);

    fn child_directory(parent_id: &DirId, name: &str) -> Option<DirId>;
    arguments (DirId, String,) = (parent_id.clone(), name.to_owned(),);
    execute database, arguments => database.child_directory(&arguments.0, &arguments.1);

    fn has_book_named(dir_id: &DirId, file_name: &str) -> bool;
    arguments (DirId, String,) = (dir_id.clone(), file_name.to_owned(),);
    execute database, arguments => database.has_book_named(&arguments.0, &arguments.1);

    fn create_directory(parent_id: &DirId, name: &FileName) -> Dir;
    arguments (DirId, FileName,) = (parent_id.clone(), name.clone(),);
    execute database, arguments => database.create_directory(&arguments.0, &arguments.1);

    fn create_directory_with_id(dir_id: &DirId, parent_id: &DirId, name: &FileName) -> Dir;
    arguments (DirId, DirId, FileName,) = (dir_id.clone(), parent_id.clone(), name.clone(),);
    execute database, arguments => database.create_directory_with_id(&arguments.0, &arguments.1, &arguments.2);

    fn directory_creation_path(parent_id: &DirId, name: &str) -> (String, String);
    arguments (DirId, String,) = (parent_id.clone(), name.to_owned(),);
    execute database, arguments => database.directory_creation_path(&arguments.0, &arguments.1);

    fn register_created_directory(id: &DirId, parent_id: &DirId, name: &str, path: &str) -> Dir;
    arguments (DirId, DirId, String, String,) = (id.clone(), parent_id.clone(), name.to_owned(), path.to_owned(),);
    execute database, arguments => database.register_created_directory(&arguments.0, &arguments.1, &arguments.2, &arguments.3);

    fn register_created_directories(directories: &[CreatedDirectory]) -> Vec<Dir>;
    arguments (Vec<CreatedDirectory>,) = (directories.to_vec(),);
    execute database, arguments => database.register_created_directories(&arguments.0);

    fn can_recover_import_directory(id: DirId) -> bool;
    arguments (DirId,) = (id,);
    execute database, arguments => database.can_recover_import_directory(arguments.0);

    fn folder_destinations() -> Vec<LibraryFolderDestination>;
    arguments () = ();
    execute database, arguments => database.folder_destinations();

    fn move_directory(directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> String;
    arguments (DirId, Option<DirId>, Option<String>,) = (directory_id.clone(), requested_parent.cloned(), requested_name.map(str::to_owned),);
    execute database, arguments => database.move_directory(&arguments.0, arguments.1.as_ref(), arguments.2.as_deref());

    fn copy_directory(directory_id: &DirId, destination_parent_id: &DirId) -> DirId;
    arguments (DirId, DirId,) = (directory_id.clone(), destination_parent_id.clone(),);
    execute database, arguments => database.copy_directory(&arguments.0, &arguments.1);

    fn restore_book_placement(book_id: &BookId, directory_id: &DirId) -> ();
    arguments (BookId, DirId,) = (book_id.clone(), directory_id.clone(),);
    execute database, arguments => database.restore_book_placement(&arguments.0, &arguments.1);

    fn copy_book_to_directory(book_id: &BookId, source_id: Option<&DirId>, destination_id: &DirId) -> bool;
    arguments (BookId, Option<DirId>, DirId,) = (book_id.clone(), source_id.cloned(), destination_id.clone(),);
    execute database, arguments => database.copy_book_to_directory(&arguments.0, arguments.1.as_ref(), &arguments.2);

    fn move_book_placement(book: &BookId, source: &DirId, destination: &DirId) -> bool;
    arguments (BookId, DirId, DirId,) = (book.clone(), source.clone(), destination.clone(),);
    execute database, arguments => database.move_book_placement(&arguments.0, &arguments.1, &arguments.2);

    fn remove_book_placement(book_id: &BookId, source_id: &DirId) -> bool;
    arguments (BookId, DirId,) = (book_id.clone(), source_id.clone(),);
    execute database, arguments => database.remove_book_placement(&arguments.0, &arguments.1);

    fn library_trash() -> LibraryTrashView;
    arguments () = ();
    execute database, arguments => database.library_trash();

    fn trash_book(book_id: &BookId) -> ();
    arguments (BookId,) = (book_id.clone(),);
    execute database, arguments => database.trash_book(&arguments.0);

    fn restore_book(book_id: &BookId) -> ();
    arguments (BookId,) = (book_id.clone(),);
    execute database, arguments => database.restore_book(&arguments.0);

    fn purge_book(book_id: &BookId) -> ();
    arguments (BookId,) = (book_id.clone(),);
    execute database, arguments => database.purge_book(&arguments.0);

    fn trash_directory(directory_id: &DirId) -> ();
    arguments (DirId,) = (directory_id.clone(),);
    execute database, arguments => database.trash_directory(&arguments.0);

    fn restore_directory(directory_id: &DirId, requested_parent: Option<&DirId>) -> String;
    arguments (DirId, Option<DirId>,) = (directory_id.clone(), requested_parent.cloned(),);
    execute database, arguments => database.restore_directory(&arguments.0, arguments.1.as_ref());

    fn purge_directory(directory_id: &DirId) -> ();
    arguments (DirId,) = (directory_id.clone(),);
    execute database, arguments => database.purge_directory(&arguments.0);

    fn empty_trash() -> ();
    arguments () = ();
    execute database, arguments => database.empty_trash();

    #[cfg(feature = "scanner")]
    fn scan_snapshot(pdf_inspection_version: i64) -> ScanSnapshot;
    arguments (i64,) = (pdf_inspection_version,);
    execute database, arguments => database.scan_snapshot(arguments.0);

    fn pdf_metadata(hash: &BookId) -> Option<PdfReaderMetadata>;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.pdf_metadata(&arguments.0);

    fn local_book_info(hash: &BookId) -> LocalBookInfo;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.local_book_info(&arguments.0);

    fn book_paths(hash: &BookId) -> Vec<RelativeBookPath>;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.book_paths(&arguments.0);

    fn registered_library_directory_ids() -> HashSet<DirId>;
    arguments () = ();
    execute database, arguments => database.registered_library_directory_ids();

    fn registered_library_files() -> Vec<(BookId, RelativeBookPath)>;
    arguments () = ();
    execute database, arguments => database.registered_library_files();

    fn local_book_paths() -> Vec<RelativeBookPath>;
    arguments () = ();
    execute database, arguments => database.local_book_paths();

    fn local_pdf_metadata(hash: &BookId, relative_path: &str) -> Option<pdf_view_common::PdfReaderMetadata>;
    arguments (BookId, String,) = (hash.clone(), relative_path.to_owned(),);
    execute database, arguments => database.local_pdf_metadata(&arguments.0, &arguments.1);

    fn shared_occupied_file_names(destination: &DirId) -> Vec<String>;
    arguments (DirId,) = (destination.clone(),);
    execute database, arguments => database.shared_occupied_file_names(&arguments.0);
}

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
impl crate::DatabaseClient {
    pub async fn apply_filesystem_scan_batch(
        &self, snapshot: &mut ScanSnapshot, discovery: &FilesystemScan, inspected: InspectedFilesystemScan, complete: bool, matcher: Option<crate::ContributorNameMatcher>,
    ) -> Result<ScanApplyResult, DatabaseError> {
        let mut next = snapshot.clone();
        let discovery = discovery.clone();
        let (next, result) = self
            .run(move |database| {
                let result = database.apply_filesystem_scan_batch(&mut next, &discovery, inspected, complete, matcher)?;
                Ok((next, result))
            })
            .await?;
        *snapshot = next;
        Ok(result)
    }
}
