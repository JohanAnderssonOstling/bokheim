//! Import storage lifecycle. The caller retains the content lease and metadata commit.
#[cfg(not(target_arch = "wasm32"))]
use client_platform_native::filesystem::parent_directory;
use super::{AssetStore, AssetStoreError, CopiedBook};
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
                return client_platform_runtime::executor::run_blocking(move || {
                    use crate::asset_store::native::{DIRECT_IMPORTS, DirectBook, placement_path, write_import_bytes};
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
                        write_import_bytes(file.as_file_mut(), source.as_mut(), &directory)?;
                        return Ok((name, CopiedBook { inner: super::imp::CopiedBook { temporary_path: file.into_temp_path(), direct: Some(direct) } }));
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
            let inner = super::imp::stage_book(&self.0, source.as_mut()).await?;
            Ok((relative.file_name().to_owned(), super::StagedBook { inner }))
        }
    }
}
