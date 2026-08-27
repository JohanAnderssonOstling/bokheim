//! Native file-selection boundary. Files become ordinary SQLite-backed imports immediately.

use super::LibraryBackend;
use crate::app::BackendError;
use crate::{ContentHash, DirId};
use library_model::BookFormat;
use std::fs::File;
use std::path::Path;

impl LibraryBackend {
    pub fn import_book(&self, parent_id: &DirId, source_path: &Path) -> Result<ContentHash, BackendError> {
        self.import_native_book(parent_id, source_path).map_err(BackendError::operation)
    }

    pub fn import_folder(&self, parent_id: &DirId, source_path: &Path) -> Result<Vec<String>, BackendError> {
        self.import_native_folder(parent_id, source_path).map_err(BackendError::operation)
    }

    fn import_native_book(&self, parent_id: &DirId, source_path: &Path) -> Result<ContentHash, Box<dyn std::error::Error>> {
        let source_path = source_path.canonicalize()?;
        if !source_path.is_file() {
            return Err(format!("selected book is not a file: {}", source_path.display()).into());
        }
        let file_name = source_path.file_name().and_then(|name| name.to_str()).ok_or("selected book has no valid UTF-8 filename")?.to_owned();
        let mut source = File::open(source_path)?;
        self.import_book_reader(parent_id, &mut source, file_name, None)
    }

    fn import_native_folder(&self, parent_id: &DirId, source_path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let source_path = source_path.canonicalize()?;
        if !source_path.is_dir() {
            return Err(format!("selected folder is not a directory: {}", source_path.display()).into());
        }
        let folder_name = source_path.file_name().and_then(|name| name.to_str()).ok_or("selected folder has no valid name")?;
        let destination = self.create_library_directory(parent_id, folder_name)?;
        let mut failures = Vec::new();
        self.import_native_folder_contents(&destination.id, &source_path, &mut failures)?;
        Ok(failures)
    }

    fn import_native_folder_contents(&self, parent_id: &DirId, source_path: &Path, failures: &mut Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        let mut entries = std::fs::read_dir(source_path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                match self.create_library_directory(parent_id, &name) {
                    Ok(directory) => {
                        if let Err(error) = self.import_native_folder_contents(&directory.id, &path, failures) {
                            failures.push(format!("{}: {error}", path.display()));
                        }
                    }
                    Err(error) => failures.push(format!("{}: {error}", path.display())),
                }
            } else if file_type.is_file() && path.extension().and_then(|extension| extension.to_str()).and_then(BookFormat::from_import_extension).is_some() {
                if let Err(error) = self.import_native_book(parent_id, &path) {
                    failures.push(format!("{}: {error}", path.display()));
                }
            }
        }
        Ok(())
    }
}
