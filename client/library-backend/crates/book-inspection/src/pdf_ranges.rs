pub(super) use pdf_range_reader::{document_root, metadata};

#[cfg(test)]
mod tests {
    use lopdf::dictionary;
    use lopdf::{Dictionary, Document, Object, Stream};
    use pdf_range_reader::metadata;
    use std::io::{Cursor, Result};
    use std::io::{Read, Seek, SeekFrom};
    use std::path::Path;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[test]
    fn metadata_inspection_skips_page_streams_and_closing_copyright() {
        use lopdf::content::{Content, Operation};
        let mut document = Document::with_version("1.7");
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let mut kids = Vec::new();
        let mut streams = Vec::new();
        for index in 0..50 {
            let lines: &[&str] =
                if index == 49 { &["Copyright Ada Author.", "Library of Congress Cataloging-in-Book Data.", "Science History.", "ISBN 978-0-393-24327-7 (pdf).", "Classification: LCC Q125 .A12 2023"] } else { &["Ordinary chapter"] };
            // Keep every asserted value inside the visible page. One long
            // line at (0, 0) placed the classification beyond the media box.
            let mut operations = vec![Operation::new("BT", vec![]), Operation::new("Tf", vec!["F1".into(), 12.into()]), Operation::new("Td", vec![40.into(), 740.into()])];
            for line in lines {
                operations.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
                operations.push(Operation::new("Td", vec![0.into(), (-18).into()]));
            }
            operations.push(Operation::new("ET", vec![]));
            let content = Content { operations };
            let stream = document.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            streams.push(stream);
            let page = document
                .add_object(dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => stream, "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }, "MediaBox" => vec![0.into(),0.into(),600.into(),800.into()] });
            kids.push(page.into());
        }
        document.objects.insert(pages, dictionary! { "Type" => "Pages", "Count" => 50, "Kids" => kids }.into());
        let root = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        let info = document.add_object(dictionary! { "Title" => Object::string_literal("Science History"), "Author" => Object::string_literal("Ada Author") });
        document.trailer.set("Root", root);
        document.trailer.set("Info", info);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let loaded = metadata(Cursor::new(bytes.clone())).unwrap();
        assert!(!loaded.objects.contains_key(&streams[25]));
        assert!(!loaded.objects.contains_key(&streams[49]));
        let inspected = crate::inspect_pdf_reader(Path::new("book.pdf"), Cursor::new(bytes)).unwrap();
        {
            assert!(inspected.metadata.book.subjects.is_empty());
            assert!(inspected.metadata.book.identifiers.is_empty());
        }
    }
    struct Counted<R> {
        inner: R,
        read: Arc<AtomicUsize>,
    }
    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, bytes: &mut [u8]) -> Result<usize> {
            let count = self.inner.read(bytes)?;
            self.read.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }
    impl<R: Seek> Seek for Counted<R> {
        fn seek(&mut self, position: SeekFrom) -> Result<u64> {
            self.inner.seek(position)
        }
    }

    #[test]
    fn pdf_inspection_keeps_metadata_without_reading_an_unrelated_large_stream_for_hashing() {
        use book_model::BookFormat;
        use content_address::BookId;
        let mut document = Document::with_version("1.7");
        let pages = document.new_object_id();
        // A large unreferenced stream represents data that metadata extraction
        // has no reason to request from a range reader.
        document.add_object(Stream::new(dictionary! { "Type" => "EmbeddedFile" }, vec![b'x'; 16 * 1024 * 1024]));
        let text = document.add_object(Stream::new(Dictionary::new(), b"BT /F1 12 Tf (ISBN 978-0-8213-3827-8) Tj ET".to_vec()));
        let font = document.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => text,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            "MediaBox" => vec![0.into(),0.into(),600.into(),800.into()]
        });
        document.objects.insert(pages, dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => vec![page.into()] }.into());
        let root = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        let info = document.add_object(dictionary! { "Title" => Object::string_literal("Remote PDF") });
        document.trailer.set("Root", root);
        document.trailer.set("Info", info);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let byte_hash = BookId::from(blake3::hash(&bytes));
        let identity = BookId::for_local_content(&[9; 16], blake3::hash(b"separate book").into());
        let read = Arc::new(AtomicUsize::new(0));
        let reader = Counted { inner: Cursor::new(bytes), read: read.clone() };
        let inspected = crate::inspect_book_reader_with_hash(Path::new("Book.pdf"), BookFormat::Pdf, reader, Some(identity)).unwrap();
        let pdf = inspected.pdf.unwrap();
        assert!(pdf.valid_for(&identity.to_hex()));
        assert!(!pdf.valid_for(&byte_hash.to_hex()));
        assert_eq!(pdf.pages.len(), 1);
        assert_eq!(inspected.metadata.title, "Remote PDF");
        assert!(inspected.metadata.book.identifiers.iter().any(|id| id.canonical_value().as_deref() == Some("9780821338278")));
        assert!(read.load(Ordering::Relaxed) < 1024 * 1024, "PDF inspection requested {} bytes", read.load(Ordering::Relaxed));
    }

    #[test]
    fn indirect_page_children_and_invalid_optional_info_remain_readable() {
        for invalid_info in [false, true] {
            let mut document = Document::with_version("1.7");
            let pages = document.new_object_id();
            let page = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),600.into(),800.into()] });
            let kids = document.add_object(Object::Array(vec![page.into()]));
            document.objects.insert(pages, dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => kids }.into());
            let root = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
            document.trailer.set("Root", root);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            if invalid_info {
                let pointer = bytes.windows(9).rposition(|part| part == b"startxref").unwrap();
                let previous = std::str::from_utf8(&bytes[pointer + 9..]).unwrap().split_whitespace().next().unwrap();
                let offset = bytes.len();
                let info = "10 0 obj\n<< /Invalid Name (Bad producer metadata) >>\nendobj\n";
                let xref = offset + info.len();
                let update = format!("{info}xref\n10 1\n{offset:010} 00000 n\ntrailer\n<< /Size 11 /Info 10 0 R /Prev {previous} >>\nstartxref\n{xref}\n%%EOF\n");
                bytes.extend_from_slice(update.as_bytes());
            }
            let inspected = crate::inspect_pdf_reader(Path::new("readable.pdf"), Cursor::new(bytes.clone())).unwrap();
            assert_eq!(inspected.metadata.title, "readable");
            let loaded = metadata(Cursor::new(bytes)).unwrap();
            assert!(loaded.get_pages().is_empty());
        }
    }

    #[test]
    fn incremental_metadata_uses_latest_revision_without_loading_the_page_tree() {
        let mut document = Document::with_version("1.7");
        let pages = document.new_object_id();
        let page = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),600.into(),800.into()] });
        document.objects.insert(pages, dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => vec![page.into()] }.into());
        let root = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", root);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let pointer = bytes.windows(9).rposition(|part| part == b"startxref").unwrap();
        let previous = std::str::from_utf8(&bytes[pointer + 9..]).unwrap().split_whitespace().next().unwrap();
        let object = bytes.len();
        let update = format!("10 0 obj\n<< /Title (Latest title) >>\nendobj\n");
        let xref = object + update.len();
        let trailer = format!("xref\n10 1\n{object:010} 00000 n\ntrailer\n<< /Size 11 /Info 10 0 R /Prev {previous} >>\nstartxref\n{xref}\n%%EOF\n");
        bytes.extend_from_slice(update.as_bytes());
        bytes.extend_from_slice(trailer.as_bytes());
        let loaded = metadata(Cursor::new(bytes)).unwrap();
        assert!(loaded.get_pages().is_empty());
        assert_eq!(loaded.get_object((10, 0)).unwrap().as_dict().unwrap().get(b"Title").unwrap().as_str().unwrap(), b"Latest title");
    }

    #[test]
    fn metadata_skips_large_images_with_classic_and_compressed_references() {
        for modern in [false, true] {
            let mut document = Document::with_version("1.7");
            let pages = document.new_object_id();
            let image = document.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 4000, "Height" => 4000, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8 }, vec![255; 32 * 1024 * 1024]));
            let font = document.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
            let text = document.add_object(Stream::new(Dictionary::new(), b"BT /F1 12 Tf (ISBN 978-0-8213-3827-8) Tj ET".to_vec()));
            let page = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => text, "Resources" => dictionary! { "Font" => dictionary! { "F1" => font }, "XObject" => dictionary! { "Im0" => image } }, "MediaBox" => vec![0.into(),0.into(),600.into(),800.into()] });
            document.objects.insert(pages, dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => vec![page.into()] }.into());
            let root = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
            let info = document.add_object(dictionary! { "Title" => Object::string_literal("Large illustrated book"), "Author" => Object::string_literal("An Author") });
            document.trailer.set("Root", root);
            document.trailer.set("Info", info);
            let mut bytes = Vec::new();
            if modern {
                document.save_modern(&mut bytes).unwrap();
            } else {
                document.save_to(&mut bytes).unwrap();
            }
            let read = Arc::new(AtomicUsize::new(0));
            let reader = Counted { inner: Cursor::new(bytes), read: read.clone() };
            let loaded = metadata(reader).unwrap();
            assert_eq!(loaded.get_object(info).unwrap().as_dict().unwrap().get(b"Title").unwrap().as_str().unwrap(), b"Large illustrated book");
            // A compressed object block can contain page dictionaries beside Info.
            // Metadata inspection must still leave the page-content stream unread.
            assert!(loaded.get_object(text).is_err());
            assert!(read.load(Ordering::Relaxed) < 1024 * 1024, "read {} bytes (modern={modern})", read.load(Ordering::Relaxed));
            assert!(loaded.get_object(image).is_err());
        }
    }
}
