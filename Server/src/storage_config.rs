use std::error::Error;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

const BOOKS_DIRECTORY: &str = "BOKHEIM_BOOKS_DIR";
const THUMBNAILS_DIRECTORY: &str = "BOKHEIM_THUMBNAILS_DIR";

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
        match (value(BOOKS_DIRECTORY), value(THUMBNAILS_DIRECTORY)) {
            (None, None) => Ok(Self { books: PathBuf::from("./books"), thumbnails: PathBuf::from("./thumbnails") }),
            (Some(_), None) => Err(StorageConfigError::new(format!("{THUMBNAILS_DIRECTORY} is required when {BOOKS_DIRECTORY} is configured"))),
            (None, Some(_)) => Err(StorageConfigError::new(format!("{BOOKS_DIRECTORY} is required when {THUMBNAILS_DIRECTORY} is configured"))),
            (Some(books), Some(thumbnails)) => Self::production(PathBuf::from(books), PathBuf::from(thumbnails)),
        }
    }

    fn production(books: PathBuf, thumbnails: PathBuf) -> Result<Self, StorageConfigError> {
        validate_production_path(BOOKS_DIRECTORY, &books)?;
        validate_production_path(THUMBNAILS_DIRECTORY, &thumbnails)?;
        if books.starts_with(&thumbnails) || thumbnails.starts_with(&books) {
            return Err(StorageConfigError::new(format!("{BOOKS_DIRECTORY} and {THUMBNAILS_DIRECTORY} must be separate, non-nested directories")));
        }
        Ok(Self { books, thumbnails })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn paths(values: &[(&str, &str)]) -> Result<StoragePaths, StorageConfigError> {
        let values: HashMap<_, _> = values.iter().map(|(key, value)| ((*key).to_owned(), OsString::from(value))).collect();
        StoragePaths::from_values(|name| values.get(name).cloned())
    }

    #[test]
    fn local_development_has_isolated_relative_defaults() {
        assert_eq!(paths(&[]).unwrap().into_parts(), (PathBuf::from("./books"), PathBuf::from("./thumbnails")));
    }

    #[test]
    fn production_paths_are_an_atomic_pair() {
        let error = paths(&[(BOOKS_DIRECTORY, "/srv/bokheim/books")]).unwrap_err();
        assert!(error.to_string().contains(THUMBNAILS_DIRECTORY));
    }

    #[test]
    fn production_paths_must_be_absolute_normalized_and_separate() {
        for values in [
            [(BOOKS_DIRECTORY, "books"), (THUMBNAILS_DIRECTORY, "/srv/bokheim/thumbnails")],
            [(BOOKS_DIRECTORY, "/srv/bokheim/../books"), (THUMBNAILS_DIRECTORY, "/srv/bokheim/thumbnails")],
            [(BOOKS_DIRECTORY, "/srv/bokheim/assets"), (THUMBNAILS_DIRECTORY, "/srv/bokheim/assets/thumbnails")],
        ] {
            assert!(paths(&values).is_err());
        }
    }

    #[test]
    fn accepts_distinct_absolute_production_paths() {
        let configured = paths(&[(BOOKS_DIRECTORY, "/var/lib/bokheim/assets/books"), (THUMBNAILS_DIRECTORY, "/var/lib/bokheim/assets/thumbnails")]).unwrap();
        assert_eq!(configured.into_parts(), (PathBuf::from("/var/lib/bokheim/assets/books"), PathBuf::from("/var/lib/bokheim/assets/thumbnails")));
    }
}
