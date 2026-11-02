//! Native book readers, filesystem availability, and playback resources.

use super::*;
use crate::DownloadState;
use crate::library::DocumentBook;
use crate::library::ResolvedBook;
use book_access::{FetchFuture, RemoteFileError};
use library_files::AssetStoreError;
use library_model::RelativeBookPath;
pub(crate) type Opened = ResolvedBook;

pub(super) async fn local(library: &LibrarySession, hash: BookId, info: &LocalBookInfo) -> Result<Option<Opened>, BackendError> {
    let mut profile = StartupProfile::new("book_lookup_local");
    let state = library.persistent_download_state(hash)?;
    profile.mark("download_state");
    if !matches!(state, DownloadState::Downloaded) {
        return Ok(None);
    }
    if info.format == BookFormat::Mp3Folder {
        return library.prepare_audiobook_track(hash, 0).await.map(Some);
    }
    let mut path = info.path.clone().ok_or_else(|| BackendError::message("book placement not found"))?;
    let format = info.format;
    if !matches!(format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b | BookFormat::Fb2 | BookFormat::Cbz) {
        return Err(format!("the {} format is no longer supported", format.canonical_extension()).into());
    }
    if matches!(format, BookFormat::Epub | BookFormat::Mobi | BookFormat::Fb2 | BookFormat::Cbz) {
        profile.mark("format_and_path");
        let source = open_with_replay(library, hash, &mut path, async |path| library.assets.open_async_book_at_path(path).await).await?;
        profile.mark("open_async_source");
        let source = source.ok_or_else(|| "book content is not downloaded")?;
        return Ok(Some(ResolvedBook::Document(DocumentBook::from_async_source(path.to_string(), format, source))));
    }
    if format == BookFormat::Pdf {
        return Ok(Some(ResolvedBook::Document(local_pdf(library, hash, &mut path).await?)));
    }
    #[cfg(target_os = "android")]
    let playback_file = if format == BookFormat::M4b { library.assets.open_playback_file(&path)? } else { None };
    #[cfg(target_os = "android")]
    let reader = if let Some(file) = &playback_file { Some(Box::new(file.try_clone()?) as crate::BoxedBookReader) } else { library.assets.open_book_at_path(path.clone()).await? };
    #[cfg(not(target_os = "android"))]
    let reader = open_with_replay(library, hash, &mut path, async |path| library.assets.open_book_at_path(path).await).await?;
    let reader = reader.ok_or_else(|| "book content is not downloaded")?;
    let book = if format == BookFormat::M4b {
        ResolvedBook::Audiobook(crate::library::AudiobookBook {
            format,
            path: path.to_string(),
            reader,
            stream_control: None,
            #[cfg(target_os = "android")]
            playback_file,
        })
    } else {
        ResolvedBook::Document(DocumentBook::new(path.to_string(), format, reader))
    };
    Ok(Some(book))
}

// Pin a file revision and its matching metadata while publication is excluded.
// The returned reader retains its file handle, not the book lease: exports can
// atomically replace the library file while this reader keeps the old revision.
async fn local_pdf(library: &LibrarySession, hash: BookId, path: &mut RelativeBookPath) -> Result<DocumentBook, BackendError> {
    let opened = open_with_replay(library, hash, path, async |path| {
        // Acquire for each attempt so a missing path releases the lease before
        // replaying placement work, which may itself need exclusive access.
        let _opening = library.assets.lease_book(&hash).await?;
        let Some(reader) = library.assets.open_book_at_path(path.clone()).await? else { return Ok(None) };
        let metadata = library.db.local_pdf_metadata(&hash, path.as_str()).map_err(AssetStoreError::operation)?;
        Ok(Some((reader, metadata)))
    })
    .await?;
    let (reader, metadata) = opened.ok_or("book content is not downloaded")?;
    let mut document = DocumentBook::new(path.to_string(), BookFormat::Pdf, reader);
    document.pdf_metadata = metadata;
    Ok(document)
}

// A placement move can land between reading `info.path` and opening it.
// Drain this hash's pending work and retry once against the fresh path.
async fn open_with_replay<T>(library: &LibrarySession, content_hash: BookId, path: &mut RelativeBookPath, mut open: impl AsyncFnMut(RelativeBookPath) -> Result<Option<T>, AssetStoreError>) -> Result<Option<T>, BackendError> {
    if let Some(reader) = open(path.clone()).await? {
        return Ok(Some(reader));
    }
    let _ = library_placement::run_file_jobs_for(&library.assets, &library.db, &content_hash).await;
    if let Some(fresh) = library.db.local_book_info(&content_hash)?.path {
        *path = fresh;
        return Ok(open(path.clone()).await?);
    }
    Ok(None)
}

pub(super) async fn remote(library: &LibrarySession, hash: BookId, info: &LocalBookInfo) -> Result<Opened, BackendError> {
    if info.format == BookFormat::Mp3Folder {
        return library.prepare_audiobook_track(hash, 0).await;
    }
    let path = info.path.as_ref().ok_or_else(|| BackendError::message("book placement not found"))?;
    if matches!(info.format, BookFormat::Epub | BookFormat::Cbz) {
        return super::remote_archive::open(library, hash, path.to_string(), info.format).await;
    }
    let RemoteDocument { source, access, prefix, pdf_metadata } = remote_document(library, hash).await?;
    let length = source.length;
    if info.format == BookFormat::Pdf {
        let fetch = book_access::remote_file::native_bundle_fetch(access.batch_fetch()).map_err(BackendError::operation)?;
        let reader = book_access::pdf_bundle::open(length, prefix, fetch).map_err(BackendError::operation)?;
        let mut book = DocumentBook::new(path.to_string(), info.format, reader);
        book.pdf_metadata = pdf_metadata;
        return Ok(ResolvedBook::Document(book));
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
    if matches!(info.format, BookFormat::Epub | BookFormat::Mobi | BookFormat::Fb2 | BookFormat::Cbz) {
        let source = access.async_source(length, prefix)?;
        return Ok(ResolvedBook::Document(DocumentBook::from_async_source(path.to_string(), info.format, source)));
    }
    let reader = book_access::remote_file::native_reader(length, prefix, access.fetch()).map_err(BackendError::operation)?;
    let mut document = DocumentBook::new(path.to_string(), info.format, reader);
    document.pdf_metadata = pdf_metadata;
    Ok(ResolvedBook::Document(document))
}

impl LibrarySession {
    pub(crate) async fn prepare_audiobook_track(&self, hash: BookId, index: usize) -> Result<ResolvedBook, BackendError> {
        let info = self.db.local_book_info(&hash)?;
        if info.format != BookFormat::Mp3Folder {
            return Err(BackendError::message("book is not an MP3 folder"));
        }
        let track = self.db.audiobook_tracks(&hash)?.into_iter().nth(index).ok_or_else(|| BackendError::message("audiobook track is unavailable"))?;
        if !track.valid() {
            return Err(BackendError::message("audiobook track index is invalid"));
        }
        let mut path = info.path.ok_or_else(|| BackendError::message("audiobook placement is unavailable"))?;
        if matches!(self.persistent_download_state(hash)?, DownloadState::Downloaded) {
            let reader = open_with_replay(self, hash, &mut path, async |path| self.assets.open_audiobook_track(path, track.name.clone()).await).await?;
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
        let source = self.describe_remote_file(hash).await.map_err(BackendError::operation)?.0;
        if track.archive_hash != Some(source.hash) {
            return Err(BackendError::message("audiobook track index does not match the remote archive; sync the book metadata and retry"));
        }
        if track.offset.checked_add(track.length).is_none_or(|end| end > source.length) {
            return Err(BackendError::message("audiobook track exceeds remote archive"));
        }
        let access = self.remote_book_access(&source)?;
        let prefix = access.read(track.offset, track.length.min(64 * 1024) as usize).await.map_err(BackendError::operation)?;
        let base = track.offset;
        let length = track.length;
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
    use crate::library::{DocumentSource, handle::concurrency_tests::fixture};
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
            let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
            let library = &actor.session;
            let bytes = b"local book bytes";
            let hash = BookId::from(blake3::hash(bytes));
            let name = format!("Book.{}", format.canonical_extension());
            std::fs::write(root.path().join(&name), bytes).unwrap();
            library
                .db
                .commit_import(ImportCommit {
                    source_identity: None,
                    byte_checksum: None,
                    parent_id: ROOT_DIR_ID,
                    file_name: name.clone(),
                    format,
                    content_hash: hash,
                    size_bytes: bytes.len() as u64,
                    inspection_version: 1,
                    inspection: Some(InspectedBook { pdf: None, audiobook: None, metadata: BookRecord { metadata: Default::default(), description: String::new() }, toc: Vec::new() }),
                    published: PublishedStagedBook { name: name.clone(), relative_path: format!("/{name}"), published: true, fingerprint: None },
                    request_thumbnail: false,
                    restore_paths: Vec::new(),
                })
                .unwrap();
            let stale = library.db.local_book_info(&hash).unwrap();
            let destination = library.placement.create_directory(&ROOT_DIR_ID, "Moved").unwrap().id;
            library.placement.move_book_placement(&hash, &ROOT_DIR_ID, &destination).await.unwrap();
            assert!(!root.path().join(&name).exists());

            let opened = local(library, hash, &stale).await.unwrap().expect("the moved book remains downloaded");
            let mut content = Vec::new();
            let path = match opened {
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
