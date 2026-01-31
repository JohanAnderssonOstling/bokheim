use crate::db;
use crate::scanner::{LibraryEvent, LibraryScanner};
use crate::sync::SyncManager;
use crossbeam_channel::{unbounded, Receiver};
use epub_provider::{EpubProvider, ResourceProvider};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, ResizeAlg, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::io::Reader as ImageReader;
use image::{ColorType, ImageEncoder, ImageFormat};
use include_sqlite_sql::{impl_sql, include_sql};
use r2d2::{Pool};
use r2d2_sqlite::SqliteConnectionManager;
use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use xxhash_rust::xxh3::Xxh3;
use crate::BookID;
use crate::home::{get_thumbnail, resolve_thumbnail};

#[derive(Serialize)]
pub struct Book {
    pub id: BookID,
    pub title: String,
    pub cover: Option<Vec<u8>>
}

pub struct Dir {
    pub id: String,
    pub name: String,
}

pub struct DirContent {
    pub books: Vec<Book>,
    pub dirs: Vec<Dir>,
}

impl Book {
    pub fn new(id: BookID, title: String) -> Self {
        Self { id, title, cover: None }
    }
}
include_sql!("src/database/create_library.sql");
include_sql!("src/database/library.sql");

pub struct Library {
    id: String,
    db_pool: Pool<SqliteConnectionManager>,
    sync_manager: Arc<SyncManager>,
    library_path: String,
    scanner: LibraryScanner,
}

fn sync_server_url() -> String {
    env::var("SYNC_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string())
}

fn auto_sync_enabled() -> bool {
    match env::var("BIBLOS_DISABLE_AUTO_SYNC") {
        Ok(value) => {
            let value = value.to_ascii_lowercase();

            !matches!(value.as_str(), "1" | "true" | "yes")
        }
        Err(_) => true,
    }
}

impl Library {
    pub fn new(library_path: &str, library_id: &str) -> (Self, Receiver<LibraryEvent>) {
        Self::with_options(library_path, library_id, None, None)
    }

    pub fn new_with_user(
        library_path: &str,
        library_id: &str,
        user_id: &str,
    ) -> (Self, Receiver<LibraryEvent>) {
        Self::with_options(library_path, library_id, Some(user_id.to_string()), None)
    }

    pub fn new_with_user_and_server(
        library_path: &str,
        library_id: &str,
        user_id: &str,
        server_url: &str,
    ) -> (Self, Receiver<LibraryEvent>) {
        Self::with_options(
            library_path,
            library_id,
            Some(user_id.to_string()),
            Some(server_url.to_string()),
        )
    }

    fn with_options(
        library_path: &str,
        library_id: &str,
        forced_user_id: Option<String>,
        forced_server_url: Option<String>,
    ) -> (Self, Receiver<LibraryEvent>) {
        log::info!("biblos: Before creating {library_path}");
        println!("Creating {library_path}");
        let res = fs::create_dir_all(format!("{library_path}/.biblos"));
        if let Err(err) = res {
            log::info!("biblos Error: {}", err);
        }

        fs::create_dir_all(format!("{library_path}/.biblos/thumbnails")).unwrap();
        let db_path = format!("{library_path}/.biblos/library.db");

        let manager = SqliteConnectionManager::file(&db_path);
        let db_pool = Pool::new(manager).unwrap();
        let conn = db_pool.get().unwrap();
        let client_id = db::init_db(&conn);

        // Create event channel
        let (event_tx, event_rx) = unbounded();
        dbg!("Before scan");
        // Create scanner with event channel
        let scanner = LibraryScanner::new(library_path.to_string(), db_pool.clone(), event_tx.clone());

        let library = Self {
            id: library_id.to_string(),
            db_pool: db_pool.clone(),
            sync_manager: Arc::new(
                SyncManager::new(
                    db_pool.clone(),
                    client_id,
                    forced_server_url.unwrap_or_else(sync_server_url),
                    library_id.to_string(),
                    forced_user_id,
                    library_path.to_string(),
                    Some(event_tx.clone()),
                )
                .unwrap(),
            ),
            library_path: library_path.to_string(),
            scanner,
        };

        if auto_sync_enabled() {
            let sync_manager = Arc::clone(&library.sync_manager);
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for sync");
                runtime.block_on(async move {
                    if let Err(err) = sync_manager.sync_once().await {
                        log::error!("Initial sync failed: {}", err);
                    }
                    sync_manager.start_sync_loop().await;
                });
            });
        }

        (library, event_rx)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn get_dir_contents(&self, dir_id: &str) -> DirContent {
        let conn = self.db_pool.get().unwrap();
        let (dirs, mut books) = db::get_dir_contents(&conn, dir_id);
        for mut book in &mut books {
            book.cover = get_thumbnail(book.id);
        }
        DirContent {
            books, dirs
        }
    }

    pub fn get_recently_read(&self, limit: i32) -> Vec<Book> {
        let conn = self.db_pool.get().unwrap();
        db::get_recently_read(&conn, limit)
    }

    pub fn get_recently_added(&self, limit: i32) -> Vec<Book> {
        let conn = self.db_pool.get().unwrap();
        db::get_recently_added(&conn, limit)
    }

    pub fn get_book_file_path(&self, book_id: &str) -> Option<String> {
        let conn = self.db_pool.get().unwrap();
        db::get_book_file_path(&conn, book_id)
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
        db::update_reading_position(&conn, book_id, cfi)
    }

    pub fn update_last_read(&self, book_id: &str) {
        let conn = self.db_pool.get().unwrap();
        db::update_last_read(&conn, book_id)
    }

    pub async fn sync_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_manager.sync_once().await
    }

    pub fn request_book_download(&self, book_id: &str) {
        self.sync_manager.request_download(book_id);
    }

    pub fn library_state(&self) -> (HashSet<String>, HashSet<String>, HashSet<(String, String)>) {
        let conn = self.db_pool.get().unwrap();
        db::get_sync_state(&conn)
    }

    pub fn delete_directory(&self, path: &str) {
        let conn = self.db_pool.get().unwrap();
        db::delete_dir(&conn, path);
    }
}
fn get_cover_path(epub: &EpubProvider) -> Option<String> {
    epub.cover_image_path().ok().flatten()
}

pub fn generate_file_hash(path: &Path) -> u64{
    const CHUNK: usize = 16 * 1024;

    let mut file = File::open(&path).unwrap();
    let file_size = file.metadata().unwrap().len();

    let mut hasher = Xxh3::new();
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

pub fn generate_thumbnail(lib_path: &str, epub_path: &String, id: &String,
) -> Result<(), Box<dyn std::error::Error>> {
    let cleaned_id = id.replace("/", "|");
    let thumbnail_path = format!("{lib_path}/.biblos/thumbnails/{cleaned_id}.png");
    if fs::metadata(&thumbnail_path).is_ok() { return Ok(()); }

    let epub = EpubProvider::new(PathBuf::from(epub_path));
    let image_path = match get_cover_path(&epub) {
        Some(path) => path,
        None => return Ok(()), // Skip if no cover
    };

    let image_type = get_image_type(&image_path).ok_or("Unknown image type")?;
    let image_bytes = epub.read_bytes(&image_path)?;
    let decoded = ImageReader::with_format(Cursor::new(&image_bytes), image_type)
        .decode()?.to_rgb8();

    let (src_width, src_height) = decoded.dimensions();
    let dst_width = 300;
    let scale = dst_width as f32 / src_width as f32;
    let dst_height = (src_height as f32 * scale).round() as u32;

    let src_image = Image::from_vec_u8(src_width, src_height, decoded.into_raw(), PixelType::U8x3)?;
    let mut dst_image = Image::new(dst_width, dst_height, PixelType::U8x3);

    let mut resizer = Resizer::new();
    resizer.resize(&src_image, &mut dst_image, None)?;

    let mut result_buf = BufWriter::new(Vec::new());
    JpegEncoder::new(&mut result_buf)
        .write_image(dst_image.buffer(), dst_width, dst_height, ColorType::Rgb8)
        .unwrap();

    fs::write(thumbnail_path, &result_buf.into_inner().unwrap())?;

    Ok(())
}

pub fn get_image_type(image_path: &str) -> Option<ImageFormat> {
    let file_extension = image_path.split(".").skip(1).next().unwrap();
    match file_extension {
        "jpeg" => Some(ImageFormat::Jpeg),
        "jpg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "gif" => Some(ImageFormat::Gif),
        _ => None,
    }
}
