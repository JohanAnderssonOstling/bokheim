use crate::BackendError;
pub(crate) use crate::library::LibrarySession;
pub(crate) use crate::library::book::browser;
pub(crate) use crate::library::browser_audiobook;

pub(crate) async fn open_remote_audio(library: &LibrarySession, content_hash: crate::ContentHash) -> Result<Option<crate::library::PlaybackSource>, BackendError> {
    browser_audiobook::open_remote_audio(library, content_hash).await
}

pub(crate) fn read_book_range(library: &LibrarySession, capability: &str, offset: u64, length: usize) -> Result<serde_bytes::ByteBuf, BackendError> {
    browser::SOURCES
        .with(|sources| {
            let sources = sources.borrow();
            let (owner, _) = sources.get(capability).ok_or("book source closed; reopen the book")?;
            if owner != library.id() {
                return Err("book belongs to another library".into());
            }
            client_platform_web::range_reader::read_range(capability, offset, length)
        })
        .map(serde_bytes::ByteBuf::from)
        .map_err(BackendError::operation)
}
