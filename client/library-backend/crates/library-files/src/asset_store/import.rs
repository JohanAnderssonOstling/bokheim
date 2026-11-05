//! Import storage lifecycle. The caller retains the content lease and metadata commit.
use super::{AssetStore, AssetStoreError, CopiedBook, PublishedStagedBook, StagedBook};
#[cfg(not(target_arch = "wasm32"))]
use client_platform_native::filesystem::parent_directory;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::executor::run_blocking;
#[cfg(target_arch = "wasm32")]
use client_platform_runtime::executor::storage_call;
use library_model::RelativeBookPath;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::PoisonError;

#[derive(Default)]
pub struct ImportOptions {
    /// Native imports may write directly to a collision-free destination.
    pub direct: bool,
}

impl AssetStore {
    /// Prepare storage and stage bytes, returning the actual destination name.
    pub async fn copy_import(&self, relative: RelativeBookPath, mut source: Box<dyn std::io::Read + Send>, occupied: Vec<String>, options: ImportOptions) -> Result<(String, CopiedBook), AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let name = relative.file_name().to_owned();
            if options.direct {
                let mut occupied = occupied;
                let store = self.clone();
                let root = self.lock_root_shared().await;
                return run_blocking(move || {
                    let _root = root;
                    if !store.root_available() {
                        return Err(AssetStoreError::operation("library folder is unavailable"));
                    }
                    use crate::asset_store::native::{placement_path, write_import_bytes, DirectBook, DIRECT_IMPORTS};
                    let requested = relative.file_name();
                    let directory = parent_directory(&placement_path(&store.0, &relative))?.canonicalize()?;
                    occupied.extend(std::fs::read_dir(&directory)?.map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned())).collect::<Result<Vec<_>, _>>()?);
                    for name in library_replica::file_name_candidates(requested, occupied.iter().map(String::as_str)) {
                        let path = directory.join(&name);
                        let imports = DIRECT_IMPORTS.get_or_init(Default::default);
                        if !imports.lock().unwrap_or_else(PoisonError::into_inner).insert(path.clone()) {
                            continue;
                        }
                        let direct = DirectBook { path: path.clone() };
                        let file = match std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path) {
                            Ok(file) => file,
                            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                                continue;
                            }
                            Err(error) => return Err(AssetStoreError::operation(format!("could not create the imported book '{}': {error}", path.display()))),
                        };
                        // Wrap the final file only to clean it up on copy/inspection failure.
                        // No temporary filename, rename, hard link, or second copy is involved.
                        let cleanup = tempfile::TempPath::try_from_path(path)?;
                        let mut file = tempfile::NamedTempFile::from_parts(file, cleanup);
                        let (checksum, size) = write_import_bytes(&store.0, file.as_file_mut(), source.as_mut(), &directory)?;
                        let copied_digest = Some((checksum, size, crate::filesystem::fingerprint(file.path())?));
                        return Ok((name, CopiedBook { inner: super::imp::CopiedBook { copied_digest, temporary_path: file.into_temp_path(), direct: Some(direct) } }));
                    }
                    unreachable!("numeric name suffix space is unbounded")
                })
                .await
                .map_err(AssetStoreError::operation)?;
            }
            Ok((name, self.stage_book(relative, source).await?))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (occupied, options);
            let store = self.clone();
            storage_call(move || async move {
                let inner = super::imp::stage_book(&store.0, source.as_mut()).await?;
                Ok((relative.file_name().to_owned(), StagedBook { inner }))
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
    }

    /// Prepare, publish and flush one import using a resolved placement. The
    /// caller validates its destination first and commits metadata afterward.
    pub async fn publish_import(&self, staged: StagedBook, relative: RelativeBookPath, occupied: Vec<String>, audiobook_archive: bool) -> Result<PublishedStagedBook, AssetStoreError> {
        // Dispatched work may outlive the caller, so retain its own file lease.
        let lease = self.lease_book(&staged.book_id()).await?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let store = self.clone();
            let root = self.lock_root_shared().await;
            run_blocking(move || {
                let _access = (root, lease);
                if !store.root_available() {
                    return Err(AssetStoreError::operation("library folder is unavailable"));
                }
                staged.publish_snapshot(&store, &relative, &occupied, audiobook_archive)
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (occupied, audiobook_archive);
            storage_call(move || async move {
                let _lease = lease;
                staged.commit().await?;
                Ok(PublishedStagedBook { name: relative.file_name().to_owned(), relative_path: String::new(), published: true, fingerprint: None })
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
    }
}
