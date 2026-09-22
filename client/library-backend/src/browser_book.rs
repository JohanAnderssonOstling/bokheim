//! Book capabilities pin local generations for the reader subscription.
use crate::{
    book::{BookSource, ResolvedBookData},
    library::LibrarySession,
    BackendError, ContentHash, LibraryId,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
};
thread_local! {
    static SOURCES: RefCell<HashMap<String, (LibraryId, client_platform_web::web_storage::FileReader)>> = RefCell::new(HashMap::new());
}
pub(crate) enum BookLease {
    Local(String),
    Audiobook(crate::library::browser_audiobook::PlaybackLease),
    None,
}
impl Drop for BookLease {
    fn drop(&mut self) {
        if let Self::Local(id) = self {
            SOURCES.with(|sources| sources.borrow_mut().remove(id));
        }
    }
}
pub(crate) async fn prepare(library: &LibrarySession, hash: ContentHash) -> Result<(ResolvedBookData, BookLease), BackendError> {
    library.prepare_book_for_reader(hash).await
}
pub(crate) fn register(library: LibraryId, mut reader: client_platform_web::web_storage::FileReader) -> Result<(BookSource, BookLease), String> {
    let length = reader.seek(SeekFrom::End(0)).map_err(|error| error.to_string())?;
    reader.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    SOURCES.with(|sources| sources.borrow_mut().insert(id.clone(), (library, reader)));
    Ok((BookSource::Local { capability: id.clone(), length }, BookLease::Local(id)))
}

pub fn read_range(library: LibraryId, id: &str, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    if length > 256 * 1024 {
        return Err("book range exceeds limit".into());
    }
    SOURCES.with(|sources| {
        let mut sources = sources.borrow_mut();
        let (owner, reader) = sources.get_mut(id).ok_or("book source closed; reopen the book")?;
        if *owner != library {
            return Err("book belongs to another library".into());
        }
        reader.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let mut bytes = vec![0; length];
        let mut count = 0;
        while count < length {
            let n = reader.read(&mut bytes[count..]).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            count += n;
        }
        bytes.truncate(count);
        Ok(bytes)
    })
}
