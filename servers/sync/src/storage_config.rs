use std::error::Error;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

const ASSETS_DIRECTORY: &str = "BOKHEIM_ASSETS_DIR";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoragePaths {
    books: PathBuf,
    thumbnails: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub struct StorageConfigError(String);

impl StorageConfigError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for StorageConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for StorageConfigError {}

impl StoragePaths {
    pub fn from_env() -> Result<Self, StorageConfigError> {
        Self::from_values(|name| std::env::var_os(name))
    }

    pub fn into_parts(self) -> (PathBuf, PathBuf) {
        (self.books, self.thumbnails)
    }

    fn from_values(mut value: impl FnMut(&str) -> Option<OsString>) -> Result<Self, StorageConfigError> {
        let Some(root) = value(ASSETS_DIRECTORY) else {
            return Ok(Self { books: PathBuf::from("./books"), thumbnails: PathBuf::from("./thumbnails") });
        };
        let root = PathBuf::from(root);
        validate_production_path(ASSETS_DIRECTORY, &root)?;
        Ok(Self { books: root.join("books"), thumbnails: root.join("thumbnails") })
    }
}

fn validate_production_path(name: &str, path: &Path) -> Result<(), StorageConfigError> {
    if !path.is_absolute() {
        return Err(StorageConfigError::new(format!("{name} must be an absolute path")));
    }
    if path.components().any(|component| matches!(component, Component::CurDir | Component::ParentDir)) {
        return Err(StorageConfigError::new(format!("{name} must be normalized and may not contain . or .. components")));
    }
    Ok(())
}
