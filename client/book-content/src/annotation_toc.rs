//! Match publication CFIs to the nearest preceding TOC anchor without layout.
use crate::{ReadingBook, resolve_resource_href};
use book_model::{BookTocEntry, EpubCfi};
use quick_xml::{Reader, XmlVersion, events::Event};
use scraper::{ElementRef, Html};
use std::collections::{HashMap, HashSet};

struct Entry {
    document: usize,
    fragment: bool,
    path: Option<Vec<usize>>,
    flattened: usize,
}

/// Ordinals follow the reader's resolved TOC order. Flattened ordinals retain
/// unresolved grouping headings for document exports.
pub struct AnnotationTocIndex {
    packages: HashMap<Vec<usize>, usize>,
    entries: Vec<Entry>,
    document_count: usize,
}

impl AnnotationTocIndex {
    pub fn new(reading: &ReadingBook, toc: &[BookTocEntry]) -> Self {
        Self::from_documents(reading.document_uris(), reading.publication_cfi_bases(), toc, None, |uri| {
            let bytes = reading.resources.read_bytes(uri).ok()?;
            let xml = reading.resources.media_type(uri).ok().flatten().as_deref() == Some("application/xhtml+xml") || uri.to_ascii_lowercase().ends_with(".xhtml");
            Some((bytes, xml))
        })
    }

    pub(crate) fn from_documents(documents: &[String], bases: Option<Vec<Option<String>>>, toc: &[BookTocEntry], cfis: Option<&[&str]>, mut read: impl FnMut(&str) -> Option<(Vec<u8>, bool)>) -> Self {
        let mut packages = HashMap::new();
        for index in 0..documents.len() {
            let base = match &bases {
                Some(bases) => bases.get(index).and_then(|base| base.as_deref()).map(str::to_owned),
                None => Some(format!("/6/{}", (index + 1) * 2)),
            };
            if let Some(path) = base.as_deref().and_then(steps) {
                packages.entry(path).or_insert(index);
            }
        }
        let needed = cfis.map(|cfis| {
            cfis.iter()
                .filter_map(|cfi| {
                    EpubCfi::parse(cfi).ok()?;
                    let inner = cfi.strip_prefix("epubcfi(")?.strip_suffix(')')?;
                    let parts = split(inner, '!')?;
                    packages.get(&steps(parts.first()?)?).copied()
                })
                .collect::<HashSet<_>>()
        });
        let mut links = Vec::new();
        flatten(toc, &mut links);
        let mut anchors = HashMap::new();
        let entries = links
            .into_iter()
            .enumerate()
            .filter_map(|(flattened, target)| {
                let (href, fragment) = target.split_once('#').unwrap_or((target, ""));
                // An unresolved grouping heading must not point to document zero.
                if href.is_empty() {
                    return None;
                }
                let uri = resolve_resource_href("", href);
                let mut matches = documents.iter().enumerate().filter(|(_, document)| resolve_resource_href("", document) == uri).map(|(index, _)| index);
                let document = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                let path = if fragment.is_empty() || needed.as_ref().is_some_and(|needed| !needed.contains(&document)) {
                    None
                } else {
                    anchors
                        .entry(document)
                        .or_insert_with(|| {
                            let uri = &documents[document];
                            let Some((bytes, xml)) = read(uri) else { return HashMap::new() };
                            if xml { xml_anchors(&bytes).unwrap_or_default() } else { html_anchors(&String::from_utf8_lossy(&bytes)) }
                        })
                        .get(fragment)
                        .cloned()
                };
                Some(Entry { document, fragment: !fragment.is_empty(), path, flattened })
            })
            .collect();
        Self { packages, entries, document_count: documents.len() }
    }

    pub fn ordinal_for_cfi(&self, cfi: &str) -> Option<i64> {
        self.resolved_ordinal(cfi)?.try_into().ok()
    }

    pub fn flattened_ordinal_for_cfi(&self, cfi: &str) -> Option<i64> {
        self.entries.get(self.resolved_ordinal(cfi)?)?.flattened.try_into().ok()
    }

    fn resolved_ordinal(&self, cfi: &str) -> Option<usize> {
        EpubCfi::parse(cfi).ok()?;
        let inner = cfi.strip_prefix("epubcfi(")?.strip_suffix(')')?;
        let parts = split(inner, '!')?;
        if parts.len() > 2 {
            return None;
        }
        let document = *self.packages.get(&steps(parts[0])?)?;
        if document >= self.document_count {
            return None;
        }
        let path = match parts.get(1) {
            None => Vec::new(),
            Some(content) => {
                let parts = split(content, ',')?;
                let start = match parts.as_slice() {
                    [point] => point.to_string(),
                    [common, start, _] => format!("{}/{}", common.trim_end_matches('/'), start.trim_start_matches('/')),
                    _ => return None,
                };
                steps(&start)?
            }
        };
        // Same precedence as the reader: nearest source anchor, then a document
        // entry, then the first same-document link, then the preceding document.
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.document == document)
            .filter_map(|(ordinal, entry)| entry.path.as_ref().filter(|anchor| *anchor <= &path).map(|anchor| (ordinal, anchor)))
            .max_by(|(_, a), (_, b)| a.cmp(b))
            .map(|(ordinal, _)| ordinal)
            .or_else(|| self.entries.iter().position(|entry| entry.document == document && !entry.fragment))
            .or_else(|| self.entries.iter().position(|entry| entry.document == document))
            .or_else(|| self.entries.iter().enumerate().filter(|(_, entry)| entry.document < document).max_by_key(|(ordinal, entry)| (entry.document, *ordinal)).map(|(ordinal, _)| ordinal))
    }
}

fn flatten<'a>(entries: &'a [BookTocEntry], links: &mut Vec<&'a str>) {
    for entry in entries {
        links.push(&entry.target);
        flatten(&entry.children, links);
    }
}

/// Split outside escaped CFI assertions (IDs can contain commas and `!`).
fn split(value: &str, delimiter: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut assertion = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '^' => escaped = true,
            '[' => assertion = true,
            ']' => assertion = false,
            _ if ch == delimiter && !assertion => {
                parts.push(&value[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if assertion || escaped {
        return None;
    }
    parts.push(&value[start..]);
    Some(parts)
}

fn steps(value: &str) -> Option<Vec<usize>> {
    let mut plain = String::new();
    let mut assertion = false;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '^' => escaped = true,
            '[' => assertion = true,
            ']' => assertion = false,
            ':' | ';' | '~' | '@' if !assertion => break,
            _ if !assertion => plain.push(ch),
            _ => {}
        }
    }
    if escaped || assertion {
        return None;
    }
    let path = plain.strip_prefix('/')?.split('/').map(|step| step.parse::<usize>().ok().filter(|step| *step > 0)).collect::<Option<Vec<_>>>()?;
    (!path.is_empty()).then_some(path)
}

fn xml_anchors(bytes: &[u8]) -> Option<HashMap<String, Vec<usize>>> {
    // Attribute decoding follows the declaration; text nodes need no decoding
    // because TOC containment compares element source paths, not text offsets.
    let source = crate::epub::decode_package_xml(bytes);
    let mut reader = Reader::from_str(&source);
    let mut anchors = HashMap::new();
    let mut stack = Vec::<(Vec<usize>, usize)>::new();
    loop {
        let event = reader.read_event().ok()?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                let path = if let Some((path, count)) = stack.last_mut() {
                    *count += 1;
                    let mut next = path.clone();
                    next.push(*count * 2);
                    next
                } else {
                    Vec::new()
                };
                for attr in start.attributes() {
                    let attr = attr.ok()?;
                    if attr.key.as_ref() == b"id" {
                        anchors.entry(attr.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder()).ok()?.into_owned()).or_insert_with(|| path.clone());
                    }
                }
                if !empty {
                    stack.push((path, 0));
                }
                if stack.len() > 128 {
                    return None;
                }
            }
            Event::End(_) => {
                stack.pop()?;
            }
            Event::Eof => return stack.is_empty().then_some(anchors),
            _ => {}
        }
    }
}

fn html_anchors(source: &str) -> HashMap<String, Vec<usize>> {
    let document = Html::parse_document(source);
    let mut anchors = HashMap::new();
    for element in document.root_element().descendants().filter_map(ElementRef::wrap) {
        let Some(id) = element.value().attr("id") else { continue };
        let mut path = Vec::new();
        let mut current = element;
        while let Some(parent) = current.parent().and_then(ElementRef::wrap) {
            let Some(index) = parent.children().filter_map(ElementRef::wrap).position(|child| child.id() == current.id()) else { break };
            path.push((index + 1) * 2);
            current = parent;
        }
        path.reverse();
        anchors.entry(id.to_owned()).or_insert(path);
    }
    anchors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Book, BookFormat};
    use futures_lite::future::block_on;
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn publication_book() -> Book {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in [
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#),
            (
                "OPS/book.opf",
                r#"<package><metadata/><manifest><item id="unsupported" href="ignored.pdf" media-type="application/pdf"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><guide/><spine id="sp!ine"><itemref idref="unsupported"/><itemref idref="one" id="one]id"/><itemref idref="missing"/><itemref idref="two"/></spine></package>"#,
            ),
            ("OPS/ignored.pdf", "%PDF-ignore"),
            ("OPS/one.xhtml", r#"<!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml"><head/><body><h1 id="first">First</h1><p>Before</p><section id="second"><h2>Second</h2><p>After 😀</p></section></body></html>"#),
            ("OPS/two.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head/><body><p>Next document</p></body></html>"#),
        ] {
            archive.start_file(name, SimpleFileOptions::default()).unwrap();
            archive.write_all(text.as_bytes()).unwrap();
        }
        Book::open_reader(BookFormat::Epub, archive.finish().unwrap()).unwrap()
    }

    fn publication() -> ReadingBook {
        block_on(publication_book().prepare_reading()).unwrap()
    }

    #[test]
    fn lazy_toc_lookup_matches_reading_and_reads_only_fragment_documents_once() {
        let cfis = ["epubcfi(/8/4!/4/4/1:0)", "epubcfi(/8/4!/4/6/4/1:0)", "epubcfi(/8/8!/4/2/1:0)"];
        let lazy = publication_book().annotation_toc_index_for_cfis(&toc(), &cfis).unwrap();
        let prepared = AnnotationTocIndex::new(&publication(), &toc());
        for cfi in cfis {
            assert_eq!(lazy.ordinal_for_cfi(cfi), prepared.ordinal_for_cfi(cfi));
        }
        let mut reads = Vec::new();
        let mut navigation = toc();
        navigation.push(BookTocEntry { title: "Unused chapter".into(), target: "/OPS/two.xhtml#third".into(), children: vec![] });
        AnnotationTocIndex::from_documents(&["OPS/one.xhtml".into(), "OPS/two.xhtml".into()], None, &navigation, Some(&["epubcfi(/6/2!/4/6/4/1:0)"]), |uri| {
            reads.push(uri.to_owned());
            Some((b"<html><head/><body><section id=\"second\"/></body></html>".to_vec(), true))
        });
        assert_eq!(reads, ["OPS/one.xhtml"]);
    }

    fn toc() -> Vec<BookTocEntry> {
        vec![BookTocEntry {
            title: "Part".into(),
            target: String::new(),
            children: vec![
                BookTocEntry { title: "Unavailable".into(), target: "/missing.xhtml".into(), children: vec![] },
                BookTocEntry { title: "Chapter".into(), target: "/OPS/one.xhtml".into(), children: vec![BookTocEntry { title: "Section".into(), target: "/OPS/one.xhtml#second".into(), children: vec![] }] },
            ],
        }]
    }

    #[test]
    fn full_cfis_match_sections_and_keep_reader_and_export_ordinals_distinct() {
        let reading = publication();
        assert_eq!(reading.document_uris(), ["OPS/one.xhtml", "OPS/two.xhtml"]);
        let index = AnnotationTocIndex::new(&reading, &toc());
        let before = "epubcfi(/8[sp!ine]/4[one^]id]!/4/4,/1:0,/1:6)";
        let after = "epubcfi(/8/4!/4/6/4,/1:0,/1:5)";
        assert_eq!(index.ordinal_for_cfi(before), Some(0));
        assert_eq!(index.flattened_ordinal_for_cfi(before), Some(2));
        assert_eq!(index.ordinal_for_cfi(after), Some(1));
        assert_eq!(index.flattened_ordinal_for_cfi(after), Some(3));
        // Another document with no TOC entry inherits the preceding section.
        assert_eq!(index.ordinal_for_cfi("epubcfi(/8/8!/4/2/1:0)"), Some(1));
        assert_eq!(index.ordinal_for_cfi("epubcfi(/8/4)"), Some(0));
        for unknown in ["invalid", "epubcfi(/8/2!/4/2/1:0)", "epubcfi(/6/2!/4/2/1:0)"] {
            assert_eq!(index.ordinal_for_cfi(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn escaped_assertions_do_not_split_ranges_and_offsets_do_not_change_sections() {
        let reading = publication();
        let index = AnnotationTocIndex::new(&reading, &toc());
        assert_eq!(index.ordinal_for_cfi("epubcfi(/8[sp!ine]/4[one^]id]!/4[body^,id]/6[second]/4,/1:2[before^,after],/1:9)"), Some(1));
        assert_eq!(index.ordinal_for_cfi("epubcfi(/8/4!/4/6[second])"), Some(1));
    }

    #[test]
    fn xml_and_html_anchors_use_element_steps_with_empty_elements_and_whitespace() {
        let source = r#"<html><head/><body> text <h1 id="one">One</h1> <p/> <section id="two"><p>Two</p></section></body></html>"#;
        let xml = xml_anchors(source.as_bytes()).unwrap();
        assert_eq!(xml["one"], vec![4, 2]);
        assert_eq!(xml["two"], vec![4, 6]);
        let html = html_anchors(source);
        assert_eq!(html["one"], vec![4, 2]);
        assert_eq!(html["two"], vec![4, 6]);
    }
}
