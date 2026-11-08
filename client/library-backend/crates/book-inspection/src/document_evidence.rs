//! Bounded opening-section and TOC-linked copyright inspection. TOC fragments
//! delimit combined HTML documents; body chapters stay separate from copyright text.
use crate::evidence::{self, EvidenceProgress, SectionEvidence};
use crate::profile_epub_phase;
use book_content::Book;
use book_model::{BookRecord, BookTocEntry};
use memchr::memmem;
use quick_xml::{
    Reader, XmlVersion,
    escape::{resolve_xml_entity, unescape_with},
    events::Event,
};
use serde_json::json;
use std::{borrow::Cow, collections::BTreeMap, io, ops::Range, time::Instant};

const SCAN_BYTES: u64 = 4 * 1024 * 1024;
const BLOCK_BYTES: usize = 16_384;
const MAX_BLOCKS: usize = 32;
// Zero-based XHTML section indices; copyright links are selected separately.
const FRONT_SECTIONS: Range<usize> = 1..7;

fn copyright(s: &str) -> bool {
    let s = s.to_lowercase();
    s.contains("copyright")
        || s.contains('©')
        || s.contains("(c)")
        || s.contains("cataloging-in-book")
        || s.contains("cataloguing-in-book")
        || s.contains("cataloging in book")
        || s.contains("cataloguing in book")
        || s.contains("cataloged the")
}
fn boundary(s: &str) -> bool {
    let s = s.trim().to_lowercase();
    ["contents", "table of contents", "prologue", "epilogue", "introduction", "preface", "foreword", "bibliography", "references", "works cited", "also by", "other books", "further reading", "index", "chapter", "part"]
        .iter()
        .any(|v| s.trim_end_matches(':') == *v || (*v != "references" && s.strip_prefix(v).is_some_and(|tail| tail.starts_with(' '))))
}

fn hidden_element(name: &[u8]) -> bool {
    name.eq_ignore_ascii_case(b"head") || name.eq_ignore_ascii_case(b"script") || name.eq_ignore_ascii_case(b"style")
}

fn block_element(name: &[u8]) -> bool {
    name.eq_ignore_ascii_case(b"p")
        || name.eq_ignore_ascii_case(b"div")
        || name.eq_ignore_ascii_case(b"li")
        || name.eq_ignore_ascii_case(b"h1")
        || name.eq_ignore_ascii_case(b"h2")
        || name.eq_ignore_ascii_case(b"h3")
        || name.eq_ignore_ascii_case(b"h4")
        || name.eq_ignore_ascii_case(b"section")
}

fn toc_links(entries: &[BookTocEntry], links: &mut BTreeMap<String, BTreeMap<String, bool>>) {
    for entry in entries.iter().take(2048) {
        let (uri, fragment) = entry.target.split_once('#').unwrap_or((&entry.target, ""));
        // TOC links are container-root-relative; document URIs omit the slash.
        links.entry(uri.trim_start_matches('/').to_owned()).or_default().insert(fragment.to_owned(), copyright(&entry.title));
        toc_links(&entry.children, links);
    }
}

pub(super) fn inspect(provider: &mut Book, book: &BookRecord) -> Vec<SectionEvidence> {
    let profiling = log::log_enabled!(target: "library_epub_profile", log::Level::Debug);
    let metadata_started = profiling.then(Instant::now);
    let mut progress = EvidenceProgress::from_book(book);
    if progress.complete() {
        return Vec::new();
    }
    if let Some(started) = metadata_started {
        profile_epub_phase("evidence_metadata_check", started.elapsed());
    }
    let navigation_started = profiling.then(Instant::now);
    let Ok(mut documents) = provider.inspect_documents() else { return Vec::new() };
    if let Some(started) = navigation_started {
        profile_epub_phase("evidence_navigation_prepare", started.elapsed());
    }
    let selection_started = profiling.then(Instant::now);
    let uris = documents.document_uris().to_vec();
    let mut links = BTreeMap::new();
    toc_links(documents.toc(), &mut links);
    let mut selected: Vec<_> = uris.iter().skip(FRONT_SECTIONS.start).take(FRONT_SECTIONS.len()).cloned().collect();
    for (uri, anchors) in &links {
        if anchors.values().any(|copyright| *copyright) && !selected.contains(uri) && selected.len() < 32 {
            selected.push(uri.clone());
        }
    }
    if let Some(started) = selection_started {
        profile_epub_phase("evidence_section_selection", started.elapsed());
    }
    let mut sections = Vec::new();
    for uri in selected {
        let anchors = links.get(&uri).cloned().unwrap_or_default();
        let read_started = profiling.then(Instant::now);
        if let Ok(bytes) = documents.read_prefix(&uri, SCAN_BYTES) {
            let read_ns = read_started.map(|started| started.elapsed().as_nanos());
            let input_bytes = bytes.len();
            let parse_started = profiling.then(Instant::now);
            if let Ok(found) = scan(&bytes, book, &uri, &anchors, &mut progress) {
                if let Some(started) = parse_started {
                    let index = uris.iter().position(|candidate| candidate == &uri);
                    let source = if index.is_some_and(|index| FRONT_SECTIONS.contains(&index)) { "front" } else { "copyright_link" };
                    log::debug!(target: "library_epub_profile", "{}", json!({"event":"epub_section", "uri":uri, "index":index, "documents":uris.len(), "source":source, "read_ns":read_ns, "parse_ns":started.elapsed().as_nanos(), "input_bytes":input_bytes, "copyright_link":anchors.values().any(|copyright| *copyright), "evidence":found}));
                }
                sections.extend(found.into_iter().take(MAX_BLOCKS - sections.len()));
            }
        }
        if progress.complete() || sections.len() >= MAX_BLOCKS {
            break;
        }
    }
    sections
}

struct Block<'a> {
    book: &'a BookRecord,
    progress: &'a mut EvidenceProgress,
    uri: &'a str,
    location: String,
    text: String,
    active: bool,
    eligible: bool,
    opening_bytes: usize,
    results: Vec<SectionEvidence>,
}
impl Block<'_> {
    fn finish(&mut self) {
        if self.active && self.results.len() < MAX_BLOCKS {
            let evidence = evidence::inspect_section(self.book, &self.location, &self.text);
            if evidence.accepted {
                self.progress.observe(&evidence);
                self.results.push(evidence);
            }
        }
        self.text.clear();
        self.active = false;
    }
    fn paragraph(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if text.len() < 160 && boundary(text) && !copyright(text) {
            self.finish();
            self.eligible = false;
            return;
        }
        if !self.eligible {
            return;
        }
        if !self.active {
            self.opening_bytes = self.opening_bytes.saturating_add(text.len());
            if self.opening_bytes > BLOCK_BYTES {
                self.eligible = false;
                return;
            }
        }
        if copyright(text) {
            self.active = true;
        }
        if self.text.len() + text.len() + 1 > BLOCK_BYTES {
            self.finish();
            self.eligible = false;
            return;
        }
        // Retain a little preceding context (ISBN may precede the copyright).
        if !self.active && self.text.len() > 1024 {
            self.text.clear();
        }
        self.text.push_str(text);
        self.text.push('\n');
    }
}

fn scan(bytes: &[u8], book: &BookRecord, uri: &str, anchors: &BTreeMap<String, bool>, progress: &mut EvidenceProgress) -> io::Result<Vec<SectionEvidence>> {
    // Only a copyright fragment or an epub:type attribute can make an
    // excluded block eligible again. A raw attribute-name search also covers
    // entity-encoded values without decoding the remaining document's tags.
    let can_reactivate = anchors.iter().any(|(fragment, copyright)| !fragment.is_empty() && *copyright) || memmem::find(bytes, b"epub:type").is_some();
    let mut xml = Reader::from_reader(bytes);
    xml.config_mut().expand_empty_elements = true;
    xml.config_mut().check_end_names = false; // EPUBs in the wild include HTML-style void elements.
    let mut paragraph = String::new();
    let mut hidden = 0usize;
    let mut block = Block { book, progress, uri, location: uri.to_owned(), text: String::new(), active: false, eligible: true, opening_bytes: 0, results: Vec::new() };
    if anchors.get("") == Some(&true) {
        block.text.push_str("Copyright\n");
        block.active = true;
    }
    loop {
        let event = match xml.read_event() {
            Ok(event) => event,
            Err(_) => break, // Only previously completed blocks survive malformed input.
        };
        // Once a section is excluded, only structural anchors can make it
        // eligible again. Keep finding those anchors without decoding body text.
        let text = if hidden == 0 && block.eligible {
            match &event {
                Event::Text(text) => text.decode().ok(),
                Event::GeneralRef(reference) => {
                    let raw = format!("&{};", reference.decode().unwrap_or_default());
                    unescape_with(&raw, |name| match name {
                        "copy" => Some("©"),
                        "nbsp" => Some(" "),
                        _ => resolve_xml_entity(name),
                    })
                    .ok()
                    .map(|text| Cow::Owned(text.into_owned()))
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(text) = text {
            if paragraph.len() + text.len() <= BLOCK_BYTES {
                paragraph.push_str(&text);
            } else {
                block.finish();
                block.eligible = false;
                paragraph.clear();
            }
        }
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let local_name = e.local_name();
                let name = local_name.as_ref();
                if hidden_element(name) {
                    hidden += 1;
                }
                if hidden > 0 {
                    continue;
                }
                let mut anchor = None;
                let mut copyright_type = false;
                for attribute in e.attributes().flatten() {
                    let key = attribute.key.as_ref();
                    let is_anchor = matches!(key, b"id" | b"name");
                    if (!is_anchor || anchor.is_some()) && key != b"epub:type" {
                        continue;
                    }
                    let Ok(value) = attribute.normalized_value_with(XmlVersion::Implicit1_0, 1, resolve_xml_entity) else { continue };
                    if is_anchor {
                        anchor = Some(value.into_owned());
                    } else {
                        copyright_type |= value.split_whitespace().any(|t| t == "copyright-page");
                    }
                }
                let toc_kind = anchor.as_ref().and_then(|id| anchors.get(id));
                if toc_kind.is_some() || copyright_type {
                    block.paragraph(&paragraph);
                    paragraph.clear();
                    block.finish();
                    block.location = anchor.as_deref().map_or_else(|| block.uri.to_owned(), |id| format!("{}#{id}", block.uri));
                    block.opening_bytes = 0;
                    block.eligible = toc_kind == Some(&true) || copyright_type;
                    if block.eligible {
                        block.text.push_str("Copyright\n");
                        block.active = true;
                    }
                }
                if block_element(name) || name.eq_ignore_ascii_case(b"br") {
                    block.paragraph(&paragraph);
                    paragraph.clear();
                }
            }
            Event::End(e) => {
                let local_name = e.local_name();
                let name = local_name.as_ref();
                if hidden_element(name) {
                    hidden = hidden.saturating_sub(1);
                }
                if hidden == 0 && (block_element(name) || name.eq_ignore_ascii_case(b"body")) {
                    block.paragraph(&paragraph);
                    paragraph.clear();
                }
            }
            Event::Eof => {
                block.paragraph(&paragraph);
                block.finish();
                break;
            }
            _ => {}
        }
        // Progress changes only after finishing a whole block, keeping ISBNs
        // and classification codes printed later in that block together.
        if block.progress.complete() || (!block.eligible && !can_reactivate) {
            break;
        }
    }
    Ok(block.results)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inspect(html: &str, anchors: &[(&str, bool)]) -> Vec<SectionEvidence> {
        let book = BookRecord { metadata: book_model::BookDetails { title: "wrong metadata".into(), subtitle: None, contributors: vec![], book: Default::default() }, description: String::new() };
        scan(&mut html.as_bytes(), &book, "book.xhtml", &anchors.iter().map(|(k, v)| (k.to_string(), *v)).collect(), &mut EvidenceProgress::from_book(&book)).unwrap()
    }
    #[test]
    fn generic_index_filename_does_not_discard_copyright_isbns() {
        let book = BookRecord { metadata: book_model::BookDetails { title: "Book".into(), subtitle: None, contributors: vec![], book: Default::default() }, description: String::new() };
        for (line, expected) in [("ISBN 978–0–19–921142–5", "9780199211425"), ("ISBN: 978-1-591-39782-3", "9781591397823")] {
            let html = format!("<html><body><p>Copyright 2007</p><p>{line}</p><h2>Contents</h2><p>ISBN 9780821338278</p></body></html>");
            let found = scan(&mut html.as_bytes(), &book, "index_split_000.html", &BTreeMap::new(), &mut EvidenceProgress::from_book(&book)).unwrap();
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].isbns.len(), 1);
            assert_eq!(found[0].isbns[0].isbn, expected);
        }
    }
    #[test]
    fn large_combined_document_stops_before_body_isbns() {
        let html = format!("<html><body><p>Copyright © 2004</p><p>I. Title. E3002.6.H2C48 2004</p><h3>Contents</h3><p>ISBN 9780393243277</p>{}</body></html>", "<p>Chapter text</p>".repeat(20000));
        let found = inspect(&html, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].lcc, ["E3002.6 .H2C48 2004"]);
        assert!(found[0].isbns.is_empty());
    }
    #[test]
    fn toc_fragment_reaches_copyright_after_large_section_and_stops_at_prologue() {
        let html = format!("<html><body><h1>Introduction</h1>{}<div id='rights'><p>ISBN 9780821338278</p></div><a id='start'/><p>ISBN 9780393243277</p></body></html>", "<p>Body</p>".repeat(20000));
        let found = inspect(&html, &[("rights", true), ("start", false)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].location, "book.xhtml#rights");
        assert_eq!(found[0].isbns.len(), 1);
        assert_eq!(found[0].isbns[0].isbn, "9780821338278");
    }
    #[test]
    fn unheaded_body_copyright_is_not_a_front_matter_block() {
        let html = format!("<html><body>{}<p>Copyright ISBN 9780821338278</p></body></html>", "<p>Body text without headings</p>".repeat(2000));
        assert!(inspect(&html, &[]).is_empty());
    }

    #[test]
    fn franco_copyright_sentence_does_not_stop_before_unlabelled_isbns() {
        let found = inspect(
            "<html><body><p id='rights'>Copyright 2018</p><p>References to websites were correct at the time of writing.</p><p>978 1 78453 942 9</p><p>eISBN: 978 1 78672 300 0</p><p>ePDF: 978 1 78673 300 9</p><p id='dedication'>To Susana</p></body></html>",
            &[("rights", true), ("dedication", false)],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].isbns.len(), 3);
    }

    #[test]
    fn copyright_variants_and_inline_markup() {
        for marker in ["©", "&copy;", "&#169;", "&#xA9;", "(c)", "Copyright"] {
            let found = inspect(&format!("<html><body><p>{marker} 2004</p><p>ISBN <b>9780821338278</b></p></body></html>"), &[]);
            assert_eq!(found.len(), 1, "{marker}");
        }
    }
    #[test]
    fn copyright_credits_are_not_chapter_boundaries_and_empty_scripts_are_hidden() {
        let found = inspect("<html><head><script src='kobo.js'/><style>hidden</style></head><body><p>Copyright 2004</p><p>Foreword © 2010 by Ron Chernow</p><p>Introduction copyright © 2007</p><p>ISBN 9780821338278</p></body></html>", &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].isbns[0].isbn, "9780821338278");
    }

    #[test]
    fn hidden_and_body_copyright_mentions_do_not_supply_identifiers() {
        assert!(inspect("<html><head><title>Copyright ISBN 9780821338278</title></head><body><h1>Prologue</h1><p>Copyright ISBN 9780821338278</p></body></html>", &[]).is_empty());
    }
}
