//! Shared native file moves for ordinary imports and metadata adapters.

use std::fs::File;
use std::path::{Path, PathBuf};

use client_platform_runtime::executor::run_blocking;
use client_runtime::BackendError;
use content_address::hash_file;
use library_files::filesystem::fingerprint;
use library_files::import_move::ImportMove;
use library_model::PublishedStagedBook;
use sync_common::{BookId, DirId};

use crate::LibraryImport;
use crate::import::ValidatedImport;
use crate::reader_transfer::{ImportMetadata, ReaderPreparation, ingest_import_file, reader_inspection_version};

#[derive(Debug)]
pub struct NativeMoveResult {
    pub content_hash: BookId,
    pub created: bool,
    /// The move committed; only private staging cleanup failed.
    pub cleanup_error: Option<String>,
}

pub(crate) enum MovePreparation {
    Inspect,
    Reader(ReaderPreparation),
}

impl LibraryImport {
    pub async fn move_book_from_path(&self, parent: DirId, source: &Path) -> Result<NativeMoveResult, BackendError> {
        self.move_book_from_path_prepared(parent, source, MovePreparation::Inspect).await.map(|(moved, _)| moved)
    }

    pub(crate) async fn move_book_from_path_prepared(&self, parent: DirId, source: &Path, preparation: MovePreparation) -> Result<(NativeMoveResult, Vec<String>), BackendError> {
        let metadata = match &preparation {
            MovePreparation::Inspect => ImportMetadata::Inspect,
            MovePreparation::Reader(_) => ImportMetadata::Reader,
        };
        let name = source.file_name().and_then(|name| name.to_str()).ok_or("invalid source filename")?.to_owned();
        let import = self.validate_import(&parent, &name)?;
        let destination = self.import_destination(parent).await?;
        let source = source.to_owned();
        let claimed = run_blocking({
            let destination = destination.clone();
            let root = PathBuf::from(&self.root);
            move || ImportMove::claim_in_library(&source, &destination, &root)
        })
        .await
        .map_err(BackendError::operation)?
        .map_err(|error| if error.kind() == std::io::ErrorKind::CrossesDevices { BackendError::operation(error).context("source and destination are on different volumes; use copy mode") } else { BackendError::operation(error) })?;

        let mut claimed = Some(claimed);
        let operation = async {
            // Admit the inode we actually claimed, never bytes read from a
            // pathname that can be replaced before the move.
            let ingested = ingest_import_file(self, claimed.as_ref().unwrap().path(), name.clone(), metadata).await?;
            let path = claimed.as_ref().unwrap().path().to_owned();
            let (ingested, state, warnings) = run_blocking(move || {
                let mut warnings = Vec::new();
                let state = match preparation {
                    MovePreparation::Inspect => None,
                    MovePreparation::Reader(reader) => Some(reader.prepare(&ingested, &path, &mut warnings)?),
                };
                Ok::<_, BackendError>((ingested, state, warnings))
            })
            .await
            .map_err(BackendError::operation)??;
            let checksum = ingested.content_hash.content_hash().ok_or("import has no byte checksum")?;
            let content = self.resolve_import_content(checksum, ingested.size_bytes, ingested.source_identity)?;
            let identity = content.content_hash;
            let assets = self.assets.clone();
            run_blocking(move || assets.remember_local_content(identity, checksum)).await.map_err(BackendError::operation)??;
            let _lease = self.lease_import_content(&content).await?;
            // A folder moved while inspection ran must not receive a stale
            // physical publication under its old path.
            if self.prepared_import_destination(parent)? != destination {
                return Err(BackendError::message("destination folder changed during import"));
            }
            let existing = self.database.transfer_destination_name(&identity, &parent)?;
            let reuse = match existing.as_deref() {
                Some(name) => {
                    let path = destination.join(name);
                    run_blocking(move || -> Result<Option<(PathBuf, u64)>, BackendError> {
                        if !path.is_file() {
                            return Ok(None);
                        }
                        let revision = fingerprint(&path)?;
                        if hash_file(&mut File::open(&path)?)? == checksum && fingerprint(&path)? == revision { Ok(Some((path, revision))) } else { Ok(None) }
                    })
                    .await
                    .map_err(BackendError::operation)??
                }
                None => None,
            };
            let ingested_fingerprint = ingested.fingerprint;
            let occupied = self.database.shared_occupied_file_names(&parent)?;
            let mut claim = claimed.take().unwrap();
            let publish_destination = destination.clone();
            let (claim, publication) = run_blocking(move || {
                let publication = (|| -> Result<(PathBuf, u64), BackendError> {
                    if fingerprint(claim.path())? != ingested_fingerprint {
                        return Err(BackendError::message("source changed after import inspection; retry the import"));
                    }
                    let path = match reuse {
                        Some((path, revision)) => {
                            if fingerprint(&path)? != revision {
                                return Err(BackendError::message("existing book changed during move"));
                            }
                            path
                        }
                        None => claim.publish(&publish_destination, &occupied)?,
                    };
                    let revision = fingerprint(&path)?;
                    Ok((path, revision))
                })();
                (claim, publication)
            })
            .await
            .map_err(BackendError::operation)?;
            claimed = Some(claim);
            let (path, revision) = publication?;
            if fingerprint(&path)? != revision {
                return Err(BackendError::message("published book changed during move"));
            }
            // Revalidate after worker I/O; no await separates these checks,
            // the database commit, and disabling rollback on the claim.
            if self.prepared_import_destination(parent)? != destination {
                return Err(BackendError::message("destination folder changed during import"));
            }
            let name = path.file_name().and_then(|name| name.to_str()).ok_or("invalid published filename")?.to_owned();
            let relative = self.database.native_book_path(parent, &name)?.ok_or("destination folder is missing")?;
            let published = PublishedStagedBook { name: name.clone(), relative_path: relative.to_string(), published: true, fingerprint: Some(fingerprint(&path)?.to_string()) };
            let created = existing.is_none();
            let import = ValidatedImport { file_name: name, ..import };
            if let Some(state) = state {
                let state = state.retarget(identity)?;
                let version = reader_inspection_version(import.format);
                let commit = self.prepare_import_commit(import, content, published, None, version)?;
                self.commit_transferred_import(commit, state, true)?;
            } else {
                self.commit_import(import, content, Some(ingested.inspection), published)?;
            }
            claimed.as_mut().unwrap().mark_committed();
            Ok::<_, BackendError>((identity, created, warnings))
        }
        .await;
        match operation {
            Ok((content_hash, created, warnings)) => {
                let claim = claimed.take().unwrap();
                let cleanup_error = match run_blocking(move || claim.commit()).await {
                    Ok(result) => result.err().map(|error| error.to_string()),
                    Err(error) => Some(error.to_string()),
                };
                if let Some(error) = &cleanup_error {
                    log::warn!("import committed: {error}");
                }
                Ok((NativeMoveResult { content_hash, created, cleanup_error }, warnings))
            }
            Err(error) => {
                if let Some(mut claim) = claimed.take() {
                    let restored = run_blocking(move || claim.rollback()).await.map_err(BackendError::operation)?;
                    if let Err(recovery) = restored {
                        return Err(error.context(recovery.to_string()));
                    }
                }
                Err(error)
            }
        }
    }
}
