//! Source ownership shared by every container format.

use futures_lite::io::{AsyncBufRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, BufReader, Cursor};
use futures_util::io::AllowStdIo;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;

/// An independent buffered cursor over a pinned book source.
pub trait BookReader: AsyncBufRead + AsyncSeek + Unpin + Send {}
impl<T: AsyncBufRead + AsyncSeek + Unpin + Send> BookReader for T {}

#[derive(Clone)]
pub(crate) struct ReaderFactory(Arc<dyn Fn() -> Box<dyn BookReader> + Send + Sync>);

impl ReaderFactory {
    pub(crate) fn new<F, R>(factory: F) -> Self
    where
        F: Fn() -> R + Send + Sync + 'static,
        R: BookReader + 'static,
    {
        Self(Arc::new(move || Box::new(factory())))
    }

    pub(crate) fn reader(&self) -> Box<dyn BookReader> {
        (self.0)()
    }
}

/// Sequential parsing owns one cursor. Reopenable sources also retain the
/// capability to create independent cursors for concurrent resource reads.
pub(crate) struct Source {
    pub(crate) reader: Box<dyn BookReader>,
    factory: Option<ReaderFactory>,
}

impl Source {
    pub(crate) fn reopenable(factory: ReaderFactory) -> Self {
        Self { reader: factory.reader(), factory: Some(factory) }
    }

    pub(crate) fn from_reader(reader: impl Read + Seek + Send + 'static) -> Self {
        Self { reader: Box::new(BufReader::new(AllowStdIo::new(reader))), factory: None }
    }

    /// Only a single, non-reopenable input needs the memory-backed fallback.
    pub(crate) async fn into_factory(mut self) -> io::Result<ReaderFactory> {
        if let Some(factory) = self.factory {
            return Ok(factory);
        }
        self.reader.seek(SeekFrom::Start(0)).await?;
        let mut bytes = Vec::new();
        self.reader.read_to_end(&mut bytes).await?;
        let bytes: Arc<[u8]> = bytes.into();
        Ok(ReaderFactory::new(move || BufReader::new(Cursor::new(bytes.clone()))))
    }
}
