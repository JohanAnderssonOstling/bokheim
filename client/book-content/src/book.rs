//! Parsed containers and renderer-independent reading preparation.

use crate::epub::{EpubProvider, EpubSession};
use crate::mobi::{MobiContainer, MobiResources};
use crate::resource_path::local_resource_uri;
use crate::resources::PreparedResources;
use crate::source::{ReaderFactory, Source};
use crate::{BookReader, DeclaredMetadata, DocumentInspection, TocEntry};
use futures_lite::future::block_on;
use std::future::Future;
use std::io::{self, Read, Seek};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookFormat {
    Epub,
    Mobi,
}

enum Container {
    Epub(EpubSession),
    Mobi(MobiContainer),
}

/// Opening parses the container directory and headers. Text preparation is a
/// separate, consuming step, so scans and covers never decode MOBI text.
pub struct Book {
    container: Container,
}

impl Book {
    pub async fn open<F, R>(format: BookFormat, factory: F) -> io::Result<Self>
    where
        F: Fn() -> R + Send + Sync + 'static,
        R: BookReader + 'static,
    {
        Self::open_source(format, Source::reopenable(ReaderFactory::new(factory))).await
    }

    async fn open_source(format: BookFormat, source: Source) -> io::Result<Self> {
        let container = match format {
            BookFormat::Epub => Container::Epub(EpubSession::open(source).await?),
            BookFormat::Mobi => Container::Mobi(MobiContainer::open(source).await?),
        };
        Ok(Self { container })
    }

    pub fn open_reader<R: Read + Seek + Send + 'static>(format: BookFormat, reader: R) -> io::Result<Self> {
        block_on(Self::open_source(format, Source::from_reader(reader)))
    }

    pub fn metadata(&mut self) -> io::Result<DeclaredMetadata> {
        match &mut self.container {
            Container::Epub(epub) => epub.metadata(),
            Container::Mobi(mobi) => Ok(mobi.metadata()),
        }
    }

    pub fn toc(&mut self) -> io::Result<Option<Vec<TocEntry>>> {
        block_on(self.toc_async())
    }

    pub async fn toc_async(&mut self) -> io::Result<Option<Vec<TocEntry>>> {
        match &mut self.container {
            Container::Epub(epub) => epub.toc().await,
            Container::Mobi(mobi) => mobi.toc().await,
        }
    }

    /// Package-declared cover art, or MOBI's thumbnail and cover records.
    pub fn preferred_cover_resources(&mut self) -> io::Result<Vec<String>> {
        match &mut self.container {
            Container::Epub(epub) => Ok(epub.declared_cover_image_path()?.into_iter().collect()),
            Container::Mobi(mobi) => Ok(mobi.preferred_covers()),
        }
    }

    /// Compute fallbacks only when declared artwork was unusable. EPUB may
    /// inspect spine markup; MOBI only walks the record directory.
    pub fn fallback_cover_resources(&mut self) -> io::Result<Vec<String>> {
        match &mut self.container {
            Container::Epub(epub) => epub.image_paths_in_reading_order(),
            Container::Mobi(mobi) => Ok(mobi.image_resources()),
        }
    }

    pub async fn read_resource(&mut self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        let uri = local_resource_uri("", uri)?;
        match &mut self.container {
            Container::Epub(epub) => epub.read_bytes_bounded_async(&uri, limit).await,
            Container::Mobi(mobi) => mobi.read_resource(&uri, limit).await,
        }
    }

    pub fn read_resource_sync(&mut self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        block_on(self.read_resource(uri, limit))
    }

    /// Inspect logical HTML documents from either format. EPUB keeps archive
    /// reads lazy; MOBI decodes and maps its text into virtual documents.
    pub fn inspect_documents(&mut self) -> io::Result<DocumentInspection<'_>> {
        block_on(async {
            match &mut self.container {
                Container::Epub(epub) => {
                    let documents = epub.package.document_uris()?;
                    let toc = epub.toc().await?.unwrap_or_default();
                    Ok(DocumentInspection::from_epub(epub, documents, toc))
                }
                Container::Mobi(mobi) => Ok(DocumentInspection::from_prepared(mobi.prepare_text().await?)),
            }
        })
    }

    pub async fn prepare_reading(self) -> io::Result<ReadingBook> {
        let (epub, toc) = match self.container {
            Container::Mobi(mobi) => return mobi.prepare_reading().await,
            Container::Epub(mut epub) => {
                let toc = epub.toc().await?.unwrap_or_default();
                (Arc::new(epub.into_provider().await?), toc)
            }
        };
        let documents = epub.package.document_uris()?;
        if documents.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "EPUB contains no readable documents"));
        }
        let weights = epub.package.document_progress_weights(&documents)?;
        let bytes = epub.prepare_text_resources_async(&documents).await?;
        let types = epub.package.resource_media_types();
        Ok(PreparedText { resources: PreparedResources::new(bytes, Some(types), documents), document_weights: weights, toc }.into_reading(ResourceSource::Epub(epub)))
    }
}

/// The common output of format-specific document preparation.
pub(crate) struct PreparedText {
    pub(crate) resources: PreparedResources,
    pub(crate) document_weights: Vec<u64>,
    pub(crate) toc: Vec<TocEntry>,
}

impl PreparedText {
    pub(crate) fn into_reading(self, source: ResourceSource) -> ReadingBook {
        ReadingBook { resources: self.resources, document_weights: self.document_weights, source, toc: self.toc }
    }
}

/// Both formats retain immutable containers for on-demand binary resources.
#[derive(Clone)]
pub(crate) enum ResourceSource {
    Epub(Arc<EpubProvider>),
    Mobi(Arc<MobiResources>),
}

/// Immutable documents and CSS for synchronous consumers, with asynchronous
/// access to remaining container resources. No rendering engine dependency.
pub struct ReadingBook {
    pub resources: PreparedResources,
    pub document_weights: Vec<u64>,
    source: ResourceSource,
    pub toc: Vec<TocEntry>,
}

impl ReadingBook {
    /// Language codes declared by the container; an empty list means unspecified.
    pub fn languages(&self) -> Vec<String> {
        match &self.source {
            ResourceSource::Epub(epub) => epub.languages(),
            ResourceSource::Mobi(mobi) => mobi.languages(),
        }
    }

    pub fn document_uris(&self) -> &[String] {
        self.resources.documents()
    }

    pub fn read_resource(&self, uri: String) -> impl Future<Output = io::Result<Vec<u8>>> + Send + 'static {
        let prepared = self.resources.read_bytes(&uri);
        let source = self.source.clone();
        async move {
            match prepared {
                Ok(bytes) => Ok(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let uri = local_resource_uri("", &uri)?;
                    match source {
                        ResourceSource::Epub(epub) => epub.read_bytes_async(&uri).await,
                        ResourceSource::Mobi(mobi) => mobi.read_bytes(&uri).await,
                    }
                }
                Err(error) => Err(error),
            }
        }
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[test]
    fn reading_languages_come_from_epub_package_metadata() {
        for languages in [vec![], vec!["en-GB"], vec!["sv"], vec!["fr", "en-US"]] {
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let metadata = languages.iter().map(|language| format!("<dc:language>{language}</dc:language>")).collect::<String>();
            let package = format!("<package xmlns:dc='http://purl.org/dc/elements/1.1/'><metadata>{metadata}</metadata><manifest><item id='chapter' href='chapter.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='chapter'/></spine></package>");
            for (name, text) in [
                ("META-INF/container.xml", "<container><rootfiles><rootfile full-path='content.opf'/></rootfiles></container>"),
                ("content.opf", package.as_str()),
                ("chapter.xhtml", "<html><body><p>A chapter.</p></body></html>"),
            ] {
                archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                archive.write_all(text.as_bytes()).unwrap();
            }
            let book = Book::open_reader(BookFormat::Epub, archive.finish().unwrap()).unwrap();
            let reading = block_on(book.prepare_reading()).unwrap();
            assert_eq!(reading.languages(), languages);
        }
    }
}
