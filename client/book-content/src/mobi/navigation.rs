//! Convert MOBI navigation offsets and links into prepared document anchors.

use super::*;
use crate::BookTocEntry;
use crate::book::PreparedText;
use crate::resources::MAX_PREPARED_TEXT_BYTES;
use crate::toc::root_href;
use ::mobi::headers::TextEncoding;
use memchr::{memchr_iter, memrchr, memrchr2};
use regex::{Captures, Regex};
use scraper::{ElementRef, Html};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

pub(super) mod index;

struct Patterns {
    pagebreak: Regex,
    guide_end: Regex,
    empty_guide: Regex,
    read_filepos: Regex,
    word_char: Regex,
    image: Regex,
    filepos: Regex,
    fragment: Regex,
    named_anchor: Regex,
}

fn patterns() -> io::Result<&'static Patterns> {
    static PATTERNS: OnceLock<Result<Patterns, String>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            (|| -> Result<Patterns, regex::Error> {
                Ok(Patterns {
                    pagebreak: Regex::new(r"(?i)<mbp:pagebreak\s*/?>")?,
                    guide_end: Regex::new(r"(?i)</guide\s*>")?,
                    empty_guide: Regex::new(r"\A(?i:<guide[\t\n\f\r ]*>\s*</guide[\t\n\f\r ]*>)\z")?,
                    read_filepos: Regex::new(r#"\A(?i:filepos\s*=\s*["']?(\d+))"#)?,
                    word_char: Regex::new(r"\w")?,
                    image: Regex::new(r#"(?i)<img([^>]*?)\srecindex\s*=\s*["']?([0-9]+)["']?([^>]*)>"#)?,
                    filepos: Regex::new(r#"(?i)\bfilepos\s*=\s*["']?(\d+)["']?"#)?,
                    fragment: Regex::new(r#"(?i)\bhref\s*=\s*["']#([^"']+)["']"#)?,
                    named_anchor: Regex::new(r#"(?i)<a\b([^>]*?)\bname\s*=\s*(["'][^"']+["'])([^>]*)>"#)?,
                })
            })()
            .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))
}

fn file_positions(text: &str, patterns: &Patterns) -> BTreeMap<usize, usize> {
    let bytes = text.as_bytes();
    let mut captures = patterns.read_filepos.capture_locations();
    let mut positions = BTreeMap::new();
    for equals in memchr_iter(b'=', bytes) {
        let prefix = text[..equals].trim_end();
        // These letters have only ASCII case variants. Leave the final 's'
        // to the regex, which also accepts its Unicode case-folded variant.
        let Some(start) = prefix.char_indices().next_back().and_then(|(last, _)| last.checked_sub(6)) else {
            continue;
        };
        if !bytes.get(start..start + 6).is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"filepo")) {
            continue;
        }
        // Preserve the original Unicode word boundary without asking a regex
        // to search the entire book. Most attribute boundaries are ASCII.
        if let Some(previous) = text[..start].chars().next_back() {
            if previous.is_ascii() {
                if previous.is_ascii_alphanumeric() || previous == '_' {
                    continue;
                }
            } else if patterns.word_char.is_match(&text[start - previous.len_utf8()..start]) {
                continue;
            }
        }
        let candidate = &text[start..];
        if patterns.read_filepos.captures_read(&mut captures, candidate).is_some() {
            if let Some((begin, end)) = captures.get(1) {
                if let Ok(position) = candidate[begin..end].parse() {
                    positions.insert(position, 0);
                }
            }
        }
    }
    positions
}

fn guide_offset(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    memchr_iter(b'<', bytes).find(|&start| bytes.get(start..start + b"<guide".len()).is_some_and(|tag| tag.eq_ignore_ascii_case(b"<guide")))
}

pub(super) struct ReadingText {
    text: String,
    sections: Vec<Range<usize>>,
    targets: BTreeMap<usize, (usize, usize)>,
    entries: Vec<BookTocEntry>,
    named_targets: Option<HashMap<String, usize>>,
}

impl ReadingText {
    pub(super) fn new(metadata: &MobiMetadata, text: String, navigation: Option<Vec<index::Entry>>) -> io::Result<Self> {
        Self::build(metadata, text, navigation, false)
    }

    pub(super) fn for_inspection(metadata: &MobiMetadata, text: String, navigation: Option<Vec<index::Entry>>) -> io::Result<Self> {
        Self::build(metadata, text, navigation, true)
    }

    fn build(metadata: &MobiMetadata, text: String, navigation: Option<Vec<index::Entry>>, inspection: bool) -> io::Result<Self> {
        let patterns = patterns()?;
        let started = profile_start();
        let pagebreak = &patterns.pagebreak;
        let mut sections = Vec::new();
        let mut start = 0;
        for separator in pagebreak.find_iter(&text) {
            if !text[start..separator.start()].trim().is_empty() {
                sections.push(start..separator.start());
            }
            start = separator.end();
        }
        if !text[start..].trim().is_empty() {
            sections.push(start..text.len());
        }
        if sections.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MOBI contains no readable text"));
        }
        profile_phase("section_boundaries", started);
        let started = profile_start();
        let compiled_only = inspection && navigation.as_ref().is_some_and(|entries| !entries.is_empty());
        let mut positions = if compiled_only { BTreeMap::new() } else { file_positions(&text, patterns) };
        // Prefer the compiled NCX. Legacy books can declare an inline contents
        // page using a guide reference instead.
        if let Some(entries) = &navigation {
            for entry in entries {
                positions.insert(entry.position, 0);
            }
        }
        profile_phase("filepos_collect", started);
        let started = profile_start();
        // filepos counts bytes in the original encoding, before our rewriting.
        // CP1252 has one byte per character; UTF-8 already uses String offsets.
        match metadata.text_encoding() {
            TextEncoding::CP1252 => {
                let mut original = 0;
                let mut utf8 = 0;
                for (position, mapped) in &mut positions {
                    // Count UTF-8 leading bytes in bulk instead of decoding
                    // every character. An interval of N bytes contains at
                    // most N characters, even after completing its last one.
                    while original < *position && utf8 < text.len() {
                        let bytes = (*position - original).min(text.len() - utf8);
                        let end = text.ceil_char_boundary(utf8 + bytes);
                        original += text[utf8..end].chars().count();
                        utf8 = end;
                    }
                    *mapped = utf8;
                }
            }
            _ => {
                for (position, mapped) in &mut positions {
                    *mapped = *position;
                }
            }
        }
        profile_phase("filepos_encoding", started);
        let started = profile_start();
        let mut targets = BTreeMap::new();
        let mut scanned = 0;
        let mut open_tag = None;
        let mut open_entity = None;
        for (position, mut offset) in positions {
            if offset >= text.len() {
                continue;
            }
            while !text.is_char_boundary(offset) {
                offset -= 1;
            }
            // Only the last angle delimiter and ampersand determine the state
            // at this offset. Search within the new interval between targets.
            let interval = &text[scanned..offset];
            if let Some(relative) = memrchr2(b'<', b'>', interval.as_bytes()) {
                open_tag = (interval.as_bytes()[relative] == b'<').then_some(scanned + relative);
            }
            let ampersand = memrchr(b'&', interval.as_bytes());
            let entity_tail = ampersand.map_or(interval, |relative| &interval[relative + 1..]);
            if entity_tail.chars().any(|ch| ch == ';' || ch.is_whitespace()) {
                open_entity = None;
            } else if let Some(relative) = ampersand {
                open_entity = Some(scanned + relative);
            }
            scanned = offset;
            // Do not inject markup into an existing tag or entity.
            offset = open_tag.or(open_entity).unwrap_or(offset);
            let document = sections.partition_point(|range| range.end <= offset);
            if let Some(section) = sections.get(document).filter(|range| range.contains(&offset)) {
                targets.insert(position, (document, offset - section.start));
            }
        }
        profile_phase("filepos_resolve", started);
        let mut reading = Self { text, sections, targets, entries: Vec::new(), named_targets: None };
        reading.entries = navigation.map(|entries| reading.index_toc(entries)).unwrap_or_default();
        if reading.entries.is_empty() {
            if compiled_only {
                // An unusable compiled TOC still gets the complete inline
                // fallback, including every file-position link it may need.
                return Self::build(metadata, reading.text, None, false);
            }
            let started = profile_start();
            reading.entries = reading.inline_toc();
            profile_phase("inline_toc", started);
        }
        Ok(reading)
    }

    fn target(&self, position: usize) -> Option<String> {
        let (document, _) = self.targets.get(&position)?;
        Some(root_href(format!("mobi/sections/{document}.html#filepos{position}")))
    }

    fn index_toc(&self, entries: Vec<index::Entry>) -> Vec<BookTocEntry> {
        crate::toc::nest_entries(entries.into_iter().filter_map(|entry| {
            let link = self.target(entry.position)?;
            Some((entry.level, BookTocEntry { title: entry.title, target: link, children: Vec::new() }))
        }))
    }

    fn named_target(&self, id: &str) -> Option<String> {
        let document = self.named_targets.as_ref()?.get(id)?;
        Some(root_href(format!("mobi/sections/{document}.html#{id}")))
    }

    fn collect_named_targets(&mut self) {
        if self.named_targets.is_some() {
            return;
        }
        let started = profile_start();
        let mut targets = HashMap::new();
        for (index, section) in self.sections.iter().enumerate() {
            let html = Html::parse_fragment(&self.text[section.clone()]);
            for element in html.root_element().descendants().filter_map(ElementRef::wrap) {
                if let Some(id) = element.value().attr("id").or_else(|| (element.value().name() == "a").then(|| element.value().attr("name")).flatten()) {
                    targets.entry(id.to_owned()).or_insert(index);
                }
            }
        }
        self.named_targets = Some(targets);
        profile_phase("named_targets", started);
    }

    fn inline_toc(&mut self) -> Vec<BookTocEntry> {
        // Without a guide element there is no inline contents reference. Avoid
        // constructing a full-book HTML tree just to discover its absence.
        let Some(guide_start) = guide_offset(&self.text) else {
            return Vec::new();
        };
        // MOBI guides normally live at the start of the document and point to
        // a page-break-delimited contents section. Parse that closed guide and
        // its target section without constructing a tree for the whole book.
        // Named anchors and guides outside this bounded prefix use the complete
        // document below, which preserves their container and lookup behavior.
        let prefix_end = self.text.floor_char_boundary(self.text.len().min(64 * 1024));
        if let Some((patterns, end)) = patterns().ok().and_then(|patterns| patterns.guide_end.find(&self.text[..prefix_end]).map(|end| (patterns, end.end()))) {
            let guide = Html::parse_document(&self.text[..end]);
            if let Some(range) =
                contents_reference(&guide).and_then(|contents| contents.value().attr("filepos")).and_then(|value| value.parse().ok()).and_then(|position| self.targets.get(&position)).map(|(document, _)| self.sections[*document].clone())
            {
                return self.html_toc(Html::parse_fragment(&self.text[range]));
            }
            // A real, explicitly empty guide cannot declare a contents page.
            // Check the source as well as the parsed prefix: a closing-guide
            // string inside an attribute or comment must not hide references.
            // Another guide later in the book still needs the full fallback.
            if guide_start < end
                && patterns.empty_guide.is_match(&self.text[guide_start..end])
                && guide.root_element().descendants().filter_map(ElementRef::wrap).any(|element| element.value().name() == "guide")
                && guide_offset(&self.text[end..]).is_none()
            {
                return Vec::new();
            }
        }
        let started = profile_start();
        let document = Html::parse_document(&self.text);
        profile_phase("full_html_tree", started);
        let Some(contents) = contents_reference(&document) else {
            return Vec::new();
        };
        let position = contents.value().attr("filepos").and_then(|value| value.parse().ok());
        let range = position.and_then(|position| self.targets.get(&position)).map(|(document, _)| self.sections[*document].clone());
        let html = if let Some(range) = range {
            Html::parse_fragment(&self.text[range])
        } else if let Some(id) = contents.value().attr("href").and_then(|href| href.strip_prefix('#')) {
            let Some(element) = document
                .root_element()
                .descendants()
                .filter_map(ElementRef::wrap)
                .filter(|element| element.value().attr("id").is_some() || (element.value().name() == "a" && element.value().attr("name").is_some()))
                .find(|element| element.value().attr("id") == Some(id) || element.value().attr("name") == Some(id))
            else {
                return Vec::new();
            };
            // Guides using named anchors typically enclose the TOC in a div.
            let container = std::iter::once(element).chain(element.ancestors().filter_map(scraper::ElementRef::wrap)).find(|element| matches!(element.value().name(), "div" | "nav" | "section" | "body")).unwrap_or(element);
            Html::parse_fragment(&container.inner_html())
        } else {
            return Vec::new();
        };
        self.html_toc(html)
    }

    fn html_toc(&mut self, html: Html) -> Vec<BookTocEntry> {
        // Only an inline contents page can need named targets during inspection.
        // Compiled navigation already resolves through the file-position map.
        if html.root_element().descendants().filter_map(ElementRef::wrap).any(|element| element.value().attr("href").is_some_and(|href| href.starts_with('#'))) {
            self.collect_named_targets();
        }
        crate::toc::html_entries(html.root_element(), |element| {
            element
                .value()
                .attr("filepos")
                .and_then(|value| value.parse().ok())
                .and_then(|position| self.target(position))
                .or_else(|| element.value().attr("href").and_then(|href| href.strip_prefix('#')).and_then(|id| self.named_target(id)))
        })
    }

    pub(super) fn toc(&self) -> Option<Vec<BookTocEntry>> {
        (!self.entries.is_empty()).then(|| self.entries.clone())
    }

    pub(super) fn prepare(mut self) -> io::Result<PreparedText> {
        // Reading rewrites named links throughout the book, even when the
        // contents table itself used only compiled file positions.
        self.collect_named_targets();
        let patterns = patterns()?;
        let image = &patterns.image;
        let filepos = &patterns.filepos;
        let fragment = &patterns.fragment;
        let named_anchor = &patterns.named_anchor;
        let mut resources = HashMap::new();
        let mut documents = Vec::new();
        let mut weights = Vec::new();
        let mut total = 0;
        for (index, range) in self.sections.iter().enumerate() {
            let mut section = self.text[range.clone()].to_owned();
            let mut insertions = self.targets.iter().filter_map(|(position, (document, offset))| (*document == index).then_some((*offset, *position))).collect::<Vec<_>>();
            insertions.sort_unstable();
            for (offset, position) in insertions.into_iter().rev() {
                section.insert_str(offset, &format!("<a id=\"filepos{position}\"></a>"));
            }
            let section = filepos.replace_all(&section, |capture: &Captures<'_>| {
                capture[1].parse().ok().and_then(|position| self.target(position)).map(|target| format!("href=\"{}\"", target.rsplit_once('/').map_or(target.as_str(), |(_, name)| name))).unwrap_or_default()
            });
            let section = fragment.replace_all(&section, |capture: &Captures<'_>| {
                self.named_target(&capture[1]).map(|target| format!("href=\"{}\"", target.rsplit_once('/').map_or(target.as_str(), |(_, name)| name))).unwrap_or_else(|| capture[0].to_owned())
            });
            let section =
                named_anchor.replace_all(&section, |capture: &Captures<'_>| if capture[1].contains("id=") || capture[3].contains("id=") { capture[0].to_owned() } else { format!("<a{} id={}{}>", &capture[1], &capture[2], &capture[3]) });
            let section = image.replace_all(&section, |capture: &Captures<'_>| {
                let index = capture[2].parse::<usize>().unwrap_or(0);
                format!("<img{} src=\"../resources/{index}\"{}>", &capture[1], &capture[3])
            });
            let html = format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>{section}</body></html>").into_bytes();
            total += html.len();
            if html.len() as u64 > MAX_BINARY_RESOURCE_BYTES || total as u64 > MAX_PREPARED_TEXT_BYTES {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "MOBI prepared text exceeds the byte limit"));
            }
            let uri = format!("mobi/sections/{index}.html");
            weights.push(html.len() as u64);
            resources.insert(uri.clone(), html);
            documents.push(uri);
        }
        Ok(PreparedText { resources: PreparedResources::new(resources, None, documents), document_weights: weights, toc: self.entries })
    }
}

fn contents_reference(document: &Html) -> Option<ElementRef<'_>> {
    document
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
        .filter(|element| element.value().name() == "reference" && element.ancestors().filter_map(ElementRef::wrap).any(|ancestor| ancestor.value().name() == "guide"))
        .find(|element| element.value().attr("type").is_some_and(|kind| kind.split_whitespace().any(|kind| kind.eq_ignore_ascii_case("toc"))))
}
