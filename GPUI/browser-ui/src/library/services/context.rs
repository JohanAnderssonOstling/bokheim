use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use epub_svelte_lib::AuthorId;
use epub_svelte_lib::FrontendContract;
use epub_svelte_lib::app::CatalogAuthorDiscoveryView;
use epub_svelte_lib::app::executor::BackendTask;
use epub_svelte_lib::catalog::CatalogImportRequest;
use epub_svelte_lib::library::LibraryBackend;
use epub_svelte_lib::{BookId, BookLocator};
use gpui::{App, Context, FileDialogFilter, PathPromptOptions, Window};

use crate::catalog::CatalogCardServices;
use crate::services::AppServices;

pub(crate) use epub_svelte_lib::library::{LibraryAuthorsView as AuthorBrowseRows, LibraryFolderView as FolderBrowseRows, LibraryHomeView as HomeBrowseRows};

/// Capabilities scoped to one selected library.
///
/// This is a plain value, not a GPUI entity: replacing `LibraryRoot` replaces
/// every clone. Pages use its typed operations instead of assembling backend
/// calls from several service wrappers.
#[derive(Clone)]
pub(crate) struct LibraryContext {
    app: AppServices,
    pub(crate) library: LibraryBackend,
}

impl LibraryContext {
    pub(crate) fn new(app: AppServices, library: LibraryBackend) -> Self {
        Self { app, library }
    }

    pub(crate) fn contracts(&self) -> &Arc<FrontendContract> {
        &self.app.contracts
    }

    pub(crate) fn catalog_cards(&self) -> CatalogCardServices {
        let context = self.clone();
        let import = Rc::new(move |request| context.import_catalog_book(request));
        CatalogCardServices::new(&self.app, import)
    }

    pub(crate) fn with_window<R>(&self, cx: &mut App, body: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
        self.app.with_window(cx, body)
    }

    pub(crate) fn open_reader(&self, book_id: BookId, title: String, cx: &mut App) {
        let Ok(locator) = BookLocator::parse(self.library.id().as_str(), book_id) else { return };
        (self.app.open_reader)(locator, title, cx);
    }

    pub(crate) fn load_cover(&self, book_id: BookId) -> impl Future<Output = Result<Option<Vec<u8>>, String>> + 'static {
        let operation = self.library.load_thumbnail(book_id);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn import_book(&self, folder_id: String, path: PathBuf, file_name: String) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.import_book_task(folder_id, path, file_name);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn choose_and_import_book<P>(&self, folder_id: String, cx: &mut Context<P>, finished: impl FnOnce(&mut P, Result<(), String>, &mut Context<P>) + 'static)
    where
        P: 'static,
    {
        let prompt = cx.prompt_for_paths_with_filters(
            PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add book".into()) },
            vec![FileDialogFilter { name: "Books and web publications".to_owned(), extensions: self.app.contracts.browsing_content.import_extensions.clone() }],
        );
        let context = self.clone();
        cx.spawn(async move |page, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else { return };
            let result = match paths.into_iter().next() {
                Some(path) => match path.file_name().and_then(|name| name.to_str()).map(str::to_owned) {
                    Some(file_name) => context.import_book(folder_id, path, file_name).await,
                    None => Err("Selected book has no valid file name".to_owned()),
                },
                None => return,
            };
            let _ = page.update(cx, |page, cx| finished(page, result, cx));
        })
        .detach();
    }

    pub(crate) fn create_folder(&self, parent_id: String, name: String) -> impl Future<Output = Result<(), String>> + 'static {
        let operation = self.library.create_directory_task(parent_id, name);
        async move { operation.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) }
    }

    pub(crate) fn discover_author_books(&self, author_id: AuthorId) -> BackendTask<Result<CatalogAuthorDiscoveryView, String>> {
        self.library.spawn(move |library| async move { library.discover_author_books(author_id).await.map_err(|error| error.to_string()) })
    }

    pub(crate) fn import_catalog_book(&self, request: CatalogImportRequest) -> Result<BackendTask<Result<(), String>>, String> {
        Ok(self.library.spawn(move |library| async move { library.import_catalog_book("__root__", &request, None).await.map(|_| ()).map_err(|error| error.to_string()) }))
    }
}
