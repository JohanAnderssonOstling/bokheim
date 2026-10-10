//! The reader's HTML-engine adapter for renderer-independent book content.

use book_content::{Book, BookFormat as BookSourceFormat, ReadingBook};
use futures_lite::future::block_on;
use html::document::ImageSource;
use html::resources::{ImageLoadCompletion, ImageLoader, ResourceMetadata, ResourceProvider, load_image_bytes_async};
use std::future::Future;
use std::io::{self, Read, Seek};
use std::pin::Pin;
use std::sync::Arc;

pub struct BookSource {
    pub provider: Arc<dyn ResourceProvider>,
    content: Arc<ReadingBook>,
}

struct Provider(Arc<ReadingBook>);

impl ResourceProvider for Provider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        self.0.resources.read_bytes(uri)
    }
    fn metadata(&self, uri: &str) -> io::Result<ResourceMetadata> {
        Ok(ResourceMetadata { media_type: self.0.resources.media_type(uri)?, charset: None })
    }
    fn exists(&self, uri: &str) -> bool {
        self.0.resources.exists(uri)
    }
    fn resolve(&self, base: &str, href: &str) -> String {
        self.0.resources.resolve(base, href)
    }
    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        Ok(self.0.document_uris().to_vec())
    }
}

type ImageTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
struct BookImageLoader {
    content: Arc<ReadingBook>,
    provider: Arc<dyn ResourceProvider>,
    spawn: Arc<dyn Fn(ImageTask) + Send + Sync>,
}
impl ImageLoader for BookImageLoader {
    fn load(&self, source: ImageSource, complete: ImageLoadCompletion) {
        let content = self.content.clone();
        let provider = self.provider.clone();
        (self.spawn)(Box::pin(async move {
            let bytes = load_image_bytes_async(provider.as_ref(), &source, |uri| content.read_resource(uri)).await;
            complete(bytes);
        }));
    }
}

impl BookSource {
    pub fn new(content: ReadingBook) -> Self {
        let content = Arc::new(content);
        Self { provider: Arc::new(Provider(content.clone())), content }
    }
    pub fn document_uris(&self) -> &[String] {
        self.content.document_uris()
    }
    pub fn toc(&self) -> &[book_content::TocEntry] {
        &self.content.toc
    }
    pub fn document_weights(&self) -> &[u64] {
        &self.content.document_weights
    }
    pub fn from_reader<R: Read + Seek + Send + 'static>(format: BookSourceFormat, reader: R) -> io::Result<Self> {
        let book = Book::open_reader(format, reader)?;
        block_on(book.prepare_reading()).map(Self::new)
    }
    pub fn image_loader(&self, spawn: impl Fn(ImageTask) + Send + Sync + 'static) -> Arc<dyn ImageLoader> {
        Arc::new(BookImageLoader { content: self.content.clone(), provider: self.provider.clone(), spawn: Arc::new(spawn) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use html::resources::{ImagePipeline, ImageService, InlineJobSpawner};
    use std::collections::HashSet;
    use std::io::{Cursor, Write};
    use std::time::{Duration, Instant};

    #[test]
    fn epub_adapter_resolves_inline_svg_image_resources() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", stored).unwrap();
        archive.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#).unwrap();
        archive.start_file("OEBPS/content.opf", stored).unwrap();
        archive.write_all(br#"<package><manifest><item id="cover" href="Text/titlepage.xhtml" media-type="application/xhtml+xml"/><item id="image" href="Images/cover.jpeg" media-type="image/jpeg"/></manifest><spine><itemref idref="cover"/></spine></package>"#).unwrap();
        archive.start_file("OEBPS/Text/titlepage.xhtml", stored).unwrap();
        archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><image width="1" height="1" xlink:href="../Images/cover.jpeg"/></svg></body></html>"#).unwrap();
        archive.start_file("OEBPS/Images/cover.jpeg", stored).unwrap();
        archive.write_all(&[0xff, 0xd8, 0xff, 0xd9]).unwrap();

        let book = BookSource::from_reader(BookSourceFormat::Epub, archive.finish().unwrap()).unwrap();
        let provider = book.provider.clone();
        let document_uri = "OEBPS/Text/titlepage.xhtml";
        assert_eq!(provider.metadata(document_uri).unwrap().media_type.as_deref(), Some("application/xhtml+xml"));
        let source = provider.read_string(document_uri).unwrap();
        assert!(source.contains(r#"xlink:href="../Images/cover.jpeg""#), "adapter must return unmodified book bytes");

        let mut factory = html::pipeline::DocumentFactory::new();
        factory.set_resource_context(provider.clone(), document_uri);
        let document = factory.parse_to_dom(&source);
        let (image_idx, image) = document.images().iter().enumerate().find(|(_, image)| matches!(&image.source, html::document::ImageSource::InlineSvg { .. })).expect("inline SVG image resource");
        assert!(matches!(&image.source, html::document::ImageSource::InlineSvg { base_uri, .. } if base_uri == document_uri));

        let loader = book.image_loader(|task| {
            std::thread::spawn(move || block_on(task));
        });
        let service = ImageService::with_loader(loader, Arc::new(InlineJobSpawner), ImageService::DEFAULT_BYTE_BUDGET);
        let mut pipeline = ImagePipeline::with_service(Arc::new(document.images().to_vec()), service);
        pipeline.ensure_window(&HashSet::from([image_idx as u32]));
        let deadline = Instant::now() + Duration::from_secs(2);
        while pipeline.get_decoded(image_idx as u32).is_none() && Instant::now() < deadline {
            pipeline.poll();
            std::thread::yield_now();
        }
        let html::resources::DecodedImage::Svg { bytes, .. } = pipeline.get_decoded(image_idx as u32).expect("decoded inline SVG") else { panic!("inline SVG must remain a vector resource") };
        let svg = std::str::from_utf8(bytes).expect("hydrated SVG remains UTF-8");
        assert!(svg.contains(r#"xlink:href="data:image/jpeg;base64,/9j/2Q==""#));
    }

    #[test]
    fn epub_adapter_exposes_xhtml_manifest_type_for_html_named_chapter() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
            ("OEBPS/content.opf", r#"<package><manifest><item id="chapter" href="chapter.html" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#),
            ("OEBPS/chapter.html", r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p><a id="page1"/>Text</p></body></html>"#),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes.as_bytes()).unwrap();
        }
        let book = BookSource::from_reader(BookSourceFormat::Epub, archive.finish().unwrap()).unwrap();
        let uri = &book.document_uris()[0];
        assert_eq!(uri, "OEBPS/chapter.html");
        assert_eq!(book.provider.clone().metadata(uri).unwrap().media_type.as_deref(), Some("application/xhtml+xml"));
    }
}
