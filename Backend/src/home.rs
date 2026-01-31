use csv::{Reader, WriterBuilder};
use directories_next::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use uuid::Uuid;
use crate::BookID;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub library_id: String,
    pub library_name: String,
    pub library_path: String,
}

static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn get_thumbnail(book_id: BookID) -> Option<Vec<u8>> {
    match resolve_thumbnail(book_id) {
        None => {None}
        Some(thumb_path) => {fs::read(&thumb_path).ok()}
    }
}

pub fn resolve_thumbnail(book_id: BookID) -> Option<PathBuf>{
    let thumb_name = format!("{}.jpg", book_id);
    let thumb_path = get_app_data_dir().join("thumbnails").join(thumb_name);
    match thumb_path.exists() {
        true => {Some(thumb_path)},
        false => {None}
    }
}

fn get_app_data_dir() -> PathBuf {
    log::info!("Creating app data dir");
    APP_DATA_DIR.get_or_init(|| {
            #[cfg(target_os = "android")]
            let dir = "/data/data/com.epub_svelte.app/files".to_string();

            #[cfg(not(target_os = "android"))]
            let dir = ProjectDirs::from("dev", "johandost", "biblos")
                .expect("Could not determine app data directory")
                .data_dir()
                .to_path_buf();

            // Create the directory (and parent directories) if it doesn't exist
            if let Err(e) = fs::create_dir_all(&dir) {
                panic!("Failed to create app data directory: {}", e);
            }

            //let dir = dir.to_str().unwrap().to_string();
            dir
        })
        .clone()
}

pub fn create_library(path: &PathBuf) {
    let existing_libraries = get_libraries();
    if existing_libraries.contains(path) {
        return;
    }
    let csv_path = get_app_data_dir().join("libraries.csv");
    let library_id = Uuid::new_v4().to_string();

    let entry = LibraryEntry {
        library_id,
        library_name: path.file_name().unwrap().to_str().unwrap().to_string(),
        library_path: path.to_str().unwrap().to_string(),
    };

    // Check if file exists and has content to determine if we need headers
    let needs_headers =
        !Path::new(&csv_path).exists() || fs::metadata(&csv_path).map_or(true, |m| m.len() == 0);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&csv_path)
        .unwrap();
    let mut writer = WriterBuilder::new()
        .has_headers(needs_headers)
        .from_writer(file);
    writer.serialize(&entry).unwrap();
    writer.flush().unwrap();
    log::info!("biblos: Created new library");
}

pub fn get_libraries() -> Vec<PathBuf> {
    get_library_entries()
        .into_iter()
        .map(|entry| PathBuf::from(entry.library_path))
        .collect()
}

pub fn get_library_entries() -> Vec<LibraryEntry> {
    let csv_path = get_app_data_dir().join("libraries.csv");
    // Return empty vector if file doesn't exist
    println!("CSV: {}", csv_path.to_str().unwrap());
    if !Path::new(&csv_path).exists() {
        println!("Not returning");
        return Vec::new();
    }
    let mut reader = Reader::from_path(&csv_path).unwrap();
    reader.deserialize()
        .map(|res| res.unwrap())
           // .filter_map(|res: Result<LibraryEntry, _>| res.ok())
            //.filter(|entry| Path::new(&entry.library_path).exists())
            .collect()

}

pub fn remove_library(path: &str) {
    let csv_path = get_app_data_dir().join("libraries.csv");
    println!("Csv: {}", csv_path.to_str().unwrap());
    let entries = read_library_entries(&csv_path);
    if entries.is_empty() {
        return;
    }

    let remaining: Vec<LibraryEntry> = entries
        .into_iter()
        .filter(|entry| entry.library_path != path)
        .collect();

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
        Ok(mut reader) => reader
            .deserialize()
            .filter_map(|res: Result<LibraryEntry, _>| res.ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}
