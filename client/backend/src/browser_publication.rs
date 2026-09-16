//! Publication capabilities pin local generations for the reader subscription.
use crate::{
    BackendError, ContentHash, LibraryId,
    library::LibrarySession,
    runtime::{PublicationSource, ResolvedBookData},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
};
thread_local! {
    static SOURCES: RefCell<HashMap<String, (LibraryId, crate::platform::web_storage::FileReader)>> = RefCell::new(HashMap::new());
}
pub(crate) enum PublicationLease {
    Local(String),
    Audiobook(crate::library::browser_audiobook::PlaybackLease),
    None,
}
impl Drop for PublicationLease {
    fn drop(&mut self) {
        if let Self::Local(id) = self {
            SOURCES.with(|sources| sources.borrow_mut().remove(id));
        }
    }
}
pub(crate) async fn prepare(library: &LibrarySession, hash: ContentHash) -> Result<(ResolvedBookData, PublicationLease), BackendError> {
    library.prepare_book_for_reader(hash).await
}
pub(crate) fn register(library: LibraryId, mut reader: crate::platform::web_storage::FileReader) -> Result<(PublicationSource, PublicationLease), String> {
    let length = reader.seek(SeekFrom::End(0)).map_err(|error| error.to_string())?;
    reader.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    SOURCES.with(|sources| sources.borrow_mut().insert(id.clone(), (library, reader)));
    Ok((PublicationSource::Local { capability: id.clone(), length }, PublicationLease::Local(id)))
}

pub(crate) fn read_range(library: LibraryId, id: &str, offset: u64, length: usize) -> Result<Vec<u8>, String> {
    if length > 256 * 1024 {
        return Err("publication range exceeds limit".into());
    }
    SOURCES.with(|sources| {
        let mut sources = sources.borrow_mut();
        let (owner, reader) = sources.get_mut(id).ok_or("publication source closed; reopen the book")?;
        if *owner != library {
            return Err("publication belongs to another library".into());
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

#[cfg(feature = "storage-contract-tests")]
pub(crate) async fn contract() -> Result<(), String> {
    use crate::platform::web_storage;
    let library = LibraryId::from_u128(123);
    let name = "assets/source-contract/book/one";
    web_storage::write_file(name, &vec![42; 2 * 1024 * 1024]).await?;
    let reader = web_storage::open_reader(name).await?.ok_or("fixture source absent")?;
    let (source, lease) = register(library, reader)?;
    assert!(crate::runtime::encode_worker_message(&source)?.len() < 256, "source must not contain the publication bytes");
    let PublicationSource::Local { capability, length } = source else { unreachable!() };
    assert_eq!(length, 2 * 1024 * 1024);
    web_storage::write_file(name, b"replacement").await?;
    assert_eq!(read_range(library, &capability, length - 3, 3)?, vec![42; 3]);
    assert!(read_range(LibraryId::from_u128(124), &capability, 0, 3).is_err());
    assert!(read_range(library, &capability, 0, 1024 * 1024).is_err());
    drop(lease);
    assert!(read_range(library, &capability, 0, 3).is_err());
    assert_eq!(web_storage::read_file(name).await?, Some(b"replacement".to_vec()));
    web_storage::remove_file(name)?;
    Ok(())
}
