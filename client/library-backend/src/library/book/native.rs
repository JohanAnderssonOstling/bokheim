//! Native book readers, filesystem availability, and playback resources.

use super::*;
use crate::asset_store::AssetStore;
use crate::library::downloads::persistent_download_state;
use crate::library::DocumentBook;
use crate::library::ResolvedBook;
use crate::DownloadState;
use book_access::{FetchFuture, RemoteFileError};
use library_database::DatabaseClient;
use library_model::RelativeBookPath;
use library_placement::LibraryState;
use remote_book_access::RemoteBookAccess;
pub(crate) type Opened = ResolvedBook;

pub(super) async fn local(library: &LibraryResources, storage: &LibraryState, hash: BookId, info: &LocalBookInfo) -> Result<Option<Opened>, BackendError> {
    let download = persistent_download_state(storage.database(), storage.assets(), hash).await?;
    if !matches!(download, DownloadState::Downloaded) {
        return Ok(None);
    }
    if info.format == BookFormat::Mp3Folder {
        return library.prepare_audiobook_track(storage, hash, 0).await.map(Some);
    }
    let mut path = info.path.clone().ok_or_else(|| BackendError::message("book placement not found"))?;
    let format = info.format;
    if !matches!(format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b | BookFormat::Fb2 | BookFormat::Cbz) {
        return Err(format!("the {} format is no longer supported", format.canonical_extension()).into());
    }
    if let Ok(document_format) = DocumentFormat::try_from(format) {
        let source = open_with_replay(storage.database(), storage.assets(), hash, &mut path, |path| storage.assets().open_async_book_at_path(path)).await?;
        let source = source.ok_or_else(|| "book content is not downloaded")?;
        return Ok(Some(ResolvedBook::Document(DocumentBook::from_async_source(path.to_string(), document_format, source))));
    }
    if format == BookFormat::Pdf {
        return Ok(Some(ResolvedBook::Pdf(local_pdf(storage.database(), storage.assets(), hash, &mut path).await?)));
    }
    #[cfg(target_os = "android")]
    let playback_file = if format == BookFormat::M4b { storage.assets().open_playback_file(&path).await? } else { None };
    #[cfg(target_os = "android")]
    let reader = if let Some(file) = &playback_file { Some(Box::new(file.try_clone()?) as crate::BoxedBookReader) } else { storage.assets().open_book_at_path(path.clone()).await? };
    #[cfg(not(target_os = "android"))]
    let reader = open_with_replay(storage.database(), storage.assets(), hash, &mut path, |path| async move { storage.assets().open_book_at_path(path).await }).await?;
    let reader = reader.ok_or_else(|| "book content is not downloaded")?;
    let book = ResolvedBook::Audiobook(crate::library::AudiobookBook {
        format,
        path: path.to_string(),
        reader,
        stream_control: None,
        #[cfg(target_os = "android")]
        playback_file,
    });
    Ok(Some(book))
}

// Pin a file revision and its matching metadata while publication is excluded.
// The returned reader retains its file handle, not the book lease: exports can
// atomically replace the library file while this reader keeps the old revision.
async fn local_pdf(db: &DatabaseClient, assets: &AssetStore, hash: BookId, path: &mut RelativeBookPath) -> Result<PdfBook, BackendError> {
    let opened = open_with_replay(db, assets, hash, path, |path: RelativeBookPath| async move {
        // Acquire for each attempt so a missing path releases the lease before
        // replaying placement work, which may itself need exclusive access.
        let _opening = assets.lease_book(&hash).await?;
        let Some(reader) = assets.open_book_at_path(path.clone()).await? else { return Ok(None) };
        let metadata = db.local_pdf_metadata(&hash, path.as_str()).await?;
        Ok::<_, BackendError>(Some((reader, metadata)))
    })
    .await?;
    let (reader, metadata) = opened.ok_or("book content is not downloaded")?;
    Ok(PdfBook { path: path.to_string(), reader, metadata })
}

// A placement move can land between reading `info.path` and opening it.
// Drain this hash's pending work and retry once against the fresh path.
async fn open_with_replay<T, E: Into<BackendError>, F: std::future::Future<Output = Result<Option<T>, E>> + Send>(
    db: &DatabaseClient, assets: &AssetStore, book_id: BookId, path: &mut RelativeBookPath, mut open: impl FnMut(RelativeBookPath) -> F + Send,
) -> Result<Option<T>, BackendError> {
    if let Some(reader) = open(path.clone()).await.map_err(Into::into)? {
        return Ok(Some(reader));
    }
    let _ = library_placement::run_file_jobs_for(assets, db, &book_id).await;
    if let Some(fresh) = db.local_book_info(&book_id).await?.path {
        *path = fresh;
        return open(path.clone()).await.map_err(Into::into);
    }
    Ok(None)
}

pub(super) async fn remote(library: &LibraryResources, storage: &LibraryState, hash: BookId, info: &LocalBookInfo) -> Result<Opened, BackendError> {
    if info.format == BookFormat::Mp3Folder {
        return library.prepare_audiobook_track(storage, hash, 0).await;
    }
    let path = info.path.as_ref().ok_or_else(|| BackendError::message("book placement not found"))?;
    let resolved = library.remote_book_source(storage, hash, info.format).await?;
    let (source, prefix) = match resolved.source {
        BookSource::RemoteArchive { source, tail } => {
            let length = source.length;
            let seed_offset = length.checked_sub(tail.len() as u64).ok_or("invalid ZIP tail")?;
            let access = RemoteBookAccess::new(library.sync.http_client().clone(), library.account.clone(), library.id, source);
            let source = SparseBookSource::remote(length, seed_offset, tail, access.clone().fetch()).map_err(BackendError::operation)?;
            let source = source.with_remote_prefetch(access.batch_fetch()).map_err(BackendError::operation)?;
            let format = DocumentFormat::try_from(resolved.format)?;
            let source = format.prepare_archive(source).await?;
            return Ok(ResolvedBook::Document(DocumentBook::from_async_source(path.to_string(), format, source)));
        }
        BookSource::Remote { source, prefix } => (source, prefix),
        _ => return Err(BackendError::message("expected a remote book source")),
    };
    let access = RemoteBookAccess::new(library.sync.http_client().clone(), library.account.clone(), library.id, source.clone());
    let length = source.length;
    if info.format == BookFormat::Pdf {
        let fetch = book_access::remote_file::native_bundle_fetch(access.batch_fetch()).map_err(BackendError::operation)?;
        let reader = book_access::pdf_bundle::open(length, prefix, fetch).map_err(BackendError::operation)?;
        return Ok(ResolvedBook::Pdf(PdfBook { path: path.to_string(), reader, metadata: resolved.pdf_metadata }));
    }
    if info.format == BookFormat::M4b {
        let (reader, stream_control) = book_access::remote_file::native_audio_reader(length, prefix, access.fetch()).map_err(BackendError::operation)?;
        return Ok(ResolvedBook::Audiobook(AudiobookBook {
            format: info.format,
            path: path.to_string(),
            reader,
            stream_control: Some(stream_control),
            #[cfg(target_os = "android")]
            playback_file: None,
        }));
    }
    if let Ok(document_format) = DocumentFormat::try_from(info.format) {
        let source = access.async_source(length, prefix).map_err(BackendError::operation)?;
        return Ok(ResolvedBook::Document(DocumentBook::from_async_source(path.to_string(), document_format, source)));
    }
    Err(BackendError::message("unsupported remote book format"))
}

impl LibraryResources {
    pub(crate) async fn prepare_audiobook_track(&self, storage: &LibraryState, hash: BookId, index: usize) -> Result<ResolvedBook, BackendError> {
        let info = storage.database().local_book_info(&hash).await?;
        if info.format != BookFormat::Mp3Folder {
            return Err(BackendError::message("book is not an MP3 folder"));
        }
        let track = storage.database().audiobook_tracks(&hash).await?.into_iter().nth(index).ok_or_else(|| BackendError::message("audiobook track is unavailable"))?;
        if !track.valid() {
            return Err(BackendError::message("audiobook track index is invalid"));
        }
        let mut path = info.path.ok_or_else(|| BackendError::message("audiobook placement is unavailable"))?;
        if matches!(persistent_download_state(storage.database(), storage.assets(), hash).await?, DownloadState::Downloaded) {
            let reader = open_with_replay(storage.database(), storage.assets(), hash, &mut path, async |path| storage.assets().open_audiobook_track(path, track.name.clone()).await).await?;
            if let Some(reader) = reader {
                return Ok(ResolvedBook::Audiobook(AudiobookBook {
                    format: BookFormat::Mp3Folder,
                    path: path.to_string(),
                    reader,
                    stream_control: None,
                    #[cfg(target_os = "android")]
                    playback_file: None,
                }));
            }
        }
        let source = describe_remote_file(self.sync.http_client(), &self.account, self.id, hash).await.map_err(BackendError::operation)?.0;
        if track.archive_hash != Some(source.hash) {
            return Err(BackendError::message("audiobook track index does not match the remote archive; sync the book metadata and retry"));
        }
        let archive_length = source.length;
        let access = RemoteBookAccess::new(self.sync.http_client().clone(), self.account.clone(), self.id, source);
        let archive = book_access::remote_file::native_reader(archive_length, Vec::new(), access.clone().fetch()).map_err(BackendError::operation)?;
        let member = client_platform_runtime::executor::run_blocking(move || audiobook_archive::archived_track(archive, index)).await.map_err(BackendError::operation)?.map_err(BackendError::operation)?;
        if member.name != track.name || member.length != track.length || member.offset.checked_add(member.length).is_none_or(|end| end > archive_length) {
            return Err(BackendError::message("remote audiobook track differs from its metadata"));
        }
        let prefix = access.read(member.offset, member.length.min(64 * 1024) as usize).await.map_err(BackendError::operation)?;
        let base = member.offset;
        let length = member.length;
        let fetch = move |offset: u64, count: usize| {
            let access = access.clone();
            Box::pin(async move {
                if offset.checked_add(count as u64).is_none_or(|end| end > length) {
                    return Err(RemoteFileError::Failed("audiobook track range is invalid".into()));
                }
                access.read(base + offset, count).await
            }) as FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>
        };
        let (reader, stream_control) = book_access::remote_file::native_audio_reader(length, prefix, fetch).map_err(BackendError::operation)?;
        Ok(ResolvedBook::Audiobook(AudiobookBook {
            format: BookFormat::Mp3Folder,
            path: path.to_string(),
            reader,
            stream_control: Some(stream_control),
            #[cfg(target_os = "android")]
            playback_file: None,
        }))
    }
}

pub struct AudiobookBook {
    pub format: BookFormat,
    pub path: String,
    pub reader: crate::library::BoxedBookReader,
    pub stream_control: Option<crate::library::AudioStreamControl>,
    #[cfg(target_os = "android")]
    pub playback_file: Option<std::fs::File>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{lifecycle::concurrency_tests::fixture, DocumentSource};
    use book_inspection::InspectedBook;
    use book_model::BookRecord;
    use futures_util::io::AsyncReadExt;
    use library_database::ImportCommit;
    use library_model::PublishedStagedBook;
    use std::io::Read;
    use sync_common::ROOT_DIR_ID;

    #[tokio::test]
    async fn local_readers_recover_a_stale_path_after_a_placement_move() {
        for format in [BookFormat::Epub, BookFormat::Pdf, BookFormat::M4b] {
            let root = tempfile::tempdir().unwrap();
            let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
            let library = &service;
            let bytes = b"local book bytes";
            let hash = BookId::from(blake3::hash(bytes));
            let name = format!("Book.{}", format.canonical_extension());
            std::fs::write(root.path().join(&name), bytes).unwrap();
            library
                .state
                .read()
                .await
                .database()
                .commit_import(ImportCommit {
                    source_identity: None,
                    byte_checksum: None,
                    audiobook_content_hash: None,
                    parent_id: ROOT_DIR_ID,
                    file_name: name.clone(),
                    format,
                    book_id: hash,
                    size_bytes: bytes.len() as u64,
                    inspection_version: 1,
                    inspection: Some(InspectedBook { pdf: None, audiobook: None, metadata: BookRecord { metadata: Default::default(), description: String::new() }, toc: Vec::new() }),
                    published: PublishedStagedBook { name: name.clone(), relative_path: format!("/{name}"), published: true, fingerprint: None },
                    request_thumbnail: false,
                    restore_paths: Vec::new(),
                })
                .await
                .unwrap();
            let stale = library.state.read().await.database().local_book_info(&hash).await.unwrap();
            let destination = library.state.write().await.create_directory(&ROOT_DIR_ID, "Moved").await.unwrap().id;
            library.state.write().await.move_book_placement(&hash, &ROOT_DIR_ID, &destination).await.unwrap();
            assert!(!root.path().join(&name).exists());

            let opened = local(library, &*library.state.write().await, hash, &stale).await.unwrap().expect("the moved book remains downloaded");
            let mut content = Vec::new();
            let path = match opened {
                ResolvedBook::Pdf(mut book) => {
                    book.reader.read_to_end(&mut content).unwrap();
                    book.path
                }
                ResolvedBook::Document(document) => {
                    match document.source {
                        DocumentSource::Async(source) => {
                            source.reader().read_to_end(&mut content).await.unwrap();
                        }
                        DocumentSource::Reader(mut reader) => {
                            reader.read_to_end(&mut content).unwrap();
                        }
                    }
                    document.path
                }
                ResolvedBook::Audiobook(mut book) => {
                    book.reader.read_to_end(&mut content).unwrap();
                    book.path
                }
            };
            assert_eq!(path, format!("/Moved/{name}"));
            assert_eq!(content, bytes);
        }
    }
}
