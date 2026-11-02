//! OPDS supplies metadata and files; ordinary library storage owns permanent copies.
use super::{DocumentBook, LibrarySession, PdfBook, ResolvedBook};
use crate::{BackendError, BookId, DownloadState, LibraryBrowseData};
use book_access::AsyncBookSource;
use book_model::BookFormat;
use library_model::{BlobKind, BrowseLocation, CardKey, DownloadProgress, LibraryBrowseQuery, OpdsCredentials, OpdsPublication, OpdsSource, RelativeBookPath};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    path::PathBuf,
    sync::{Arc, Weak},
};
use tokio::{io::AsyncWriteExt, sync::Mutex};

pub(super) const COVER_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct OpdsLibrary {
    pub source: OpdsSource,
    pub(crate) credentials: RefCell<Option<OpdsCredentials>>,
    scope: OpdsScope,
    temporary: RefCell<HashMap<BookId, (Option<String>, PathBuf)>>,
    downloads: Arc<Mutex<()>>,
    covers: RefCell<HashMap<BookId, Weak<Mutex<()>>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OpdsScope {
    Library,
    Catalog,
}

impl OpdsLibrary {
    pub(super) fn new(source: OpdsSource, credentials: Option<OpdsCredentials>) -> Self {
        Self { source, credentials: RefCell::new(credentials), scope: OpdsScope::Library, temporary: RefCell::new(HashMap::new()), downloads: Arc::new(Mutex::new(())), covers: RefCell::new(HashMap::new()) }
    }

    pub(super) fn make_catalog(&mut self) {
        self.scope = OpdsScope::Catalog;
    }

    pub(crate) fn is_catalog(&self) -> bool {
        self.scope == OpdsScope::Catalog
    }
}

impl LibrarySession {
    /// Fetch and register a page using this library's feed and device credentials.
    pub(crate) async fn browse_opds(&self, query: LibraryBrowseQuery) -> Result<LibraryBrowseData, BackendError> {
        let opds = self.opds.as_ref().ok_or("not an OPDS library")?;
        let BrowseLocation::Opds { source, page } = query.location else { return Err("not an OPDS feed page".into()) };
        if source != opds.source.id {
            return Err("this page belongs to another OPDS library".into());
        }
        let credentials = opds.credentials.borrow().clone();
        let (mut contents, feed, books) = library_opds::browse(&opds.source, credentials.as_ref(), page, &query.search).await?;
        let states = self.register_opds_books(books)?;
        for book in &mut contents.books {
            let id = BookId::for_opds_entry(&book.key().0);
            if let Some((_, progress, downloaded)) = states.iter().find(|(key, _, _)| *key == id) {
                book.progress = *progress;
                book.downloaded = *downloaded;
            }
        }
        Ok(LibraryBrowseData { sections: Vec::new(), contents, path: Vec::new(), ancestors: Vec::new(), language_counts: Vec::new(), format_counts: Vec::new(), feed: Some(feed), page: None })
    }

    /// A listed feed book's file and the name to import it under.
    pub(crate) async fn download_opds_book(&self, entry: CardKey) -> Result<(String, Vec<u8>), BackendError> {
        let opds = self.opds.as_ref().ok_or("not an OPDS library")?;
        let credentials = opds.credentials.borrow().clone();
        library_opds::download(&opds.source, credentials.as_ref(), entry).await
    }

    fn register_opds_books(&self, books: Vec<OpdsPublication>) -> Result<Vec<(BookId, f32, bool)>, BackendError> {
        if !self.is_opds() {
            return Err("not an OPDS library".into());
        }
        if self.db.register_opds_books(&books, !self.opds.as_ref().ok_or("not an OPDS library")?.is_catalog())? {
            self.events.sync.wake();
        }
        self.opds_book_states(books.iter().filter(|book| book.format.is_some() && book.download_url.is_some()).map(OpdsPublication::book_id).collect())
    }

    pub(crate) fn opds_book_states(&self, books: Vec<BookId>) -> Result<Vec<(BookId, f32, bool)>, BackendError> {
        books.into_iter().map(|id| Ok((id, self.db.opds_progress(id)?, matches!(self.persistent_download_state(id)?, DownloadState::Downloaded)))).collect()
    }

    async fn opds_file(&self, id: BookId, permanent: bool) -> Result<(PathBuf, BookFormat), BackendError> {
        let opds = self.opds.as_ref().ok_or("not an OPDS library")?;
        let _download = opds.downloads.lock().await;
        let book = self.db.opds_publication(id)?.ok_or("this book is no longer known")?;
        let format = book.format.ok_or("this book has no supported format")?;
        let info = self.db.local_book_info(&id)?;
        if matches!(self.persistent_download_state(id)?, DownloadState::Downloaded) {
            return Ok((self.assets.native_library_root().join(info.path.ok_or("book path missing")?.as_str().trim_start_matches('/')), info.format));
        }
        let existing =
            opds.temporary.borrow().get(&id).filter(|(url, path)| url == &book.download_url && path.extension().and_then(|extension| extension.to_str()) == Some(format.canonical_extension()) && path.is_file()).map(|(_, path)| path.clone());
        let temporary = if let Some(path) = existing {
            path
        } else {
            let credentials = opds.credentials.borrow().clone();
            let bytes = library_opds::download_publication(&opds.source, credentials.as_ref(), &book).await?;
            // Plain files intentionally remain for OS cleanup, including interrupted downloads.
            let path = std::env::temp_dir().join(format!("bokheim-{}.{}", uuid::Uuid::new_v4(), format.canonical_extension()));
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&path).await?;
            file.write_all(&bytes).await?;
            file.flush().await?;
            opds.temporary.borrow_mut().insert(id, (book.download_url.clone(), path.clone()));
            path
        };
        if !permanent {
            return Ok((temporary, format));
        }
        let relative = RelativeBookPath::parse(&format!("/{id}.{}", format.canonical_extension())).map_err(BackendError::operation)?;
        let reader = crate::executor::run_blocking(move || File::open(temporary)).await.map_err(BackendError::operation)??;
        let copied = self.assets.stage_book(relative.clone(), Box::new(reader)).await.map_err(BackendError::operation)?;
        let (checksum, size, _) = copied.copied_digest().ok_or("download has no local checksum")?;
        let staged = copied.into_staged(checksum.into(), size).with_local_identity(&self.assets, id).await.map_err(BackendError::operation)?;
        let _lease = self.assets.lease_book(&id).await.map_err(BackendError::operation)?;
        let published = staged.publish_snapshot(&self.assets, &relative, &[], false).map_err(BackendError::operation)?;
        self.db.record_opds_download(id, &published, checksum, size, format)?;
        self.emit(super::events::LibraryEvent::DownloadStatusChanged { book_id: id, state: DownloadState::Downloaded });
        self.events.notify_contents_changed();
        Ok((self.assets.native_library_root().join(published.relative_path.trim_start_matches('/')), format))
    }

    pub(crate) async fn download_opds(&self, id: BookId) -> Result<DownloadState, BackendError> {
        self.emit(super::events::LibraryEvent::DownloadStatusChanged { book_id: id, state: DownloadState::Downloading(DownloadProgress::ZERO) });
        if let Err(error) = self.opds_file(id, true).await {
            self.emit(super::events::LibraryEvent::DownloadStatusChanged { book_id: id, state: self.persistent_download_state(id).unwrap_or(DownloadState::NotDownloaded) });
            return Err(error);
        }
        Ok(DownloadState::Downloaded)
    }

    pub(crate) async fn open_opds_book(&self, id: BookId) -> Result<ResolvedBook, BackendError> {
        let (path, format) = self.opds_file(id, false).await?;
        let name = path.to_string_lossy().into_owned();
        let reader = crate::executor::run_blocking(move || File::open(path)).await.map_err(BackendError::operation)??;
        if format == BookFormat::Pdf {
            Ok(ResolvedBook::Pdf(PdfBook { path: name, reader: Box::new(reader), metadata: None }))
        } else {
            Ok(ResolvedBook::Document(DocumentBook::from_async_source(name, format.try_into()?, AsyncBookSource::from_file(reader)?)))
        }
    }

    pub(crate) async fn opds_cover(&self, entry: CardKey, width: u32, card: bool) -> Result<Option<Vec<u8>>, BackendError> {
        let opds = self.opds.as_ref().ok_or("not an OPDS library")?;
        let Some(book) = self.db.opds_publication(BookId::for_opds_entry(&entry.0))? else {
            let credentials = opds.credentials.borrow().clone();
            return Ok(if card { library_opds::card_cover(&opds.source, credentials.as_ref(), entry, width).await } else { library_opds::cover(&opds.source, credentials.as_ref(), entry, width).await });
        };
        let Some(url) = &book.cover_url else { return Ok(None) };
        // Cache the source image by URL, independently of display size or crop.
        let cover_id = BookId::for_opds_entry(blake3::hash(url.as_bytes()).as_bytes());
        let request = {
            let mut requests = opds.covers.borrow_mut();
            requests.retain(|_, request| request.strong_count() > 0);
            let slot = requests.entry(cover_id).or_default();
            slot.upgrade().unwrap_or_else(|| {
                let request = Arc::new(Mutex::new(()));
                *slot = Arc::downgrade(&request);
                request
            })
        };
        // Different covers use the actor's concurrent read slots; requests for
        // the same source wait for its cache write instead of fetching it twice.
        let _request = request.lock().await;
        let cached = self.assets.read_bytes(BlobKind::Thumbnail, &cover_id).await.map_err(BackendError::operation)?;
        let fetched = cached.is_none();
        let bytes = match cached {
            Some(bytes) => bytes,
            None => {
                let credentials = opds.credentials.borrow().clone();
                let Some(bytes) = library_opds::publication_cover_bytes(&opds.source, credentials.as_ref(), &book).await else { return Ok(None) };
                bytes
            }
        };
        // Decoding and resizing must not serialize other covers' network I/O.
        let rendered = crate::executor::run_blocking(move || library_opds::thumbnail_jpeg(&bytes, width, card).map(|cover| (bytes, cover))).await.map_err(BackendError::operation)?;
        let Some((bytes, cover)) = rendered else { return Ok(None) };
        if fetched {
            if let Err(error) = self.assets.write_cached_thumbnail(cover_id, bytes, COVER_CACHE_MAX_BYTES).await {
                log::warn!("OPDS cover cache write failed: {error}");
            }
        }
        Ok(Some(cover))
    }
}
