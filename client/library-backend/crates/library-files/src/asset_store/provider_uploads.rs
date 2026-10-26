//! Atomic device-owned import snapshots. The book and a credential-free
//! manifest survive a process exit even before SQLite records the queue row.

use super::{imp, AssetStore, AssetStoreError, StagedBook};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::PathBuf,
};
use sync_common::BookId;
use uuid::Uuid;

const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

fn directory(store: &AssetStore, id: Uuid) -> PathBuf {
    store.0.assets_root.join("provider-uploads").join(id.to_string())
}

fn manifest(store: &AssetStore, id: Uuid) -> Result<Vec<u8>, AssetStoreError> {
    read_manifest(&directory(store, id))
}

fn read_manifest(path: &std::path::Path) -> Result<Vec<u8>, AssetStoreError> {
    let mut bytes = Vec::new();
    File::open(path.join("draft.json"))?.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(AssetStoreError::operation("pending provider import exceeds its metadata limit"));
    }
    Ok(bytes)
}

impl StagedBook {
    pub async fn retain_provider_upload(self, store: AssetStore, id: Uuid, metadata: Vec<u8>) -> Result<(), AssetStoreError> {
        client_platform_runtime::executor::run_blocking(move || {
            if metadata.len() as u64 > MAX_MANIFEST_BYTES || self.content_hash().is_external() || !imp::root_available(&store.0) {
                return Err(AssetStoreError::operation("invalid pending provider import or unavailable library storage"));
            }
            let base = store.0.assets_root.join("provider-uploads");
            fs::create_dir_all(&base)?;
            let destination = directory(&store, id);
            if destination.exists() {
                if manifest(&store, id)? != metadata || !imp::content_matches(&destination.join("book"), &self.content_hash())? {
                    return Err(AssetStoreError::operation("pending provider import differs from its retained source"));
                }
                return Ok(());
            }
            let staging = tempfile::Builder::new().prefix(&format!(".preparing-{id}-")).tempdir_in(&base)?;
            let mut file = OpenOptions::new().write(true).create_new(true).open(staging.path().join("draft.json"))?;
            file.write_all(&metadata)?;
            file.sync_all()?;
            let imp::StagedBook { temporary_path, .. } = self.inner;
            temporary_path.persist(staging.path().join("book")).map_err(|error| AssetStoreError::operation(error.error))?;
            File::open(staging.path().join("book"))?.sync_all()?;
            imp::sync_parent(&staging.path().join("draft.json"))?;
            fs::rename(staging.path(), &destination)?;
            imp::sync_parent(&destination)
        })
        .await
        .map_err(AssetStoreError::operation)?
    }
}

impl AssetStore {
    pub fn retained_provider_uploads(&self) -> Result<Vec<(Uuid, Vec<u8>)>, AssetStoreError> {
        let entries = match fs::read_dir(self.0.assets_root.join("provider-uploads")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut drafts = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let id = if let Ok(id) = name.parse::<Uuid>() {
                id
            } else {
                let Some(id) = name.strip_prefix(".preparing-").and_then(|name| name.get(..36)).and_then(|id| id.parse::<Uuid>().ok()) else { continue };
                if !entry.path().join("book").is_file() || !entry.path().join("draft.json").is_file() {
                    continue;
                }
                let destination = directory(self, id);
                if destination.exists() {
                    if read_manifest(&entry.path())? != manifest(self, id)? {
                        return Err(AssetStoreError::operation("interrupted cloud import has conflicting source metadata"));
                    }
                    let source = entry.path().join("book");
                    let previous = destination.join("book");
                    if previous.exists() {
                        if content_address::hash_file(&mut File::open(&source)?)? != content_address::hash_file(&mut File::open(&previous)?)? {
                            return Err(AssetStoreError::operation("interrupted cloud import has conflicting book bytes"));
                        }
                    } else {
                        fs::rename(&source, &previous)?;
                    }
                    fs::remove_dir_all(entry.path())?;
                } else {
                    fs::rename(entry.path(), &destination)?;
                }
                imp::sync_parent(&destination)?;
                id
            };
            drafts.push((id, manifest(self, id)?));
        }
        Ok(drafts)
    }

    pub fn open_provider_upload(&self, id: Uuid) -> Result<File, AssetStoreError> {
        if !imp::root_available(&self.0) {
            return Err(AssetStoreError::operation("library storage is unavailable"));
        }
        File::open(directory(self, id).join("book")).map_err(Into::into)
    }

    pub fn stage_retained_provider_upload(&self, upload: Uuid, book: BookId, size: u64) -> Result<Option<StagedBook>, AssetStoreError> {
        if !book.is_external() {
            return Err(AssetStoreError::operation("expected a confirmed provider book"));
        }
        let path = directory(self, upload).join("book");
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() && metadata.len() == size => {}
            Ok(_) => return Err(AssetStoreError::operation("retained provider import changed size")),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        Ok(Some(StagedBook { inner: imp::StagedBook { temporary_path: tempfile::TempPath::try_from_path(path)?, content_hash: book, size_bytes: size, direct: None } }))
    }

    pub fn remove_provider_upload(&self, id: Uuid) -> Result<(), AssetStoreError> {
        let path = directory(self, id);
        match fs::remove_dir_all(&path) {
            Ok(()) => imp::sync_parent(&path),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_model::RelativeBookPath;
    use std::io::Cursor;

    #[tokio::test]
    async fn retained_import_survives_reopen_and_publishes_with_provider_identity() {
        let root = tempfile::tempdir().unwrap();
        let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let bytes = b"complete local import";
        let digest = BookId::from(blake3::hash(bytes));
        let copied = store.stage_book(RelativeBookPath::parse("book.epub").unwrap(), Box::new(Cursor::new(bytes))).await.unwrap();
        let temporary = copied.path().to_owned();
        let upload = Uuid::new_v4();
        copied.into_staged(digest, bytes.len() as u64).retain_provider_upload(store.clone(), upload, b"manifest".to_vec()).await.unwrap();
        assert!(!temporary.exists());
        drop(store);
        let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        assert_eq!(store.retained_provider_uploads().unwrap(), vec![(upload, b"manifest".to_vec())]);
        let mut recovered = Vec::new();
        store.open_provider_upload(upload).unwrap().read_to_end(&mut recovered).unwrap();
        assert_eq!(recovered, bytes);
        let id = BookId::for_provider("google_drive", "account", "my_drive", "file");
        let staged = store.stage_retained_provider_upload(upload, id, bytes.len() as u64).unwrap().unwrap();
        let path = RelativeBookPath::parse("book.epub").unwrap();
        staged.publish_snapshot(&store, &path, &[], false).unwrap();
        assert!(store.book_exists(&id, &[path]).unwrap());
        store.remove_provider_upload(upload).unwrap();
        assert!(store.retained_provider_uploads().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_process_exit_before_queue_directory_rename_is_recovered() {
        let root = tempfile::tempdir().unwrap();
        let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let bytes = b"queued book";
        let copied = store.stage_book(RelativeBookPath::parse("book.epub").unwrap(), Box::new(Cursor::new(bytes))).await.unwrap();
        let upload = Uuid::new_v4();
        copied.into_staged(BookId::from(blake3::hash(bytes)), bytes.len() as u64).retain_provider_upload(store.clone(), upload, b"manifest".to_vec()).await.unwrap();
        let ready = directory(&store, upload);
        let interrupted = ready.parent().unwrap().join(format!(".preparing-{upload}-interrupted"));
        fs::rename(&ready, &interrupted).unwrap();
        assert_eq!(store.retained_provider_uploads().unwrap(), vec![(upload, b"manifest".to_vec())]);
        assert!(ready.join("book").is_file());
        assert!(!interrupted.exists());
        let mut reader = store.open_provider_upload(upload).unwrap();
        assert_eq!(content_address::hash_file(&mut reader).unwrap(), digest(bytes));
    }

    fn digest(bytes: &[u8]) -> content_address::ContentHash {
        blake3::hash(bytes).into()
    }
}
