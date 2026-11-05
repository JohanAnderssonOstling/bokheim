//! The reader's HTML-engine adapter for renderer-independent book content.

use html_dom as dom_values;
#[cfg(test)]
use html_resources::resources::images as image_resources;
use book_content::{Book, BookFormat as BookSourceFormat, ReadingBook};
use book_model::{AnnotationAnchor, BookTocEntry, PUBLICATION_CFI_PREFIX};
use futures_lite::future::block_on;
use html_dom::ImageSource;
use html_resources::ImageLoadCompletion;
use html::resources::ImageLoader;
use html_resources::ImageResourceContext;
use html::resources::ResourceMetadata;
use html::resources::ResourceProvider;
use html_resources::load_image_bytes_async;
use html::search::SearchIndex;
use skrift_column_paginator::cfi::PublicationCfi;
use std::future::Future;
use std::io::{self, Read, Seek};
use std::pin::Pin;
use std::sync::Arc;

pub struct BookSource {
    pub provider: Arc<dyn ResourceProvider>,
    pub cfi: PublicationCfi,
    pub publication_cfi_bases: Option<Vec<Option<String>>>,
    content: Arc<ReadingBook>,
}

struct Provider {
    content: Arc<ReadingBook>,
    language: Option<String>,
}

impl ResourceProvider for Provider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        self.content.resources.read_bytes(uri)
    }
    fn read_bytes_async<'a>(&'a self, uri: &'a str) -> Pin<Box<dyn Future<Output = io::Result<Vec<u8>>> + Send + 'a>> {
        Box::pin(self.content.read_resource(uri.to_owned()))
    }
    fn metadata(&self, uri: &str) -> io::Result<ResourceMetadata> {
        Ok(ResourceMetadata { media_type: self.content.resources.media_type(uri)?, charset: None })
    }

    fn primary_language(&self) -> Option<String> {
        self.language.clone()
    }
    fn exists(&self, uri: &str) -> bool {
        self.content.resources.exists(uri)
    }
    fn resolve(&self, base: &str, href: &str) -> String {
        self.content.resources.resolve(base, href)
    }
    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        Ok(self.content.document_uris().to_vec())
    }
}

type ImageTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

struct BookImageContext(Arc<ReadingBook>);

impl ImageResourceContext for BookImageContext {
    fn resolve(&self, base: &str, href: &str) -> String {
        self.0.resources.resolve(base, href)
    }

    fn metadata(&self, uri: &str) -> io::Result<ResourceMetadata> {
        Ok(ResourceMetadata { media_type: self.0.resources.media_type(uri)?, charset: None })
    }
}

struct BookImageLoader {
    content: Arc<ReadingBook>,
    spawn: Arc<dyn Fn(ImageTask) + Send + Sync>,
}
impl ImageLoader for BookImageLoader {
    fn load(&self, source: ImageSource, complete: ImageLoadCompletion) {
        let content = self.content.clone();
        (self.spawn)(Box::pin(async move {
            let context = BookImageContext(content.clone());
            let bytes = load_image_bytes_async(&context, &source, |uri| content.read_resource(uri)).await;
            complete(bytes);
        }));
    }
}

impl BookSource {
    pub fn new(content: ReadingBook) -> Self {
        let publication_cfi_bases = content.publication_cfi_bases();
        let cfi = PublicationCfi::new(publication_cfi_bases.clone());
        let content = Arc::new(content);
        let language = content.languages().into_iter().map(|language| language.trim().to_owned()).find(|language| !language.is_empty());
        Self { provider: Arc::new(Provider { content: content.clone(), language }), content, cfi, publication_cfi_bases }
    }

    /// Decode the stored publication address for an EPUB reading position.
    pub fn restore_cfi(&self, stored: &str) -> Option<String> {
        if !self.cfi.is_epub() {
            return Some(stored.to_owned());
        }
        let cfi = stored.strip_prefix(PUBLICATION_CFI_PREFIX)?;
        self.cfi.document_index(cfi)?;
        Some(cfi.to_owned())
    }

    pub fn store_cfi(&self, cfi: String) -> String {
        if self.cfi.is_epub() { format!("{PUBLICATION_CFI_PREFIX}{cfi}") } else { cfi }
    }

    pub fn text_search_cfi(&self, target: &book_model::BookTextTarget) -> Option<String> {
        let document = self.document_uris().iter().position(|uri| uri == &target.document)?;
        let (source, syntax) = html_parse::read_document_source(self.provider.as_ref(), &target.document).ok()?;
        let parsed = html_parse::parse_document(&source, syntax).ok()?;
        let index = html_parse::DocumentTextIndex::from_parsed(&parsed).ok()?;
        let source = index.source_position_in(target.character_offset..target.character_offset.checked_add(1)?)?;
        self.cfi.to_publication(&skrift_column_paginator::cfi::generate_cfi_from_source_position(document, &source))
    }

    pub fn annotation_anchor(&self, cfi: String) -> AnnotationAnchor {
        if self.cfi.is_epub() { AnnotationAnchor::epub_publication_cfi(cfi) } else { AnnotationAnchor::epub_cfi(cfi) }
    }

    pub fn annotation_cfi(&self, anchor: &AnnotationAnchor) -> Option<String> {
        if self.cfi.is_epub() && !anchor.is_epub_publication_cfi() {
            return None;
        }
        anchor.epub_cfi_value().map(str::to_owned)
    }
    pub fn languages(&self) -> Vec<String> {
        self.content.languages()
    }
    pub fn document_uris(&self) -> &[String] {
        self.content.document_uris()
    }
    pub fn toc(&self) -> &[BookTocEntry] {
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
        Arc::new(BookImageLoader { content: self.content.clone(), spawn: Arc::new(spawn) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use html::resources::{ImageService, InlineJobSpawner};
    use html_resources::ImagePipeline;
    use html_pipeline::{DocumentFactory, ResourceContext};
    use std::collections::HashSet;
    use std::io::{Cursor, Write};
    use std::time::{Duration, Instant};

    #[test]
    fn epub_primary_language_uses_metadata_order_for_multilingual_books() {
        for languages in [vec![], vec!["sv"], vec!["fr", "en-US"], vec!["en-US", "fr"]] {
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let metadata = languages.iter().map(|language| format!("<dc:language>{language}</dc:language>")).collect::<String>();
            let package = format!(
                "<package xmlns:dc='http://purl.org/dc/elements/1.1/'><metadata>{metadata}</metadata><manifest><item id='chapter' href='chapter.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='chapter'/></spine></package>"
            );
            for (name, source) in [("META-INF/container.xml", "<container><rootfiles><rootfile full-path='book.opf'/></rootfiles></container>"), ("book.opf", package.as_str()), ("chapter.xhtml", "<html lang='sv'><body>Text</body></html>")]
            {
                archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                archive.write_all(source.as_bytes()).unwrap();
            }
            let book = BookSource::from_reader(BookSourceFormat::Epub, archive.finish().unwrap()).unwrap();
            assert_eq!(book.provider.primary_language().as_deref(), languages.first().copied());
            assert!(book.provider.read_string("chapter.xhtml").unwrap().contains("lang='sv'"), "publication fallback must not rewrite explicit content language");
        }
    }

    #[test]
    fn epub_publication_fonts_load_on_demand_without_eager_text_reads() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
            (
                "OEBPS/content.opf",
                r#"<package><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="font" href="book.ttf" media-type="font/ttf"/></manifest><spine><itemref idref="chapter"/></spine></package>"#,
            ),
            ("OEBPS/chapter.xhtml", "<html><body>Mixed Latin and CJK</body></html>"),
            ("OEBPS/book.ttf", "deferred publication font bytes"),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes.as_bytes()).unwrap();
        }
        let book = BookSource::from_reader(BookSourceFormat::Epub, archive.finish().unwrap()).unwrap();
        assert_eq!(book.provider.read_bytes("OEBPS/book.ttf").unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(block_on(book.provider.read_bytes_async("OEBPS/book.ttf")).unwrap(), b"deferred publication font bytes");
        assert_eq!(book.provider.read_bytes("OEBPS/book.ttf").unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn epub_adapter_resolves_inline_svg_image_resources() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", stored).unwrap();
        archive.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#).unwrap();
        archive.start_file("OEBPS/content.opf", stored).unwrap();
        archive
            .write_all(br#"<package><manifest><item id="cover" href="Text/titlepage.xhtml" media-type="application/xhtml+xml"/><item id="image" href="Images/cover.jpeg" media-type="image/jpeg"/></manifest><spine><itemref idref="cover"/></spine></package>"#)
            .unwrap();
        archive.start_file("OEBPS/Text/titlepage.xhtml", stored).unwrap();
        archive
            .write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><image width="1" height="1" xlink:href="../Images/cover.jpeg"/></svg></body></html>"#)
            .unwrap();
        archive.start_file("OEBPS/Images/cover.jpeg", stored).unwrap();
        archive.write_all(&[0xff, 0xd8, 0xff, 0xd9]).unwrap();

        let book = BookSource::from_reader(BookSourceFormat::Epub, archive.finish().unwrap()).unwrap();
        let provider = book.provider.clone();
        let document_uri = "OEBPS/Text/titlepage.xhtml";
        assert_eq!(provider.metadata(document_uri).unwrap().media_type.as_deref(), Some("application/xhtml+xml"));
        let source = provider.read_string(document_uri).unwrap();
        assert!(source.contains(r#"xlink:href="../Images/cover.jpeg""#), "adapter must return unmodified book bytes");

        let mut factory = DocumentFactory::new();
        let document = factory.with_resources(ResourceContext::new(provider.as_ref(), document_uri)).parse_to_dom(&source);
        let (image_idx, image) = document.images().iter().enumerate().find(|(_, image)| matches!(&image.source, html_dom::ImageSource::InlineSvg { .. })).expect("inline SVG image resource");
        assert!(matches!(&image.source, html_dom::ImageSource::InlineSvg { base_uri, .. } if base_uri == document_uri));

        let loader = book.image_loader(|task| {
            std::thread::spawn(move || block_on(task));
        });
        let service = ImageService::with_loader(loader, Arc::new(InlineJobSpawner), ImageService::DEFAULT_BYTE_BUDGET);
        let mut pipeline = ImagePipeline::with_service(service);
        let image = dom_values::image_id_new(image_idx as u32);
        pipeline.ensure_window(document.images(), &HashSet::from([image]));
        let deadline = Instant::now() + Duration::from_secs(2);
        while image_resources::decoded_images_get(pipeline.decoded_images(), image).is_none() && Instant::now() < deadline {
            pipeline.poll(document.images());
            std::thread::yield_now();
        }
        let html_resources::DecodedImage::Svg { bytes, .. } = image_resources::decoded_images_get(pipeline.decoded_images(), image).expect("decoded inline SVG") else { panic!("inline SVG must remain a vector resource") };
        let svg = std::str::from_utf8(bytes).expect("hydrated SVG remains UTF-8");
        assert!(svg.contains(r#"xlink:href="data:image/jpeg;base64,/9j/2Q==""#));
    }

    #[test]
    fn epub_adapter_exposes_xhtml_manifest_type_for_html_named_chapter() {
        use html_pipeline::MarkupSyntax;
        use html_pipeline::markup_syntax_for_resource;
        use html_pipeline::parse_document;

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
        let syntax = markup_syntax_for_resource(book.provider.as_ref(), uri);
        assert_eq!(syntax, MarkupSyntax::Xhtml);
        parse_document(&book.provider.read_string(uri).unwrap(), syntax).unwrap();
    }

    #[test]
    fn epub_adapter_recovers_html_markup_declared_as_xhtml() {
        use html_pipeline::MarkupSyntax;
        use html_pipeline::markup_syntax_for_resource;
        use html_pipeline::parse_document;

        let cover = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Romeo y Julieta</title><link rel="stylesheet" href="style.css"></head><body><img src="cover.png"><p>Romeo<br>Julieta &amp; friends</p></body></html>"#;
        for name in ["cover.html", "cover.xhtml"] {
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let package = format!("<package><manifest><item id='cover' href='{name}' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='cover'/></spine></package>");
            for (uri, source) in [("META-INF/container.xml", "<container><rootfiles><rootfile full-path='book.opf'/></rootfiles></container>"), ("book.opf", package.as_str()), (name, cover)] {
                archive.start_file(uri, zip::write::SimpleFileOptions::default()).unwrap();
                archive.write_all(source.as_bytes()).unwrap();
            }
            let book = BookSource::from_reader(BookSourceFormat::Epub, archive.finish().unwrap()).unwrap();
            let provider = book.provider.as_ref();
            // Rendering and search use the same recovery while retaining the manifest type.
            for _ in 0..2 {
                assert_eq!(provider.metadata(name).unwrap().media_type.as_deref(), Some("application/xhtml+xml"));
                let syntax = markup_syntax_for_resource(provider, name);
                assert_eq!(syntax, MarkupSyntax::Xhtml);
                let parsed = parse_document(cover, syntax).unwrap();
                assert_eq!(parsed.title().as_deref(), Some("Romeo y Julieta"));
                assert_eq!(parsed.into_dom_checked().unwrap().images().len(), 1);
                let text = html::search::SearchIndex::load(provider, name).unwrap();
                assert!(text.text().contains("Julieta & friends"));
                assert_eq!(provider.read_string(name).unwrap(), cover, "recovery must preserve publication bytes");
            }
        }
    }
}
