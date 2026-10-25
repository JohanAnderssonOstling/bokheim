use crate::{MetadataResult, author_from_field, author_names, fallback_metadata};
use book_model::{BookMetadata, BookRecord, BookSubject, CONTRIBUTOR_MARC_RELATOR_CODE, Contributor, Identifier, LanguageTag, Scheme, Scope, canonical_doi, canonical_uuid, unique_book_year};
use lopdf::{Dictionary, Document};
use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashSet;
use std::error::Error;
use std::fs::File;
use std::io::SeekFrom;
use std::io::{Read, Seek};
use std::path::Path;

const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";

const XML: &str = "http://www.w3.org/XML/1998/namespace";
const MAX_VALUES: usize = 256;
const MAX_TEXT_BYTES: usize = 4_096;
const MAX_DESCRIPTION_BYTES: usize = 64 * 1_024;
const MAX_XMP_BYTES: usize = 8 * 1_024 * 1_024;
const EARLY_TEXT_PAGES: usize = 20;

pub struct InspectedPdf {
    pub metadata: BookRecord,
}

/// Extracts bounded, allow-listed bibliographic metadata from the PDF info
/// dictionary and XMP packet. First and last pages use the shared context-aware evidence rules; arbitrary XMP extensions are never promoted.
pub fn inspect_pdf(path: &Path) -> MetadataResult<InspectedPdf> {
    inspect_pdf_reader(path, File::open(path)?)
}

/// Page evidence is always read: it joins the file facts the metadata server
/// is asked about.
pub fn inspect_pdf_reader<R: Read + Seek + Send + 'static>(source_name: &Path, reader: R) -> MetadataResult<InspectedPdf> {
    inspect_pdf_pages_mode(source_name, reader)
}

pub fn inspect_pdf_pages_reader<R: Read + Seek + Send + 'static>(source_name: &Path, reader: R) -> MetadataResult<InspectedPdf> {
    inspect_pdf_pages_mode(source_name, reader)
}

fn inspect_pdf_pages_mode<R: Read + Seek + Send + 'static>(source_name: &Path, mut reader: R) -> MetadataResult<InspectedPdf> {
    let mut inspected = super::pdf_ranges::load_edges(&mut reader, EARLY_TEXT_PAGES).and_then(|document| inspect_pdf_document(source_name, document));
    let original_ids = inspected.as_mut().map(|i| std::mem::take(&mut i.metadata.book.identifiers)).unwrap_or_default();
    {
        // A PDFium-readable book must not lose its page evidence just because
        // the metadata-only parser cannot handle its cross-reference variant.
        let mut inspected = inspected.unwrap_or_else(|_| InspectedPdf { metadata: fallback_metadata(source_name) });
        reader.seek(SeekFrom::Start(0))?;
        let (count, pages) = pdf_reader_core::inspect_page_text_with_count(reader, EARLY_TEXT_PAGES, crate::evidence::MAX_SECTION_BYTES)?;
        let evidence = pages
            .into_iter()
            .map(|(page, text)| {
                if page >= count.saturating_sub(1) {
                    {
                        use crate::evidence::*;
                        let book: &BookRecord = &inspected.metadata;
                        let location: &str = &format!("pdf:page:{page}:back-cover");
                        let text: &str = &text;

                        try_inspect_section(book, location, text)
                    }
                } else {
                    crate::evidence::try_inspect_section(&inspected.metadata, &format!("pdf:page:{page}"), &text)
                }
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        crate::evidence::apply_evidence(&mut inspected.metadata, &evidence, "pdf");
        inspected.metadata.book.identifiers.extend(original_ids);
        Ok(inspected)
    }
}

fn pdf_xmp(document: &Document) -> Option<String> {
    let root = document.dereference(document.trailer.get(b"Root").ok()?).ok()?.1.as_dict().ok()?;
    let metadata = document.dereference(root.get(b"Metadata").ok()?).ok()?.1.as_stream().ok()?;
    if metadata.content.len() > MAX_XMP_BYTES {
        return None;
    }
    let bytes = metadata.decompressed_content().ok()?;
    if bytes.len() > MAX_XMP_BYTES {
        return None;
    }
    std::str::from_utf8(&bytes).ok().map(str::to_owned)
}

fn inspect_pdf_document(path: &Path, document: Document) -> MetadataResult<InspectedPdf> {
    if document.is_encrypted() && document.encryption_state.is_none() {
        return Err("password-protected PDF books are not supported".into());
    }

    let info = document.trailer.get(b"Info").ok().and_then(|object| document.dereference(object).ok()).and_then(|(_, object)| object.as_dict().ok());
    let xmp = pdf_xmp(&document);
    let parsed = xmp.as_deref().and_then(|xml| XmlDocument::parse(xml).ok());

    let title_values = parsed.as_ref().map(|xml| xmp_values(xml, DC, "title", MAX_TEXT_BYTES)).unwrap_or_default();
    let info_title = info.and_then(|dictionary| dictionary_text(dictionary, b"Title"));
    let description_values = parsed.as_ref().map(|xml| xmp_values(xml, DC, "description", MAX_DESCRIPTION_BYTES)).unwrap_or_default();
    let description = preferred_language_value(&description_values).or_else(|| info.and_then(|dictionary| dictionary_text(dictionary, b"Subject"))).unwrap_or_default();

    let creator_values = parsed.as_ref().map(|xml| xmp_values(xml, DC, "creator", MAX_TEXT_BYTES)).unwrap_or_default();
    // An XMP creator sequence names one person per entry, so those are only
    // cleaned. `/Author` is a single free-text field that a third of files use
    // for several people at once, so it is also split — see `author_names`.
    let mut contributors = creator_values.iter().flat_map(|value| author_names(&value.text)).filter_map(author_from_field).collect::<Vec<_>>();
    if contributors.is_empty() {
        let stated = info.and_then(|dictionary| dictionary_text(dictionary, b"Author")).unwrap_or_default();
        contributors.extend(author_names(&stated).into_iter().filter_map(author_from_field));
    }
    if let Some(xml) = parsed.as_ref() {
        contributors.extend(xmp_values(xml, DC, "contributor", MAX_TEXT_BYTES).into_iter().filter_map(|value| Contributor::new(value.text, CONTRIBUTOR_MARC_RELATOR_CODE).ok()));
    }
    {
        let values: &mut Vec<Contributor> = &mut contributors;
        let mut seen = HashSet::new();
        values.retain(|value| seen.insert((value.name().to_owned(), value.role_code())));
    };
    let valid_titles = title_values.iter().filter(|value| !crate::title_identity::unusable_title(&value.text, &contributors)).cloned().collect::<Vec<_>>();
    let title = preferred_language_value(&valid_titles).or_else(|| info_title.clone().filter(|title| !crate::title_identity::unusable_title(title, &contributors))).unwrap_or_else(|| fallback_metadata(path).metadata.title);

    let mut book = BookMetadata::default();
    if let Some(xml) = parsed.as_ref() {
        book.identifiers = {
            let document: &XmlDocument<'_> = xml;

            let mut values = xmp_values(document, DC, "identifier", MAX_TEXT_BYTES);
            values.extend(xmp_values(document, XMP, "Identifier", MAX_TEXT_BYTES));
            for node in document
                .descendants()
                .filter(|node| node.is_element() && node.tag_name().namespace().is_some_and(|namespace| namespace.contains("prismstandard.org")) && matches!(node.tag_name().name().to_ascii_lowercase().as_str(), "isbn" | "doi" | "issn"))
            {
                let mut extra = Vec::new();
                push_xmp_value(&mut extra, node, MAX_TEXT_BYTES);
                for value in &mut extra {
                    value.scheme = Some(node.tag_name().name().to_owned());
                }
                values.extend(extra);
            }
            values
                .into_iter()
                .filter_map(|value: XmpValue| -> Option<Identifier> {
                    let declared = value.scheme.as_deref().unwrap_or_default().to_ascii_lowercase();
                    let trimmed = {
                        let text = value.text.trim();
                        ["ISBN-13:", "ISBN-10:", "ISBN:", "isbn-13:", "isbn-10:", "isbn:", "DOI:", "doi:"].iter().find_map(|label| text.strip_prefix(label)).unwrap_or(text).trim()
                    };
                    let (value, scheme) = if declared.contains("isbn") || book_model::from_metadata_value(trimmed).is_some() {
                        (book_model::from_metadata_value(trimmed)?, Scheme::Isbn)
                    } else if declared.contains("doi") || canonical_doi(trimmed).is_some() {
                        (canonical_doi(trimmed)?, Scheme::Doi)
                    } else if declared.contains("uuid") || canonical_uuid(trimmed).is_some() {
                        (canonical_uuid(trimmed)?, Scheme::Uuid)
                    } else if declared.contains("issn") {
                        (trimmed.to_owned(), Scheme::Issn)
                    } else {
                        (trimmed.to_owned(), Scheme::Unspecified)
                    };
                    Identifier::new(value, scheme, Scope::Book).ok()
                })
                .take(MAX_VALUES)
                .collect()
        };
        book.publishers = xmp_values(xml, DC, "publisher", MAX_TEXT_BYTES).into_iter().map(|value| value.text).collect();
        book.languages = xmp_values(xml, DC, "language", MAX_TEXT_BYTES).into_iter().filter_map(|value| LanguageTag::parse(value.text).ok()).collect();
        book.year = unique_book_year(xmp_values(xml, DC, "date", MAX_TEXT_BYTES).iter().map(|value| value.text.as_str()));
        book.subjects = xmp_values(xml, DC, "subject", MAX_TEXT_BYTES).into_iter().filter_map(|value| BookSubject::new(None, value.text, "pdf:xmp:dc:subject", None, None).ok()).collect();
    }

    for original in title_values.iter().map(|value| value.text.as_str()).chain(info_title.as_deref()) {
        crate::filename_isbn::record(original, &mut book)?;
    }

    // Check complete dictionary/XMP property values regardless of their names.
    // Do not extract numeric substrings from URLs, dates, UUIDs or filenames.
    let mut values = Vec::new();
    if let Some(info) = info {
        for (key, _) in info.iter().take(MAX_VALUES) {
            if let Some(value) = dictionary_text(info, key) {
                values.push(value);
            }
        }
    }
    if let Some(xml) = parsed.as_ref() {
        for node in xml.descendants().filter(|node| node.is_element()).take(4096) {
            if let Some(text) = node.text() {
                values.push(text.to_owned());
            }
            values.extend(node.attributes().map(|a| a.value().to_owned()));
        }
    }
    for value in values {
        if let Some(isbn) = book_model::from_metadata_value(&value) {
            book_model::push_isbn(&mut book.identifiers, isbn, Scope::Book)?;
        }
    }

    {
        let book: &mut BookMetadata = &mut book;
        let mut identifiers = Vec::new();
        book.identifiers.retain(|value| {
            let key = (value.scheme().clone(), value.lookup_value());
            if identifiers.contains(&key) {
                false
            } else {
                identifiers.push(key);
                true
            }
        });
        let mut subjects = HashSet::new();
        book.subjects.retain(|value| subjects.insert(value.name().to_owned()));
    };
    let mut metadata = BookRecord { metadata: book_model::BookDetails { title, subtitle: None, contributors, book }, description };
    crate::title_identity::normalize(&mut metadata).map_err(|error| -> Box<dyn Error + Send + Sync> { error.into() })?;
    Ok(InspectedPdf { metadata })
}

#[derive(Clone)]
struct XmpValue {
    text: String,
    language: Option<String>,
    scheme: Option<String>,
}

fn dictionary_text(dictionary: &Dictionary, key: &[u8]) -> Option<String> {
    bounded_text(&lopdf::decode_text_string(dictionary.get(key).ok()?).ok()?, MAX_DESCRIPTION_BYTES)
}

fn xmp_values(document: &XmlDocument<'_>, namespace: &str, local_name: &str, max_bytes: usize) -> Vec<XmpValue> {
    let mut values = Vec::new();
    // XMP permits simple properties both as RDF attributes on Description and
    // as child elements. Adobe tools commonly use the former.
    for attribute in document.descendants().filter(|node| node.is_element()).flat_map(|node| node.attributes()) {
        if attribute.namespace() == Some(namespace) && attribute.name().eq_ignore_ascii_case(local_name) {
            if let Some(text) = bounded_text(attribute.value(), max_bytes) {
                values.push(XmpValue { text, language: None, scheme: None });
            }
            if values.len() >= MAX_VALUES {
                return values;
            }
        }
    }
    for property in document.descendants().filter(|node| node.is_element() && node.tag_name().namespace() == Some(namespace) && node.tag_name().name().eq_ignore_ascii_case(local_name)) {
        let items = property.descendants().filter(|node| node.is_element() && node.tag_name().name() == "li").collect::<Vec<_>>();
        if items.is_empty() {
            push_xmp_value(&mut values, property, max_bytes);
        } else {
            for item in items {
                push_xmp_value(&mut values, item, max_bytes);
                if values.len() >= MAX_VALUES {
                    return values;
                }
            }
        }
        if values.len() >= MAX_VALUES {
            break;
        }
    }
    values
}

fn push_xmp_value(values: &mut Vec<XmpValue>, node: Node<'_, '_>, max_bytes: usize) {
    let combined = node.descendants().filter(|child| child.is_text()).filter_map(|child| child.text()).collect::<Vec<_>>().join(" ");
    let Some(text) = bounded_text(&combined, max_bytes) else { return };
    let language = node.attribute((XML, "lang")).map(str::to_owned);
    let scheme = node.attributes().find(|attribute| attribute.name().eq_ignore_ascii_case("Scheme")).map(|attribute| attribute.value().to_owned());
    values.push(XmpValue { text, language, scheme });
}

fn preferred_language_value(values: &[XmpValue]) -> Option<String> {
    values.iter().find(|value| value.language.as_deref() == Some("x-default")).or_else(|| values.first()).map(|value| value.text.clone())
}

fn bounded_text(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes || value.chars().any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t')) {
        return None;
    }
    Some(value.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Object, Stream, dictionary, text_string};
    use tempfile::tempdir;

    #[test]
    #[ignore = "requires PDF_INSPECTION_FIXTURES pointing at a local PDF corpus"]
    fn range_inspection_matches_full_parser_on_local_corpus() {
        fn collect(path: &Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    collect(&entry.path(), files);
                } else if entry.path().extension().is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")) {
                    files.push(entry.path());
                }
            }
        }
        let root = std::env::var("PDF_INSPECTION_FIXTURES").expect("PDF_INSPECTION_FIXTURES");
        let mut files = Vec::new();
        collect(Path::new(&root), &mut files);
        files.sort();
        assert!(!files.is_empty());
        let mut successful = 0;
        for path in files.iter().take(30) {
            let full = Document::load(path).map_err(|e| -> Box<dyn Error + Send + Sync> { Box::new(e) }).and_then(|document| inspect_pdf_document(path, document));
            let ranged = inspect_pdf(path);
            match (full, ranged) {
                (Ok(full), Ok(ranged)) => {
                    fn normalized(metadata: &BookRecord) -> serde_json::Value {
                        let mut value = serde_json::to_value(metadata).unwrap();
                        for contributor in value["contributors"].as_array_mut().unwrap() {
                            contributor.as_object_mut().unwrap().remove("contributor_id");
                        }
                        value
                    }
                    assert_eq!(normalized(&ranged.metadata), normalized(&full.metadata), "metadata changed for {}", path.display());
                    successful += 1;
                }
                (Err(_), Err(_)) => (),
                (Ok(_), Err(error)) => panic!("range inspection failed for {}: {error}", path.display()),
                (Err(error), Ok(_)) => panic!("full parser rejected {}, range parser accepted it: {error}", path.display()),
            }
        }
        println!("Compared {} PDFs; {successful} successfully inspected", files.len().min(30));
        assert!(successful > 0);
    }

    #[test]
    fn pathological_pdf_title_uses_filename_and_stores_canonical_fields() {
        for title in ["Untrusted words ISBN: 0192805045", "0192805045.pdf", "ISBN: 9780192802156", "9780192802157.pdf", "Microsoft Word - Document1", "C:\\Users\\scanner\\draft.doc", "d838e6e05a8f009f5fa3c0ee4fd1ecd8"] {
            let mut document = Document::with_version("1.7");
            let info = document.add_object(dictionary! { "Title" => Object::string_literal(title), "Author" => Object::string_literal("Adobe Acrobat") });
            document.trailer.set("Info", info);
            let inspected = inspect_pdf_document(Path::new("Algorithms_ A Guide (Jeff Erickson) (2019).pdf"), document).unwrap();
            assert_eq!(inspected.metadata.title, "Algorithms");
            let expected = crate::filename_isbn::from_filename(title);
            assert_eq!(inspected.metadata.book.identifiers.iter().filter(|id| id.scheme() == &Scheme::Isbn).map(|id| id.value().to_owned()).collect::<Vec<_>>(), expected);
            assert_eq!(inspected.metadata.subtitle(), Some("A Guide"));
            assert_eq!(inspected.metadata.contributors[0].name(), "Jeff Erickson");
            assert_eq!(inspected.metadata.book.year, Some(2019));
        }
    }

    #[test]
    fn invalid_xmp_title_does_not_hide_valid_info_title() {
        let mut document = Document::with_version("1.7");
        let xml = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>0192805045.pdf</dc:title></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let xmp = document.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xml.to_vec()));
        let root = document.add_object(dictionary! { "Type" => "Catalog", "Metadata" => xmp });
        document.trailer.set("Root", root);
        let info = document.add_object(dictionary! { "Title" => Object::string_literal("1984") });
        document.trailer.set("Info", info);
        let inspected = inspect_pdf_document(Path::new("unrelated.pdf"), document).unwrap();
        assert_eq!(inspected.metadata.title, "1984");
        assert!(inspected.metadata.book.identifiers.iter().any(|id| id.value() == "0192805045"));
    }

    #[test]
    fn production_extensions_in_xmp_fall_back_to_info_title() {
        for title in ["85885_TheArt_TX_p1-342.indd", "0465002566-text.qxd:0465002566 text", "Book.p65"] {
            let mut document = Document::with_version("1.7");
            let xml = format!(
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title></rdf:Description></rdf:RDF></x:xmpmeta>"#
            );
            let xmp = document.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xml.into_bytes()));
            let root = document.add_object(dictionary! { "Type" => "Catalog", "Metadata" => xmp });
            document.trailer.set("Root", root);
            let info = document.add_object(dictionary! { "Title" => Object::string_literal("Actual title") });
            document.trailer.set("Info", info);
            let inspected = inspect_pdf_document(Path::new("unrelated.pdf"), document).unwrap();
            assert_eq!(inspected.metadata.title, "Actual title", "{title}");
        }
    }

    #[test]
    fn author_only_xmp_uses_valid_info_title() {
        let mut document = Document::with_version("1.7");
        let xml = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Quentin Skinner</dc:title><dc:creator>Quentin Skinner</dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let xmp = document.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xml.to_vec()));
        let root = document.add_object(dictionary! { "Type" => "Catalog", "Metadata" => xmp });
        document.trailer.set("Root", root);
        let info = document.add_object(dictionary! { "Title" => Object::string_literal("Machiavelli") });
        document.trailer.set("Info", info);
        let inspected = inspect_pdf_document(Path::new("unrelated.pdf"), document).unwrap();
        assert_eq!(inspected.metadata.title, "Machiavelli");
    }

    #[test]
    fn extracts_rich_xmp_and_checksum_valid_identifier() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("rich.pdf");
        let mut document = Document::with_version("1.7");
        let metadata = br#"<?xpacket begin=''?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xmp='http://ns.adobe.com/xap/1.0/' xmlns:calibre='http://calibre-ebook.com/xmp-namespace' calibre:series='Examples' calibre:series_index='2'><dc:title><rdf:Alt><rdf:li xml:lang='x-default'>Rich title</rdf:li><rdf:li xml:lang='sv'>Rik titel</rdf:li></rdf:Alt></dc:title><dc:creator><rdf:Seq><rdf:li>Ada Author</rdf:li></rdf:Seq></dc:creator><dc:publisher><rdf:Bag><rdf:li>Example Press</rdf:li></rdf:Bag></dc:publisher><dc:date><rdf:Seq><rdf:li>2024-05-06</rdf:li></rdf:Seq></dc:date><dc:language><rdf:Bag><rdf:li>sv</rdf:li></rdf:Bag></dc:language><dc:identifier>urn:isbn:978-0-8213-3827-8</dc:identifier><dc:subject><rdf:Bag><rdf:li>History</rdf:li></rdf:Bag></dc:subject><dc:rights><rdf:Alt><rdf:li xml:lang='x-default'>Copyright example</rdf:li></rdf:Alt></dc:rights></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let metadata = String::from_utf8(metadata.to_vec()).unwrap().replace("</rdf:Description>", "<calibre:ASIN>978-1-934356-55-5</calibre:ASIN><calibre:cover>9780199588503.jpg</calibre:cover></rdf:Description>");
        let metadata_id = document.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, metadata.into_bytes()));
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
            "Resources" => dictionary! {},
        });
        document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
        let root_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id, "Metadata" => metadata_id });
        document.trailer.set("Root", root_id);
        document.save(&path).unwrap();

        let inspected = inspect_pdf(&path).unwrap();
        assert_eq!(inspected.metadata.title, "Rich title");
        assert_eq!(inspected.metadata.authors().next().unwrap().name(), "Ada Author");
        assert_eq!(inspected.metadata.book.publishers[0], "Example Press");
        assert_eq!(inspected.metadata.book.identifiers[0].canonical_value().as_deref(), Some("9780821338278"));
        assert!(inspected.metadata.book.identifiers.iter().any(|id| id.scheme() == &Scheme::Isbn && id.canonical_value().as_deref() == Some("9781934356555")));
        assert!(!inspected.metadata.book.identifiers.iter().any(|id| id.canonical_value().as_deref() == Some("9780199588503")));
        assert_eq!(inspected.metadata.book.subjects[0].name(), "History");

        let author_cases: &[(&str, &[&str])] = &[
            ("d838e6e05a8f009f5fa3c0ee4fd1ecd8", &[]),
            (r"C:\Users\scanner", &[]),
            ("\u{fffd}garbled", &[]),
            ("Adobe Acrobat 7.0", &[]),
            ("ABBYY FineReader", &[]),
            ("PScript5.dll Version 5.2", &[]),
            ("LaTeX with hyperref package", &[]),
            ("Www.Yutou.Org", &[]),
            ("Administrator", &[]),
            ("Microsoft Word", &[]),
            ("Yakobson, Alexander, Gat, Azar", &["Yakobson, Alexander, Gat, Azar"]),
            ("Russ Cox, Frans Kaashoek, Robert Morris", &["Russ Cox, Frans Kaashoek, Robert Morris"]),
            ("Fred C. Piper (Author), Sean Murphy (Author)", &["Fred C. Piper", "Sean Murphy"]),
            ("Piper, Fred C. (Author), Murphy, Sean (Author)", &["Fred C. Piper", "Sean Murphy"]),
            ("Boyer, Paul S.(Author)", &["Paul S. Boyer"]),
            ("Grose, Peter, 1934-", &["Peter Grose"]),
            ("Renehan, Edward.", &["Edward Renehan"]),
            ("Sidebottom, Harry.", &["Harry Sidebottom"]),
            ("Boyer, Paul S.", &["Paul S. Boyer"]),
        ];
        for (index, (raw, expected)) in author_cases.iter().enumerate() {
            let mut author_document = document.clone();
            author_document.get_object_mut(root_id).unwrap().as_dict_mut().unwrap().remove(b"Metadata");
            let info_id = author_document.add_object(dictionary! {
                "Title" => Object::string_literal("Rich title"),
                "Author" => text_string(*raw),
            });
            author_document.trailer.set("Info", info_id);
            let author_path = directory.path().join(format!("author-field-{index}.pdf"));
            author_document.save(&author_path).unwrap();
            let inspected = inspect_pdf_metadata(&author_path).unwrap();
            assert_eq!(inspected.metadata.title, "Rich title");
            assert_eq!(inspected.metadata.authors().map(|credit| credit.name()).collect::<Vec<_>>().as_slice(), *expected, "{raw}");
        }
    }
}

/// Read dictionary and XMP values without page text.
pub fn inspect_pdf_metadata(path: &Path) -> MetadataResult<InspectedPdf> {
    inspect_pdf_document(path, super::pdf_ranges::metadata(File::open(path)?)?)
}
