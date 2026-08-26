use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use app_contract::NavigationCrumb;
use app_contract::account::{AccountStatus, AccountStorageUsage, LibrarySyncState, LibrarySyncStatus, ServerLibraryStorageUsage};
use app_contract::backend::{AppBackend, BackendFuture, BrowserSession, CloudSnapshot, LibraryBackend};
use app_contract::catalog::{CatalogImportRequest, CatalogSourceId, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseRequest, CatalogSubjectBrowseView};
use app_contract::library::{
    BookCardRow, BookFormat, BrowseBookSort, BrowseChipSort, BrowseContents, BrowseRow, Dir, DirectoryImport, EditionIdentityReview, LibraryBrowseQuery, LibraryFileTypeFilter, LibraryFolderDestination, LibraryHomeView, LibrarySummary,
    LibraryTrashView, LibraryUpdate, ReaderContent, SubjectPath,
};
use app_contract::preferences::{BrowsingPreferencePatch, BrowsingPreferences};
use app_contract::reader_state::{ReaderAnnotation, ReaderBookmark};
use app_contract::transfer::{LibraryTransfers, TransferStatus};
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sync_common::{ContentHash, DirId, LibraryId, ROOT_DIR_ID};
use wasm_bindgen::prelude::*;

use bokheim_web_core::web_database as core;

fn js_error(error: JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

fn decode<T: DeserializeOwned>(operation: &str, value: Result<JsValue, JsValue>) -> Result<T, String> {
    serde_wasm_bindgen::from_value(value.map_err(js_error)?).map_err(|error| format!("invalid {operation} response: {error}"))
}

fn encode<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, String> {
    serde_wasm_bindgen::to_value(value).map_err(|error| error.to_string())
}

fn ready<T: 'static>(value: Result<T, String>) -> BackendFuture<T> {
    Box::pin(async move { value })
}

fn unavailable<T: 'static>(operation: &'static str) -> BackendFuture<T> {
    ready(Err(format!("{operation} is not available in the web backend")))
}

fn closed_receiver<T>() -> async_channel::Receiver<T> {
    let (sender, receiver) = async_channel::unbounded();
    drop(sender);
    receiver
}

#[derive(Clone, Deserialize)]
struct WebLibraryEntry {
    library_id: LibraryId,
    library_name: String,
}

impl WebLibraryEntry {
    fn summary(&self) -> LibrarySummary {
        LibrarySummary::new(self.library_id, self.library_name.clone())
    }
}

#[derive(Deserialize)]
struct WebLibraryListState {
    libraries: Vec<WebLibraryEntry>,
    sync_statuses: Vec<LibrarySyncStatus>,
}

#[derive(Deserialize)]
struct WebHierarchyEntry {
    id: String,
    name: String,
    book_count: usize,
    downloaded_book_count: usize,
}

#[derive(Deserialize)]
struct WebDir {
    id: DirId,
    name: String,
    book_count: usize,
    downloaded_book_count: usize,
}

impl From<WebDir> for Dir {
    fn from(value: WebDir) -> Self {
        Dir::new(value.id, value.name, value.book_count, value.downloaded_book_count)
    }
}

#[derive(Deserialize)]
struct WebHierarchyView {
    children: Vec<WebHierarchyEntry>,
    books: Vec<BookCardRow>,
}

#[derive(Default, Deserialize)]
struct WebLibrarySearchResult {
    title_matches: Vec<BookCardRow>,
    author_matches: Vec<BookCardRow>,
}

#[derive(Deserialize)]
struct WebLibraryTransfers {
    library_id: LibraryId,
    library_name: String,
    transfers: Vec<TransferStatus>,
    history: Vec<TransferStatus>,
}

#[derive(Deserialize)]
struct WebCatalogAcquisition {
    file_name: String,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct WebPublicationAsset {
    file_name: String,
    publication_bytes: Vec<u8>,
}

impl From<WebLibraryTransfers> for LibraryTransfers {
    fn from(value: WebLibraryTransfers) -> Self {
        Self { library_id: value.library_id, library_name: value.library_name, scanner_running: false, transfers: value.transfers, history: value.history }
    }
}

fn file_type(query: &LibraryBrowseQuery) -> LibraryFileTypeFilter {
    match query.file_types.as_slice() {
        [only] => *only,
        _ => LibraryFileTypeFilter::All,
    }
}

fn file_type_name(value: LibraryFileTypeFilter) -> &'static str {
    match value {
        LibraryFileTypeFilter::All => "all",
        LibraryFileTypeFilter::Book => "book",
        LibraryFileTypeFilter::Pdf => "pdf",
        LibraryFileTypeFilter::Audiobook => "audiobook",
    }
}

fn sort_contents(contents: &mut BrowseContents, query: &LibraryBrowseQuery) {
    match query.chip_sort {
        BrowseChipSort::Alphabetical => contents.children.sort_by_key(|row| row.name.to_lowercase()),
        BrowseChipSort::BookCount => contents.children.sort_by_key(|row| std::cmp::Reverse(row.book_count)),
    }
    match query.book_sort {
        BrowseBookSort::Alphabetical => contents.books.sort_by_key(|book| (book.title.to_lowercase(), book.author.to_lowercase())),
        BrowseBookSort::RecentlyRead => contents.books.sort_by(|left, right| right.progress.total_cmp(&left.progress).then_with(|| right.added_at.cmp(&left.added_at))),
    }
}

struct BrowserLibraryBackend {
    id: LibraryId,
    paths: Rc<RefCell<HashMap<DirId, Vec<Dir>>>>,
    catalog_subjects: Rc<RefCell<HashMap<String, CatalogSubjectBrowseView>>>,
    update_sender: async_channel::Sender<LibraryUpdate>,
    update_receiver: async_channel::Receiver<LibraryUpdate>,
}

impl BrowserLibraryBackend {
    fn new(id: LibraryId) -> Self {
        let (update_sender, update_receiver) = async_channel::unbounded();
        Self { id, paths: Rc::new(RefCell::new(HashMap::new())), catalog_subjects: Rc::new(RefCell::new(HashMap::new())), update_sender, update_receiver }
    }

    fn changed(&self) {
        let _ = self.update_sender.try_send(LibraryUpdate::Contents);
    }

    async fn browse(&self, query: LibraryBrowseQuery, subject: bool) -> Result<BrowseContents, String> {
        let location = (query.location != library_model::ROOT_SUBJECT_PATH).then(|| query.location.clone());
        let hierarchy: WebHierarchyView = if subject {
            decode("library subjects", core::bokheim_library_subjects(&self.id.to_string(), location.clone(), file_type_name(file_type(&query))))?
        } else {
            decode("library folder", core::bokheim_library_folder(&self.id.to_string(), &query.location, file_type_name(file_type(&query))))?
        };
        let mut children = hierarchy.children.into_iter().map(|row| BrowseRow { id: row.id, name: row.name, path: None, book_count: row.book_count, downloaded_book_count: row.downloaded_book_count }).collect::<Vec<_>>();
        let needle = query.search.trim().to_lowercase();
        if !needle.is_empty() {
            children.retain(|child| child.name.to_lowercase().contains(&needle));
        }
        let books = if needle.is_empty() {
            hierarchy.books
        } else {
            let result: WebLibrarySearchResult = if subject {
                decode(
                    "subject search",
                    core::bokheim_library_search_subject(&self.id.to_string(), location, &query.search, 100, file_type_name(file_type(&query))),
                )?
            } else {
                decode(
                    "folder search",
                    core::bokheim_library_search_folder(&self.id.to_string(), &query.location, &query.search, file_type_name(file_type(&query))),
                )?
            };
            let mut books = result.title_matches;
            for book in result.author_matches {
                if !books.iter().any(|existing| existing.content_hash == book.content_hash) {
                    books.push(book);
                }
            }
            books
        };
        if !subject {
            let paths = self.paths.clone();
            let library_id = self.id;
            let child_ids = children.iter().filter_map(|child| DirId::parse_str(&child.id).ok()).collect::<Vec<_>>();
            for child_id in child_ids {
                let path = decode::<Vec<WebDir>>("folder path", core::bokheim_library_directory_path(&library_id.to_string(), &child_id.to_string()))?.into_iter().map(Into::into).collect();
                paths.borrow_mut().insert(child_id, path);
            }
        }
        let mut contents = BrowseContents { children, books };
        sort_contents(&mut contents, &query);
        Ok(contents)
    }

    async fn import_bytes(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> Result<ContentHash, String> {
        let imported: serde_json::Value = decode(
            "book import",
            core::bokheim_library_import(&self.id.to_string(), &parent_id.to_string(), &file_name, &bytes, &[]),
        )?;
        self.changed();
        let hash = imported.get("content_hash").and_then(Value::as_str).ok_or_else(|| "book import returned no content hash".to_owned())?;
        Ok(ContentHash::new(hash))
    }
}

impl LibraryBackend for BrowserLibraryBackend {
    fn id(&self) -> LibraryId {
        self.id
    }

    fn scanner_running(&self) -> bool {
        false
    }

    fn folder_contents(&self, query: LibraryBrowseQuery) -> BackendFuture<BrowseContents> {
        let this = self.clone_for_future();
        Box::pin(async move { this.browse(query, false).await })
    }

    fn subject_contents(&self, query: LibraryBrowseQuery) -> BackendFuture<BrowseContents> {
        let this = self.clone_for_future();
        Box::pin(async move { this.browse(query, true).await })
    }

    fn directory_path(&self, directory_id: &DirId) -> Result<Vec<Dir>, String> {
        self.paths.borrow().get(directory_id).cloned().ok_or_else(|| "the folder path has not been loaded yet".to_owned())
    }

    fn import_book(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> BackendFuture<ContentHash> {
        let this = self.clone_for_future();
        Box::pin(async move { this.import_bytes(parent_id, file_name, bytes).await })
    }

    fn import_folder(&self, _: DirId, _: PathBuf) -> BackendFuture<Vec<String>> {
        unavailable("Folder path import")
    }

    fn import_directory(&self, parent_id: DirId, source: DirectoryImport) -> BackendFuture<Vec<String>> {
        let this = self.clone_for_future();
        Box::pin(async move {
            let DirectoryImport { name, directories, mut files } = source;
            let root: WebDir = decode("create directory", core::bokheim_library_create_directory(&this.id.to_string(), &parent_id.to_string(), &name))?;
            let mut ids = HashMap::from([(Vec::<String>::new(), root.id)]);
            let mut directories = directories;
            directories.sort_by_key(Vec::len);
            for path in directories {
                if path.is_empty() || ids.contains_key(&path) {
                    continue;
                }
                let parent_path = path[..path.len() - 1].to_vec();
                let parent = *ids.get(&parent_path).ok_or_else(|| format!("missing imported parent folder {}", parent_path.join("/")))?;
                let directory: WebDir = decode(
                    "create directory",
                    core::bokheim_library_create_directory(&this.id.to_string(), &parent.to_string(), path.last().expect("non-empty path")),
                )?;
                ids.insert(path, directory.id);
            }
            files.sort_by(|left, right| left.path.cmp(&right.path));
            let mut failures = Vec::new();
            for file in files {
                let display_path = file.path.join("/");
                let Some(file_name) = file.path.last().cloned() else {
                    continue;
                };
                let directory_path = file.path[..file.path.len() - 1].to_vec();
                let Some(directory_id) = ids.get(&directory_path).copied() else {
                    failures.push(format!("{display_path}: parent folder was not imported"));
                    continue;
                };
                let result = async {
                    let mut reader = (file.open)().await?;
                    let mut bytes = Vec::new();
                    reader.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
                    this.import_bytes(directory_id, file_name, bytes).await.map(|_| ())
                }
                .await;
                if let Err(error) = result {
                    failures.push(format!("{display_path}: {error}"));
                }
            }
            this.changed();
            Ok(failures)
        })
    }

    fn create_directory(&self, parent_id: DirId, name: String) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            let _: WebDir = decode("create directory", core::bokheim_library_create_directory(&id.to_string(), &parent_id.to_string(), &name))?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn folder_destinations(&self) -> BackendFuture<Vec<LibraryFolderDestination>> {
        let id = self.id;
        Box::pin(async move { decode("folder destinations", core::bokheim_library_folder_destinations(&id.to_string())) })
    }

    fn rename_directory(&self, directory_id: DirId, name: String) -> BackendFuture<String> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            let result = core::bokheim_library_rename_directory(&id.to_string(), &directory_id.to_string(), &name).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(result)
        })
    }

    fn move_directory(&self, directory_id: DirId, parent_id: DirId) -> BackendFuture<String> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            let result = core::bokheim_library_move_directory(&id.to_string(), &directory_id.to_string(), &parent_id.to_string()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(result)
        })
    }

    fn copy_book_to_directory(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId) -> BackendFuture<String> {
        self.transfer_book(content_hash, source_id, destination_id, false)
    }

    fn move_book_to_directory(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId) -> BackendFuture<String> {
        self.transfer_book(content_hash, source_id, destination_id, true)
    }

    fn remove_book_from_directory(&self, content_hash: ContentHash, source_id: DirId) -> BackendFuture<bool> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            let result = core::bokheim_library_remove_book_placement(&id.to_string(), content_hash.as_str(), &source_id.to_string()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(result)
        })
    }

    fn load_cover(&self, content_hash: ContentHash) -> BackendFuture<Option<Vec<u8>>> {
        let id = self.id;
        Box::pin(async move {
            let bytes = core::bokheim_library_thumbnail(&id.to_string(), content_hash.as_str()).map_err(js_error)?;
            Ok((!bytes.is_empty()).then_some(bytes))
        })
    }

    fn load_home(&self, limit: i32) -> BackendFuture<LibraryHomeView> {
        let id = self.id;
        Box::pin(async move { decode("library home", core::bokheim_library_home(&id.to_string(), limit)) })
    }

    fn load_prepared_edition_identity_reviews(&self) -> BackendFuture<Vec<EditionIdentityReview>> {
        ready(Ok(Vec::new()))
    }

    fn load_unmapped_subject_texts(&self) -> BackendFuture<Vec<String>> {
        ready(Ok(Vec::new()))
    }

    fn select_edition_identity_candidate(&self, _: ContentHash, _: &str, _: &str) -> Result<bool, String> {
        Err("Metadata review is not available in the web backend".to_owned())
    }

    fn apply_manual_edition_identity(&self, _: ContentHash, _: String) -> BackendFuture<bool> {
        unavailable("Metadata review")
    }

    fn reject_edition_identity_candidates(&self, _: ContentHash) -> Result<(), String> {
        Err("Metadata review is not available in the web backend".to_owned())
    }

    fn retry_failed_edition_identities(&self) -> BackendFuture<usize> {
        unavailable("Metadata review")
    }

    fn trash_book(&self, content_hash: ContentHash) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            core::bokheim_library_trash_book(&id.to_string(), content_hash.as_str()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn trash_folder(&self, folder_id: DirId) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            core::bokheim_library_trash_directory(&id.to_string(), &folder_id.to_string()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn restore_folder(&self, folder_id: DirId, _parent_id: Option<DirId>) -> BackendFuture<String> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            let result = core::bokheim_library_restore_directory(&id.to_string(), &folder_id.to_string()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(result)
        })
    }

    fn load_trash(&self) -> BackendFuture<LibraryTrashView> {
        let id = self.id;
        Box::pin(async move { decode("library trash", core::bokheim_library_trash(&id.to_string())) })
    }

    fn restore_book(&self, content_hash: ContentHash) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            core::bokheim_library_restore_book(&id.to_string(), content_hash.as_str()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn purge_book(&self, content_hash: ContentHash) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            core::bokheim_library_purge_book(&id.to_string(), content_hash.as_str()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn purge_folder(&self, folder_id: DirId) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            core::bokheim_library_purge_directory(&id.to_string(), &folder_id.to_string()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn empty_trash(&self) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            core::bokheim_library_empty_trash(&id.to_string()).map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn cached_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> Result<Option<CatalogSubjectBrowseView>, String> {
        Ok(self.catalog_subjects.borrow().get(&serde_json::to_string(&request).map_err(|error| error.to_string())?).cloned())
    }

    fn refresh_catalog_subjects(&self, request: CatalogSubjectBrowseRequest) -> BackendFuture<CatalogSubjectBrowseView> {
        let id = self.id;
        let cache = self.catalog_subjects.clone();
        Box::pin(async move {
            let key = serde_json::to_string(&request).map_err(|error| error.to_string())?;
            let view: CatalogSubjectBrowseView = decode(
                "catalog subjects",
                core::bokheim_catalog_subjects(
                    id.to_string(),
                    request.locator.clone(),
                    encode(&request.source_ids)?,
                    request.query.clone(),
                    encode(&request.range)?,
                )
                .await,
            )?;
            cache.borrow_mut().insert(key, view.clone());
            Ok(view)
        })
    }

    fn catalog_subject_capabilities(&self, request: CatalogSubjectBrowseRequest) -> BackendFuture<CatalogSubjectBrowseCapabilities> {
        let id = self.id;
        Box::pin(async move {
            let view: CatalogSubjectBrowseView = decode(
                "catalog subjects",
                core::bokheim_catalog_subjects(
                    id.to_string(),
                    request.locator,
                    encode(&request.source_ids)?,
                    request.query,
                    encode(&request.range)?,
                )
                .await,
            )?;
            Ok(view.response.capabilities)
        })
    }

    fn import_catalog_book(&self, request: CatalogImportRequest) -> BackendFuture<()> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            let acquisition: WebCatalogAcquisition = decode("catalog acquisition", core::bokheim_catalog_acquire(encode(&request)?).await)?;
            core::bokheim_library_import_catalog(
                &id.to_string(),
                &ROOT_DIR_ID.to_string(),
                &acquisition.file_name,
                &acquisition.bytes,
                &[],
                encode(&request)?,
            )
            .map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(())
        })
    }

    fn open_reader_content(&self, content_hash: ContentHash) -> BackendFuture<ReaderContent> {
        let id = self.id;
        Box::pin(async move {
            let asset: WebPublicationAsset = decode("publication asset", core::bokheim_library_publication_asset(&id.to_string(), content_hash.as_str()))?;
            let extension = Path::new(&asset.file_name).extension().and_then(|value| value.to_str()).unwrap_or_default();
            let format = BookFormat::from_extension(extension).ok_or_else(|| format!("unsupported publication format: {}", asset.file_name))?;
            Ok(ReaderContent::new(format, Box::new(Cursor::new(asset.publication_bytes))))
        })
    }

    fn reading_position(&self, content_hash: ContentHash) -> BackendFuture<Option<String>> {
        let id = self.id;
        Box::pin(async move { core::bokheim_library_reading_position(&id.to_string(), content_hash.as_str()).map_err(js_error) })
    }

    fn pdf_reading_position(&self, content_hash: ContentHash) -> BackendFuture<Option<(u32, f32)>> {
        let id = self.id;
        Box::pin(async move {
            let value: Option<(u32, Option<f32>)> = decode("PDF reading position", core::bokheim_library_pdf_reading_position(&id.to_string(), content_hash.as_str()))?;
            Ok(value.map(|(page, position)| (page, position.unwrap_or_default())))
        })
    }

    fn update_reading_position(&self, content_hash: ContentHash, position: String, progress: Option<f32>) -> BackendFuture<()> {
        let id = self.id;
        Box::pin(async move { core::bokheim_library_update_reading_position(&id.to_string(), content_hash.as_str(), &position, progress).map_err(js_error) })
    }

    fn annotations(&self, content_hash: ContentHash) -> BackendFuture<Vec<ReaderAnnotation>> {
        let id = self.id;
        Box::pin(async move { decode("annotations", core::bokheim_library_annotations(&id.to_string(), content_hash.as_str())) })
    }

    fn upsert_annotation(&self, annotation: ReaderAnnotation) -> BackendFuture<()> {
        let id = self.id;
        Box::pin(async move { core::bokheim_library_store_annotation(&id.to_string(), encode(&annotation)?).map_err(js_error) })
    }

    fn delete_annotation(&self, annotation_id: String, modified_at: i64) -> BackendFuture<()> {
        let id = self.id;
        Box::pin(async move { core::bokheim_library_delete_annotation(&id.to_string(), &annotation_id, Some(modified_at as f64)).map_err(js_error) })
    }

    fn bookmarks(&self, content_hash: ContentHash) -> BackendFuture<Vec<ReaderBookmark>> {
        let id = self.id;
        Box::pin(async move { decode("bookmarks", core::bokheim_library_bookmarks(&id.to_string(), content_hash.as_str())) })
    }

    fn create_bookmark(&self, bookmark: ReaderBookmark) -> BackendFuture<()> {
        let id = self.id;
        Box::pin(async move { core::bokheim_library_create_bookmark(&id.to_string(), encode(&bookmark)?).map_err(js_error) })
    }

    fn delete_bookmark(&self, bookmark_id: String, deleted_at: i64) -> BackendFuture<()> {
        let id = self.id;
        Box::pin(async move { core::bokheim_library_delete_bookmark(&id.to_string(), &bookmark_id, Some(deleted_at as f64)).map_err(js_error) })
    }
}

impl BrowserLibraryBackend {
    fn clone_for_future(&self) -> Self {
        Self { id: self.id, paths: self.paths.clone(), catalog_subjects: self.catalog_subjects.clone(), update_sender: self.update_sender.clone(), update_receiver: self.update_receiver.clone() }
    }

    fn transfer_book(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId, remove_source: bool) -> BackendFuture<String> {
        let id = self.id;
        let updates = self.update_sender.clone();
        Box::pin(async move {
            let result = core::bokheim_library_transfer_book(
                &id.to_string(),
                content_hash.as_str(),
                &source_id.to_string(),
                &destination_id.to_string(),
                remove_source,
            )
            .map_err(js_error)?;
            let _ = updates.try_send(LibraryUpdate::Contents);
            Ok(result)
        })
    }
}

#[derive(Clone)]
pub(crate) struct BrowserAppBackend {
    libraries: Rc<RefCell<Vec<LibrarySummary>>>,
    backends: Rc<RefCell<HashMap<LibraryId, Rc<BrowserLibraryBackend>>>>,
    library_list_sender: async_channel::Sender<()>,
    library_list_receiver: async_channel::Receiver<()>,
    session: Rc<RefCell<BrowserSession>>,
    preferences: Rc<RefCell<BrowsingPreferences>>,
    account: Rc<RefCell<AccountStatus>>,
    session_saves: async_channel::Sender<BrowserSession>,
    sync_sender: async_channel::Sender<()>,
    sync_receiver: async_channel::Receiver<()>,
}

impl BrowserAppBackend {
    pub(crate) async fn load(session: BrowserSession) -> Result<Self, String> {
        let entries: Vec<WebLibraryEntry> = decode("libraries", core::bokheim_library_list())?;
        let preferences = decode("browsing preferences", core::bokheim_preferences_browsing_get())?;
        let account = decode("account status", core::bokheim_account_status().await)?;
        let summaries = entries.iter().map(WebLibraryEntry::summary).collect::<Vec<_>>();
        let backends = entries.into_iter().map(|entry| (entry.library_id, Rc::new(BrowserLibraryBackend::new(entry.library_id)))).collect();
        let (library_list_sender, library_list_receiver) = async_channel::unbounded();
        let (sync_sender, sync_receiver) = async_channel::unbounded();
        let (session_saves, session_updates) = async_channel::unbounded();
        wasm_bindgen_futures::spawn_local(async move {
            while let Ok(mut session) = session_updates.recv().await {
                while let Ok(newer) = session_updates.try_recv() {
                    session = newer;
                }
                if let Err(error) = encode(&session).and_then(|value| core::bokheim_browser_session_store(value).map_err(js_error)) {
                    log::error!("failed to persist browser session: {error}");
                }
            }
        });
        Ok(Self {
            libraries: Rc::new(RefCell::new(summaries)),
            backends: Rc::new(RefCell::new(backends)),
            library_list_sender,
            library_list_receiver,
            session: Rc::new(RefCell::new(session)),
            preferences: Rc::new(RefCell::new(preferences)),
            account: Rc::new(RefCell::new(account)),
            session_saves,
            sync_sender,
            sync_receiver,
        })
    }

    fn save_session(&self) -> Result<(), String> {
        self.session_saves.try_send(self.session.borrow().clone()).map_err(|_| "browser session persistence task stopped".to_owned())
    }

    fn replace_libraries(&self, entries: Vec<WebLibraryEntry>) {
        let mut backends = self.backends.borrow_mut();
        backends.retain(|id, _| entries.iter().any(|entry| entry.library_id == *id));
        for entry in &entries {
            backends.entry(entry.library_id).or_insert_with(|| Rc::new(BrowserLibraryBackend::new(entry.library_id)));
        }
        *self.libraries.borrow_mut() = entries.iter().map(WebLibraryEntry::summary).collect();
        let _ = self.library_list_sender.try_send(());
    }

    async fn refresh_library_list(&self) -> Result<Vec<LibrarySummary>, String> {
        let entries: Vec<WebLibraryEntry> = decode("libraries", core::bokheim_library_list())?;
        self.replace_libraries(entries);
        Ok(self.libraries.borrow().clone())
    }

    async fn cloud(&self) -> Result<CloudSnapshot, String> {
        let account: AccountStatus = decode("account status", core::bokheim_account_status().await)?;
        *self.account.borrow_mut() = account.clone();
        let transfers = decode::<Vec<WebLibraryTransfers>>("transfers", core::bokheim_library_transfer_statuses())?.into_iter().map(Into::into).collect();
        let local_storage_usage = decode("local storage usage", core::bokheim_library_local_storage_usage())?;
        if !account.signed_in() {
            let sync_statuses = self
                .libraries
                .borrow()
                .iter()
                .map(|library| LibrarySyncStatus {
                    library_id: *library.library_id(),
                    library_name: library.library_name().to_owned(),
                    state: LibrarySyncState::SignInRequired,
                    server_url: None,
                    account_id: None,
                    has_unsynced_changes: false,
                    last_synced_at: None,
                })
                .collect();
            return Ok(CloudSnapshot { account, storage_usage: None, sync_statuses, local_storage_usage, server_storage_usage: None, transfers });
        }
        let state: WebLibraryListState = decode("library reconciliation", core::bokheim_library_reconcile().await)?;
        self.replace_libraries(state.libraries);
        let storage_usage: AccountStorageUsage = decode("account storage usage", core::bokheim_account_storage_usage().await)?;
        #[derive(Deserialize)]
        struct ServerUsage {
            libraries: Vec<ServerLibraryStorageUsage>,
        }
        let server_storage_usage = decode::<ServerUsage>("server library storage usage", core::bokheim_account_library_storage_usage().await)?.libraries;
        let _ = self.sync_sender.try_send(());
        Ok(CloudSnapshot { account, storage_usage: Some(storage_usage), sync_statuses: state.sync_statuses, local_storage_usage, server_storage_usage: Some(server_storage_usage), transfers })
    }
}

impl AppBackend for BrowserAppBackend {
    fn browser_session(&self) -> Result<BrowserSession, String> {
        Ok(self.session.borrow().clone())
    }

    fn save_active_library_id(&self, library_id: Option<LibraryId>) -> Result<(), String> {
        self.session.borrow_mut().active_library_id = library_id;
        self.save_session()
    }

    fn folder_navigation_path(&self, library_id: LibraryId) -> Vec<NavigationCrumb<DirId>> {
        self.session.borrow().place(&library_id).folder_path
    }

    fn save_folder_navigation_path(&self, library_id: LibraryId, path: Vec<NavigationCrumb<DirId>>) -> Result<(), String> {
        self.session.borrow_mut().places.entry(library_id).or_default().folder_path = path;
        self.save_session()
    }

    fn subject_navigation_path(&self, library_id: LibraryId) -> Vec<NavigationCrumb<SubjectPath>> {
        self.session.borrow().place(&library_id).subject_path
    }

    fn save_subject_navigation_path(&self, library_id: LibraryId, path: Vec<NavigationCrumb<SubjectPath>>) -> Result<(), String> {
        self.session.borrow_mut().places.entry(library_id).or_default().subject_path = path;
        self.save_session()
    }

    fn retain_browser_session_libraries(&self, library_ids: &[LibraryId]) -> Result<(), String> {
        self.session.borrow_mut().retain_places(|library_id| library_ids.contains(library_id));
        self.save_session()
    }

    fn libraries(&self) -> Result<Vec<LibrarySummary>, String> {
        Ok(self.libraries.borrow().clone())
    }

    fn ensure_default_library(&self) -> Result<LibrarySummary, String> {
        self.libraries.borrow().first().cloned().ok_or_else(|| "the web backend returned no libraries".to_owned())
    }

    fn library(&self, library_id: &LibraryId) -> Result<Rc<dyn LibraryBackend>, String> {
        self.backends.borrow().get(library_id).cloned().map(|backend| backend as Rc<dyn LibraryBackend>).ok_or_else(|| format!("unknown web library {library_id}"))
    }

    fn create_libraries_from_paths(&self, _: Vec<PathBuf>) -> BackendFuture<(Vec<LibraryId>, Vec<String>)> {
        unavailable("Library path creation")
    }

    fn create_library(&self, name: String) -> BackendFuture<LibraryId> {
        let this = self.clone();
        Box::pin(async move {
            let entry: WebLibraryEntry = decode("create library", core::bokheim_library_create(&name))?;
            let id = entry.library_id;
            this.refresh_library_list().await?;
            Ok(id)
        })
    }

    fn import_library_directory(&self, library_id: LibraryId, directory: DirectoryImport) -> BackendFuture<Vec<String>> {
        match self.backends.borrow().get(&library_id).cloned() {
            Some(backend) => backend.import_directory(ROOT_DIR_ID, directory),
            None => ready(Err(format!("unknown web library {library_id}"))),
        }
    }

    fn delete_library(&self, library_id: LibraryId) -> BackendFuture<()> {
        let this = self.clone();
        Box::pin(async move {
            core::bokheim_library_remove(&library_id.to_string()).map_err(js_error)?;
            this.refresh_library_list().await?;
            Ok(())
        })
    }

    fn reconcile_libraries(&self) -> BackendFuture<Vec<LibrarySummary>> {
        let this = self.clone();
        Box::pin(async move { this.refresh_library_list().await })
    }

    fn enable_library_scanning(&self) -> Result<(), String> {
        Ok(())
    }

    fn request_library_scan(&self, _: &LibraryId) -> Result<(), String> {
        Ok(())
    }

    fn library_list_changes(&self) -> async_channel::Receiver<()> {
        self.library_list_receiver.clone()
    }

    fn library_updates(&self, library_id: LibraryId) -> async_channel::Receiver<LibraryUpdate> {
        self.backends.borrow().get(&library_id).map(|backend| backend.update_receiver.clone()).unwrap_or_else(closed_receiver)
    }

    fn browsing_preferences(&self) -> Result<BrowsingPreferences, String> {
        Ok(self.preferences.borrow().clone())
    }

    fn update_browsing_preference(&self, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, String> {
        patch.clone().apply(&mut self.preferences.borrow_mut()).map_err(|error| error.to_string())?;
        let preferences = self.preferences.borrow().clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(error) = encode(&patch).and_then(|patch| decode::<BrowsingPreferences>("browsing preference update", core::bokheim_preferences_browsing_update(patch))) {
                log::error!("failed to persist web browsing preferences: {error}");
            }
        });
        Ok(preferences)
    }

    fn account_status(&self) -> Result<AccountStatus, String> {
        Ok(self.account.borrow().clone())
    }

    fn sync_changes(&self) -> async_channel::Receiver<()> {
        self.sync_receiver.clone()
    }

    fn transfer_changes(&self) -> async_channel::Receiver<()> {
        self.sync_receiver.clone()
    }

    fn cloud_snapshot(&self) -> BackendFuture<CloudSnapshot> {
        let this = self.clone();
        Box::pin(async move { this.cloud().await })
    }

    fn sync_all_libraries(&self) -> BackendFuture<CloudSnapshot> {
        let this = self.clone();
        Box::pin(async move {
            core::bokheim_library_sync_all().await.map_err(js_error)?;
            this.cloud().await
        })
    }

    fn login(&self, email: String, password: String) -> BackendFuture<CloudSnapshot> {
        let this = self.clone();
        Box::pin(async move {
            let account: AccountStatus = decode("account login", core::bokheim_account_login(email, password).await)?;
            *this.account.borrow_mut() = account;
            this.cloud().await
        })
    }

    fn logout(&self) -> BackendFuture<CloudSnapshot> {
        let this = self.clone();
        Box::pin(async move {
            let account: AccountStatus = decode("account logout", core::bokheim_account_logout().await)?;
            *this.account.borrow_mut() = account;
            this.refresh_library_list().await?;
            this.cloud().await
        })
    }

    fn request_registration(&self, email: String, password: String) -> BackendFuture<()> {
        Box::pin(async move { core::bokheim_account_register(email, password).await.map_err(js_error) })
    }

    fn resend_verification(&self, email: String) -> BackendFuture<()> {
        Box::pin(async move { core::bokheim_account_resend_verification(email).await.map_err(js_error) })
    }

    fn verify_email(&self, email: String, pin: String) -> BackendFuture<CloudSnapshot> {
        let this = self.clone();
        Box::pin(async move {
            let account: AccountStatus = decode("email verification", core::bokheim_account_verify_email(email, pin).await)?;
            *this.account.borrow_mut() = account;
            this.cloud().await
        })
    }

    fn request_password_reset(&self, email: String) -> BackendFuture<()> {
        Box::pin(async move { core::bokheim_account_request_password_reset(email).await.map_err(js_error) })
    }

    fn reset_password(&self, token: String, password: String) -> BackendFuture<()> {
        Box::pin(async move { core::bokheim_account_reset_password(token, password).await.map_err(js_error) })
    }

    fn catalog_cover(&self, source_id: CatalogSourceId, image_url: String) -> BackendFuture<Vec<u8>> {
        Box::pin(async move { core::bokheim_catalog_cover(source_id.as_str().to_owned(), image_url).await.map_err(js_error) })
    }
}
