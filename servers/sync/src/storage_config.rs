use server_asset_store::CatalogStorage;
use std::error::Error;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

const ASSETS_DIRECTORY: &str = "BOKHEIM_ASSETS_DIR";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoragePaths {
    books: PathBuf,
    thumbnails: PathBuf,
    catalog: Option<CatalogStorage>,
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

    pub fn catalog(&self) -> Option<&CatalogStorage> {
        self.catalog.as_ref()
    }

    fn from_values(mut value: impl FnMut(&str) -> Option<OsString>) -> Result<Self, StorageConfigError> {
        let catalog = match (value("BOKHEIM_CATALOG_BOOKS_DIR"), value("BOKHEIM_CATALOG_PINS_DIR")) {
            (None, None) => None,
            (Some(books), Some(pins)) => {
                let books = PathBuf::from(books);
                let pins = PathBuf::from(pins);
                validate_production_path("BOKHEIM_CATALOG_BOOKS_DIR", &books)?;
                validate_production_path("BOKHEIM_CATALOG_PINS_DIR", &pins)?;
                Some(CatalogStorage { books, pins })
            }
            _ => return Err(StorageConfigError::new("BOKHEIM_CATALOG_BOOKS_DIR and BOKHEIM_CATALOG_PINS_DIR must be configured together")),
        };
        let Some(root) = value(ASSETS_DIRECTORY) else {
            return Ok(Self { books: PathBuf::from("./books"), thumbnails: PathBuf::from("./thumbnails"), catalog });
        };
        let root = PathBuf::from(root);
        validate_production_path(ASSETS_DIRECTORY, &root)?;
        Ok(Self { books: root.join("books"), thumbnails: root.join("thumbnails"), catalog })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_requires_both_absolute_paths() {
        assert!(StoragePaths::from_values(|_| None).unwrap().catalog().is_none());
        assert!(StoragePaths::from_values(|name| (name == "BOKHEIM_CATALOG_BOOKS_DIR").then(|| "/catalog/books".into())).is_err());
        assert!(StoragePaths::from_values(|name| match name {
            "BOKHEIM_CATALOG_BOOKS_DIR" => Some("relative/books".into()),
            "BOKHEIM_CATALOG_PINS_DIR" => Some("/private/pins".into()),
            _ => None,
        })
        .is_err());
        let paths = StoragePaths::from_values(|name| match name {
            "BOKHEIM_CATALOG_BOOKS_DIR" => Some("/catalog/books".into()),
            "BOKHEIM_CATALOG_PINS_DIR" => Some("/private/pins".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(paths.catalog().unwrap().pins, PathBuf::from("/private/pins"));
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
