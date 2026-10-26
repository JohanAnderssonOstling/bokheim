//! Convert MOBI navigation offsets and links into prepared document anchors.

use super::*;
use crate::TocEntry;
use crate::book::PreparedText;
use crate::resources::MAX_PREPARED_TEXT_BYTES;
use crate::toc::root_href;
use ::mobi::headers::TextEncoding;
use regex::{Captures, Regex};
use scraper::{ElementRef, Html};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

pub(super) mod index;

struct Patterns {
    pagebreak: Regex,
    read_filepos: Regex,
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
                    read_filepos: Regex::new(r#"(?i)\bfilepos\s*=\s*["']?(\d+)"#)?,
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

pub(super) struct ReadingText {
    text: String,
    sections: Vec<Range<usize>>,
    targets: BTreeMap<usize, (usize, usize)>,
    entries: Vec<TocEntry>,
    named_targets: HashMap<String, usize>,
}

impl ReadingText {
    pub(super) fn new(metadata: &MobiMetadata, text: String, navigation: Option<Vec<index::Entry>>) -> io::Result<Self> {
        let patterns = patterns()?;
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
        let filepos = &patterns.read_filepos;
        let mut positions: BTreeMap<usize, usize> = filepos.captures_iter(&text).filter_map(|capture| capture[1].parse().ok().map(|position| (position, 0))).collect();
        // Prefer the compiled NCX. Legacy books can declare an inline contents
        // page using a guide reference instead.
        if let Some(entries) = &navigation {
            for entry in entries {
                positions.insert(entry.position, 0);
            }
        }
        // filepos counts bytes in the original encoding, before our rewriting.
        // CP1252 has one byte per character; UTF-8 already uses String offsets.
        match metadata.text_encoding() {
            TextEncoding::CP1252 => {
                let mut requested = positions.iter_mut().peekable();
                for (original, (utf8, _)) in text.char_indices().enumerate() {
                    while let Some((_, mapped)) = requested.next_if(|(position, _)| **position <= original) {
                        *mapped = utf8;
                    }
                }
                for (_, mapped) in requested {
                    *mapped = text.len();
                }
            }
            _ => {
                for (position, mapped) in &mut positions {
                    *mapped = *position;
                }
            }
        }
        let mut targets = BTreeMap::new();
        for (position, mut offset) in positions {
            if offset >= text.len() {
                continue;
            }
            while !text.is_char_boundary(offset) {
                offset -= 1;
            }
            // Do not inject markup into an existing tag or entity.
            let prefix = &text[..offset];
            if let Some(tag) = prefix.rfind('<')
                && prefix.rfind('>').is_none_or(|end| tag > end)
            {
                offset = tag;
            } else if let Some(entity) = prefix.rfind('&')
                && prefix.rfind(';').is_none_or(|end| entity > end)
                && !prefix[entity..].contains(char::is_whitespace)
            {
                offset = entity;
            }
            if let Some((document, section)) = sections.iter().enumerate().find(|(_, range)| range.contains(&offset)) {
                targets.insert(position, (document, offset - section.start));
            }
        }
        let mut named_targets = HashMap::new();
        for (index, section) in sections.iter().enumerate() {
            let html = Html::parse_fragment(&text[section.clone()]);
            for element in html.root_element().descendants().filter_map(ElementRef::wrap).filter(|element| element.value().attr("id").is_some() || (element.value().name() == "a" && element.value().attr("name").is_some())) {
                if let Some(id) = element.value().attr("id").or_else(|| element.value().attr("name")) {
                    named_targets.entry(id.to_owned()).or_insert(index);
                }
            }
        }
        let mut reading = Self { text, sections, targets, entries: Vec::new(), named_targets };
        reading.entries = navigation.map(|entries| reading.index_toc(entries)).unwrap_or_default();
        if reading.entries.is_empty() {
            reading.entries = reading.inline_toc();
        }
        Ok(reading)
    }

    fn target(&self, position: usize) -> Option<String> {
        let (document, _) = self.targets.get(&position)?;
        Some(root_href(format!("mobi/sections/{document}.html#filepos{position}")))
    }

    fn index_toc(&self, entries: Vec<index::Entry>) -> Vec<TocEntry> {
        crate::toc::nest_entries(entries.into_iter().filter_map(|entry| {
            let link = self.target(entry.position)?;
            Some((entry.level, TocEntry { title: entry.title, link, children: Vec::new() }))
        }))
    }

    fn named_target(&self, id: &str) -> Option<String> {
        let document = self.named_targets.get(id)?;
        Some(root_href(format!("mobi/sections/{document}.html#{id}")))
    }

    fn inline_toc(&self) -> Vec<TocEntry> {
        let document = Html::parse_document(&self.text);
        let Some(contents) = document
            .root_element()
            .descendants()
            .filter_map(ElementRef::wrap)
            .filter(|element| element.value().name() == "reference" && element.ancestors().filter_map(ElementRef::wrap).any(|ancestor| ancestor.value().name() == "guide"))
            .find(|element| element.value().attr("type").is_some_and(|kind| kind.split_whitespace().any(|kind| kind.eq_ignore_ascii_case("toc"))))
        else {
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
        crate::toc::html_entries(html.root_element(), |element| {
            element
                .value()
                .attr("filepos")
                .and_then(|value| value.parse().ok())
                .and_then(|position| self.target(position))
                .or_else(|| element.value().attr("href").and_then(|href| href.strip_prefix('#')).and_then(|id| self.named_target(id)))
        })
    }

    pub(super) fn toc(&self) -> Option<Vec<TocEntry>> {
        (!self.entries.is_empty()).then(|| self.entries.clone())
    }

    pub(super) fn prepare(self) -> io::Result<PreparedText> {
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
