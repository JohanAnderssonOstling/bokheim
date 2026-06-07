use crate::db;
use crate::home::{get_app_data_thumbnails_dir, get_library_path_by_id, get_thumbnail, set_app_data_dir};
use crate::scanner::{LibraryEvent, LibraryScanner};
use crate::sync::SyncManager;
use crate::BookID;
use crossbeam_channel::{unbounded, Receiver, Sender};
use epub_provider::{EpubProvider, ResourceProvider};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::io::Reader as ImageReader;
use image::{ColorType, ImageEncoder, ImageFormat};
use include_sqlite_sql::{impl_sql, include_sql};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::Serialize;
use std::env;
use std::fs;
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Serialize)]
pub struct Book {
    pub id: BookID,
    pub title: String,
    pub author: String,
    pub cover: Option<Vec<u8>>,
    pub read_progress: f32,
    pub last_read: i64,
    pub added_at: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Dir {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize)]
pub struct DirContent {
    pub books: Vec<Book>,
    pub dirs: Vec<Dir>,
}

#[derive(Clone, Serialize)]
pub struct AuthorSummary {
    pub name: String,
    pub book_count: usize,
}

#[derive(Clone, Serialize)]
pub struct AuthorFolderBooks {
    pub folder_id: String,
    pub folder_path: String,
    pub books: Vec<Book>,
}

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

impl Book {
    pub fn new(id: BookID, title: String, author: String) -> Self {
        Self { id, title, author, cover: None, read_progress: 0.0, last_read: 0, added_at: 0 }
    }
}
include_sql!("src/database/create_library.sql");
include_sql!("src/database/library.sql");

pub struct Library {
    id: String,
    db_pool: Pool<SqliteConnectionManager>,
    sync_manager: Arc<SyncManager>,
    event_tx: Sender<LibraryEvent>,
    scanner: LibraryScanner,
}

pub fn sync_server_url() -> String {
    env::var("SYNC_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

impl Library {
    pub fn new(library_path: &str, library_id: &str) -> (Self, Receiver<LibraryEvent>) {
        Self::with_options(library_path, library_id, None, None)
    }

    pub fn new_with_user(library_path: &str, library_id: &str, user_id: &str) -> (Self, Receiver<LibraryEvent>) {
        Self::with_options(library_path, library_id, Some(user_id.to_string()), None)
    }

    pub fn new_with_user_and_server(library_path: &str, library_id: &str, user_id: &str, server_url: &str) -> (Self, Receiver<LibraryEvent>) {
        Self::with_options(library_path, library_id, Some(user_id.to_string()), Some(server_url.to_string()))
    }

    fn with_options(library_path: &str, library_id: &str, forced_user_id: Option<String>, forced_server_url: Option<String>) -> (Self, Receiver<LibraryEvent>) {
        println!("Creating {library_path}");
        let _ = set_app_data_dir(Path::new(library_path).join(".biblos"));
        let res = fs::create_dir_all(format!("{library_path}/.biblos"));
        if let Err(err) = res {
            log::error!("biblos Error: {}", err);
        }

        fs::create_dir_all(format!("{library_path}/.biblos/thumbnails")).unwrap();
        let db_path = format!("{library_path}/.biblos/library.db");

        let manager = SqliteConnectionManager::file(&db_path).with_init(|conn| {
            // Enable WAL mode for better concurrency
            conn.pragma_update(None, "journal_mode", "WAL")?;
            // Set busy timeout to 10 seconds (increased for large scans)
            conn.pragma_update(None, "busy_timeout", "10000")?;
            Ok(())
        });

        // Configure pool with larger size to avoid connection starvation
        let db_pool = Pool::builder()
            .max_size(20) // Increased from default 10
            .connection_timeout(std::time::Duration::from_secs(30))
            .build(manager)
            .unwrap();

        let client_id = {
            let conn = db_pool.get().unwrap();
            db::init_db(&conn)
        }; // Connection explicitly dropped here before scanner starts

        // Create event channel
        let (event_tx, event_rx) = unbounded();
        dbg!("Before scan");
        // Create scanner with event channel
        let scanner = LibraryScanner::new(library_path.to_string(), db_pool.clone(), event_tx.clone());

        let library = Self {
            id: library_id.to_string(),
            db_pool: db_pool.clone(),
            sync_manager: Arc::new(SyncManager::new(db_pool.clone(), client_id, forced_server_url.unwrap_or_else(sync_server_url), library_id.to_string(), forced_user_id, library_path.to_string(), Some(event_tx.clone())).unwrap()),
            event_tx: event_tx.clone(),
            scanner,
        };

        (library, event_rx)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn get_dir_contents(&self, dir_id: &str) -> DirContent {
        let conn = self.db_pool.get().unwrap();
        let (dirs, mut books) = db::get_dir_contents(&conn, dir_id);
        for book in &mut books {
            book.cover = get_thumbnail(book.id);
        }
        DirContent { books, dirs }
    }

    pub fn get_dir_contents_raw(&self, dir_id: &str) -> DirContent {
        let conn = self.db_pool.get().unwrap();
        let (dirs, books) = db::get_dir_contents(&conn, dir_id);
        DirContent { books, dirs }
    }

    pub fn get_recently_read(&self, limit: i32) -> Vec<Book> {
        let conn = self.db_pool.get().unwrap();
        let mut books = db::get_recently_read(&conn, limit);
        for book in &mut books {
            book.cover = get_thumbnail(book.id);
        }
        books
    }

    pub fn get_recently_added(&self, limit: i32) -> Vec<Book> {
        let conn = self.db_pool.get().unwrap();
        let mut books = db::get_recently_added(&conn, limit);
        for book in &mut books {
            book.cover = get_thumbnail(book.id);
        }
        books
    }

    pub fn get_authors(&self) -> Vec<AuthorSummary> {
        let conn = self.db_pool.get().unwrap();
        db::get_authors(&conn).into_iter().map(|(name, book_count)| AuthorSummary { name, book_count }).collect()
    }

    pub fn get_author_folder_books(&self, author_name: &str) -> Vec<AuthorFolderBooks> {
        let conn = self.db_pool.get().unwrap();
        let mut groups = db::get_author_books_grouped(&conn, author_name);
        for (_, _, books) in &mut groups {
            for book in books {
                book.cover = get_thumbnail(book.id);
            }
        }
        groups.into_iter().map(|(folder_id, folder_path, books)| AuthorFolderBooks { folder_id, folder_path, books }).collect()
    }

    fn resolve_book_path(&self, book_id: BookID) -> Option<PathBuf> {
        let conn = self.db_pool.get().unwrap();
        let relative_path = db::get_book_file_path_as_pathbuf(&conn, book_id)?;
        let library_path = get_library_path_by_id(&self.id)?;
        Some(library_path.join(relative_path))
    }

    pub fn open_book_reader(&self, book_id: &str) -> Result<Box<dyn ReadSeek + Send>, Box<dyn std::error::Error>> {
        let parsed: BookID = book_id.parse()?;
        let path = self.resolve_book_path(parsed).ok_or_else(|| format!("book not found: {book_id}"))?;
        let file = File::open(path)?;
        Ok(Box::new(file))
    }

    pub fn read_book_bytes(&self, book_id: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut reader = self.open_book_reader(book_id)?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub fn get_book_reading_position(&self, book_id: &str) -> Option<String> {
        let conn = self.db_pool.get().unwrap();
        db::get_book_reading_position(&conn, book_id)
    }

    pub fn is_book_downloaded(&self, book_id: &str) -> bool {
        let conn = self.db_pool.get().unwrap();
        db::is_book_downloaded(&conn, book_id)
    }

    pub fn update_reading_position(&self, book_id: BookID, cfi: &str) {
        let conn = self.db_pool.get().unwrap();
        db::update_reading_position(&conn, book_id, cfi);
    }

    pub fn update_last_read(&self, book_id: &str) {
        let conn = self.db_pool.get().unwrap();
        db::update_last_read(&conn, book_id);
    }

    pub fn mark_book_opened(&self, book_id: &str) {
        let conn = self.db_pool.get().unwrap();
        db::update_last_read(&conn, book_id);
        let _ = self.event_tx.send(LibraryEvent::BookOpened { book_id: book_id.to_string() });
    }

    pub async fn sync_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_manager.sync_once().await
    }

    pub fn request_book_download(&self, book_id: &str) {
        self.sync_manager.request_download(book_id);
    }

    pub fn user_id(&self) -> &str {
        self.sync_manager.user_id()
    }

    pub fn server_url(&self) -> String {
        sync_server_url()
    }

    pub fn is_signed_in(&self) -> bool {
        self.sync_manager.is_signed_in()
    }

    pub fn current_session(&self) -> Option<crate::auth::Session> {
        self.sync_manager.current_session()
    }

    pub fn set_session(&self, session: crate::auth::Session) {
        self.sync_manager.set_session(session);
    }

    pub fn clear_session(&self) {
        self.sync_manager.clear_session();
    }

    pub fn login_blocking(&self, username: &str, password: &str) -> Result<crate::auth::Session, crate::auth::AuthError> {
        let url = self.server_url();
        let user_id = self.user_id().to_string();
        let username = username.to_string();
        let password = password.to_string();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(crate::auth::AuthError::Io)?;
        let session = rt.block_on(async move {
            crate::auth::login(&url, &username, &password, Some(&user_id)).await
        })?;
        self.sync_manager.set_session(session.clone());
        Ok(session)
    }

    pub fn signup_blocking(&self, invite: &str, username: &str, password: &str) -> Result<crate::auth::Session, crate::auth::AuthError> {
        let url = self.server_url();
        let user_id = self.user_id().to_string();
        let invite = invite.to_string();
        let username = username.to_string();
        let password = password.to_string();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(crate::auth::AuthError::Io)?;
        let session = rt.block_on(async move {
            crate::auth::signup(&url, &invite, &username, &password, Some(&user_id)).await
        })?;
        self.sync_manager.set_session(session.clone());
        Ok(session)
    }

    pub fn logout_blocking(&self) {
        if let Some(s) = self.sync_manager.current_session() {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok();
            if let Some(rt) = rt {
                let _ = rt.block_on(async move { crate::auth::logout(&s).await });
            }
        }
        self.sync_manager.clear_session();
    }

    pub fn delete_directory(&self, path: &str) {
        let conn = self.db_pool.get().unwrap();
        db::delete_dir(&conn, path);
    }
}
fn get_cover_path(epub: &EpubProvider) -> Option<String> {
    epub.cover_image_path().ok().flatten()
}

pub fn generate_file_hash(path: &Path) -> u64 {
    const CHUNK: usize = 16 * 1024;

    let mut file = File::open(&path).unwrap();
    let file_size = file.metadata().unwrap().len();

    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut buf = vec![0u8; CHUNK];

    let n = file.read(&mut buf).unwrap();
    hasher.update(&buf[..n]);

    if file_size > CHUNK as u64 {
        file.seek(SeekFrom::End(-(CHUNK as i64))).unwrap();
        let n = file.read(&mut buf).unwrap();
        hasher.update(&buf[..n]);
    }
    hasher.update(&file_size.to_le_bytes());
    hasher.digest()
}

pub fn generate_book_id_blake3(path: &Path, buf: &mut [u8]) -> u64 {
    let mut file = File::open(path).unwrap();
    let mut hasher = blake3::Hasher::new();

    loop {
        let n = file.read(buf).unwrap();
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let digest = hasher.finalize();
    let mut id_bytes = [0u8; 8];
    id_bytes.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(id_bytes)
}

pub fn generate_thumbnail(lib_path: &str, epub_path: &String, id: &String) -> Result<(), Box<dyn std::error::Error>> {
    let cleaned_id = id.replace("/", "|");
    let thumbnail_path = format!("{lib_path}/.biblos/thumbnails/{cleaned_id}.png");
    if fs::metadata(&thumbnail_path).is_ok() {
        return Ok(());
    }

    let epub = match File::open(epub_path).and_then(EpubProvider::try_from_reader) {
        Ok(epub) => epub,
        Err(err) => {
            log::warn!("Skipping thumbnail generation for book {}: failed to open EPUB {} ({})", id, epub_path, err);
            return Ok(());
        }
    };
    let image_path = match epub.cover_image_path() {
        Ok(Some(path)) => path,
        Ok(None) => {
            log::warn!("Skipping thumbnail generation for book {}: no cover path in EPUB {}", id, epub_path);
            return Ok(());
        }
        Err(err) => {
            log::warn!("Skipping thumbnail generation for book {}: failed to read cover path in EPUB {} ({})", id, epub_path, err);
            return Ok(());
        }
    };

    let image_type = get_image_type(&image_path).ok_or_else(|| format!("Unsupported cover image format for book {}: cover path {}", id, image_path))?;
    let image_bytes = epub.read_bytes(&image_path).map_err(|err| format!("Failed to read cover bytes for book {} from {}: {}", id, image_path, err))?;
    let decoded = ImageReader::with_format(Cursor::new(&image_bytes), image_type).decode().map_err(|err| format!("Failed to decode cover image for book {} from {}: {}", id, image_path, err))?.to_rgb8();

    let (src_width, src_height) = decoded.dimensions();
    let dst_width = 300;
    let scale = dst_width as f32 / src_width as f32;
    let dst_height = (src_height as f32 * scale).round() as u32;

    let src_image = Image::from_vec_u8(src_width, src_height, decoded.into_raw(), PixelType::U8x3)?;
    let mut dst_image = Image::new(dst_width, dst_height, PixelType::U8x3);

    let mut resizer = Resizer::new();
    resizer.resize(&src_image, &mut dst_image, None)?;

    let mut result_buf = BufWriter::new(Vec::new());
    JpegEncoder::new(&mut result_buf).write_image(dst_image.buffer(), dst_width, dst_height, ColorType::Rgb8).unwrap();

    fs::write(thumbnail_path, &result_buf.into_inner().unwrap())?;

    Ok(())
}

pub fn get_image_type(image_path: &str) -> Option<ImageFormat> {
    let file_extension = image_path.rsplit('.').next()?.to_ascii_lowercase();
    match file_extension.as_str() {
        "jpeg" => Some(ImageFormat::Jpeg),
        "jpg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "gif" => Some(ImageFormat::Gif),
        _ => None,
    }
}

pub fn generate_thumbnail_to_app_data(epub_path: &str, book_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let thumbnails_dir = get_app_data_thumbnails_dir();
    let thumbnail_path = thumbnails_dir.join(format!("{}.jpg", book_id));

    // Check if thumbnail already exists using book hash ID
    if thumbnail_path.exists() {
        return Ok(());
    }

    let epub = match File::open(epub_path).and_then(EpubProvider::try_from_reader) {
        Ok(epub) => epub,
        Err(err) => {
            log::warn!("Skipping app-data thumbnail generation for book {}: failed to open EPUB {} ({})", book_id, epub_path, err);
            return Ok(());
        }
    };
    let image_path = match epub.cover_image_path() {
        Ok(Some(path)) => path,
        Ok(None) => {
            log::warn!("No cover image path in EPUB while generating thumbnail for book {} ({})", book_id, epub_path);
            return Ok(());
        }
        Err(err) => {
            log::warn!("Failed to inspect EPUB cover path for book {} ({}): {}", book_id, epub_path, err);
            return Ok(());
        }
    };

    let image_type = match get_image_type(&image_path) {
        Some(image_type) => image_type,
        None => {
            log::warn!("Unsupported EPUB cover image format for book {}: {}", book_id, image_path);
            return Ok(());
        }
    };
    let image_bytes = match epub.read_bytes(&image_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!("Failed to read EPUB cover bytes for book {} from {}: {}", book_id, image_path, err);
            return Ok(());
        }
    };
    let decoded = match ImageReader::with_format(Cursor::new(&image_bytes), image_type).decode() {
        Ok(decoded) => decoded.to_rgb8(),
        Err(err) => {
            log::warn!("Failed to decode EPUB cover image for book {} from {}: {}", book_id, image_path, err);
            return Ok(());
        }
    };

    let (src_width, src_height) = decoded.dimensions();
    let dst_width = 300;
    let scale = dst_width as f32 / src_width as f32;
    let dst_height = (src_height as f32 * scale).round() as u32;

    let src_image = Image::from_vec_u8(src_width, src_height, decoded.into_raw(), PixelType::U8x3)?;
    let mut dst_image = Image::new(dst_width, dst_height, PixelType::U8x3);

    let mut resizer = Resizer::new();
    resizer.resize(&src_image, &mut dst_image, None)?;

    let mut result_buf = BufWriter::new(Vec::new());
    JpegEncoder::new(&mut result_buf).write_image(dst_image.buffer(), dst_width, dst_height, ColorType::Rgb8).unwrap();

    fs::write(&thumbnail_path, &result_buf.into_inner().unwrap())?;

    Ok(())
}
