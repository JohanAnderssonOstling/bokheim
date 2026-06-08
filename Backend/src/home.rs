use crate::BookID;
use csv::{Reader, WriterBuilder};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub library_id: String,
    pub library_name: String,
    pub library_path: String,
}

static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_app_data_dir(path: PathBuf) -> Result<(), String> {
    APP_DATA_DIR.set(path).map_err(|_| "app data dir already initialized".to_string())
}

pub fn get_thumbnail(book_id: BookID) -> Option<Vec<u8>> {
    match resolve_thumbnail(book_id) {
        None => None,
        Some(thumb_path) => match fs::read(&thumb_path) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                log::warn!("Failed to read book cover thumbnail for book {} at {}: {}", book_id, thumb_path.to_string_lossy(), err);
                None
            }
        },
    }
}

pub fn resolve_thumbnail(book_id: BookID) -> Option<PathBuf> {
    let thumb_name = format!("{}.jpg", book_id);
    let thumb_path = get_app_data_dir().join("thumbnails").join(thumb_name);
    match thumb_path.exists() {
        true => Some(thumb_path),
        false => None,
    }
}

fn get_app_data_dir() -> PathBuf {
    APP_DATA_DIR.get_or_init(|| panic!("app data dir not initialized; call home::set_app_data_dir(...) before using backend")).clone()
}

fn ensure_dir(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(path)?;
    Ok(())
}

fn append_library_entry(csv_path: &Path, entry: &LibraryEntry) -> Result<(), Box<dyn std::error::Error>> {
    let needs_headers = !csv_path.exists() || fs::metadata(csv_path).map_or(true, |m| m.len() == 0);
    let file = OpenOptions::new().create(true).append(true).open(csv_path)?;
    let mut writer = WriterBuilder::new().has_headers(needs_headers).from_writer(file);
    writer.serialize(entry)?;
    writer.flush()?;
    Ok(())
}

pub fn get_app_data_thumbnails_dir() -> PathBuf {
    let thumbnails_dir = get_app_data_dir().join("thumbnails");
    if let Err(e) = fs::create_dir_all(&thumbnails_dir) {
        log::error!("Failed to create thumbnails directory: {}", e);
    }
    thumbnails_dir
}

pub fn create_library(name: String, root: Option<PathBuf>) -> Result<LibraryEntry, Box<dyn std::error::Error>> {
    ensure_dir(&get_app_data_dir())?;
    let csv_path = get_app_data_dir().join("libraries.csv");
    let library_id = Uuid::new_v4().to_string();
    let library_root = match root {
        Some(path) => path,
        None => {
            let path = get_app_data_dir().join("libraries").join(&library_id);
            ensure_dir(&path)?;
            path
        }
    };

    let existing_libraries = get_libraries();
    if existing_libraries.contains(&library_root) {
        return Err(format!("library path already exists: {}", library_root.display()).into());
    }

    ensure_dir(&library_root)?;

    let entry = LibraryEntry { library_id, library_name: if name.trim().is_empty() { "Library".to_string() } else { name }, library_path: library_root.to_string_lossy().to_string() };

    append_library_entry(&csv_path, &entry)?;
    Ok(entry)
}

pub fn get_libraries() -> Vec<PathBuf> {
    get_library_entries().into_iter().map(|entry| PathBuf::from(entry.library_path)).collect()
}

pub fn get_library_entries() -> Vec<LibraryEntry> {
    let csv_path = get_app_data_dir().join("libraries.csv");
    // Return empty vector if file doesn't exist
    if !Path::new(&csv_path).exists() {
        return Vec::new();
    }
    let mut reader = Reader::from_path(&csv_path).unwrap();
    reader
        .deserialize()
        .map(|res| res.unwrap())
        // .filter_map(|res: Result<LibraryEntry, _>| res.ok())
        //.filter(|entry| Path::new(&entry.library_path).exists())
        .collect()
}

pub fn get_library_path_by_id(library_id: &str) -> Option<PathBuf> {
    get_library_entries().into_iter().find(|entry| entry.library_id == library_id).map(|entry| PathBuf::from(entry.library_path))
}

pub fn remove_library(path: &str) {
    let csv_path = get_app_data_dir().join("libraries.csv");
    println!("Csv: {}", csv_path.to_str().unwrap());
    let entries = read_library_entries(&csv_path);
    if entries.is_empty() {
        return;
    }

    let remaining: Vec<LibraryEntry> = entries.into_iter().filter(|entry| entry.library_path != path).collect();

    if remaining.is_empty() {
        let _ = fs::remove_file(&csv_path);
    } else {
        let file = fs::File::create(&csv_path).unwrap();
        let mut writer = WriterBuilder::new().has_headers(true).from_writer(file);
        for entry in remaining {
            writer.serialize(entry).unwrap();
        }
        writer.flush().unwrap();
    }

    let assets_dir = Path::new(path).join(".biblos");
    if assets_dir.exists() {
        let _ = fs::remove_dir_all(assets_dir);
    }
}

fn read_library_entries(csv_path: &PathBuf) -> Vec<LibraryEntry> {
    if !Path::new(csv_path).exists() {
        return Vec::new();
    }

    match Reader::from_path(csv_path) {
        Ok(mut reader) => reader.deserialize().filter_map(|res: Result<LibraryEntry, _>| res.ok()).collect(),
        Err(_) => Vec::new(),
    }
}
