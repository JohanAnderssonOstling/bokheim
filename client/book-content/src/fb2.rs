//! FictionBook XML mapped to the common HTML, metadata and resource interfaces.

use crate::book::{PreparedText, ResourceSource};
use crate::epub::archive::ArchiveReader;
use crate::resources::{MAX_BINARY_RESOURCE_BYTES, MAX_PREPARED_TEXT_BYTES};
use crate::source::Source;
use crate::xml::{self, Content, Node, invalid};
use crate::{CreatorRole, DeclaredMetadata, MetadataCredit, MetadataDate, MetadataProperty, MetadataSubject, PreparedResources, ReadingBook, TocEntry};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_lite::io::{AsyncReadExt, AsyncSeekExt};
use std::{
    collections::HashMap,
    io::{self, SeekFrom},
    sync::Arc,
};

const MAX_FB2_BYTES: u64 = 96 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 24 * 1024 * 1024;
const FB2_NAMESPACE: &str = "http://www.gribuser.ru/xml/fictionbook/2.0";

pub(crate) struct Fb2Container {
    root: Node,
    metadata: DeclaredMetadata,
    resources: Arc<Fb2Resources>,
    cover: Option<String>,
}

pub(crate) struct Fb2Resources {
    images: HashMap<String, String>,
    media_types: HashMap<String, String>,
    languages: Vec<String>,
}

impl Fb2Resources {
    pub(crate) fn languages(&self) -> Vec<String> {
        self.languages.clone()
    }

    pub(crate) fn read_bytes(&self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        let encoded = self.images.get(uri).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "FB2 image not found"))?;
        let limit = limit.min(MAX_BINARY_RESOURCE_BYTES);
        if encoded.len() as u64 > limit.div_ceil(3) * 4 {
            return Err(invalid("FB2 image exceeds the size limit"));
        }
        let bytes = STANDARD.decode(encoded).map_err(|error| invalid(error.to_string()))?;
        if bytes.len() as u64 > limit {
            return Err(invalid("FB2 image exceeds the size limit"));
        }
        Ok(bytes)
    }
}

impl Fb2Container {
    pub(crate) async fn open(mut source: Source) -> io::Result<Self> {
        source.reader.seek(SeekFrom::Start(0)).await?;
        let mut signature = [0; 4];
        source.reader.read_exact(&mut signature).await?;
        source.reader.seek(SeekFrom::Start(0)).await?;
        let bytes = if signature == *b"PK\x03\x04" {
            let mut archive = ArchiveReader::open_with_limit(source, MAX_FB2_BYTES).await?;
            let names: Vec<_> = archive.metadata.names().iter().filter(|name| name.to_ascii_lowercase().ends_with(".fb2") && !name.starts_with("__MACOSX/")).cloned().collect();
            if names.len() != 1 {
                return Err(invalid("FB2 archive must contain exactly one FictionBook document"));
            }
            archive.read(&names[0], MAX_FB2_BYTES).await?
        } else {
            let mut bytes = Vec::new();
            source.reader.take(MAX_FB2_BYTES + 1).read_to_end(&mut bytes).await?;
            if bytes.len() as u64 > MAX_FB2_BYTES {
                return Err(invalid("FB2 exceeds the size limit"));
            }
            bytes
        };
        let mut root = xml::parse(&bytes, MAX_FB2_BYTES as usize)?;
        if root.name != "FictionBook" || !root.attrs.iter().any(|(key, value)| key.starts_with("xmlns") && value == FB2_NAMESPACE) {
            return Err(invalid("expected a FictionBook 2 document"));
        }
        let metadata = declared_metadata(&root);
        let mut images = HashMap::new();
        let mut media_types = HashMap::new();
        let mut image_ids = HashMap::new();
        root.content.retain_mut(|item| {
            let Content::Element(node) = item else { return true };
            if node.name != "binary" {
                return true;
            }
            if let Some(id) = node.attr("id") {
                // Generated paths cannot escape the container or masquerade as URLs.
                let extension = match node.attr("content-type") {
                    Some("image/jpeg") => "jpg",
                    Some("image/png") => "png",
                    Some("image/svg+xml") => "svg",
                    Some("image/gif") => "gif",
                    Some("image/webp") => "webp",
                    _ => "bin",
                };
                let uri = format!("fb2/images/{}.{}", images.len(), extension);
                if !image_ids.contains_key(id) {
                    image_ids.insert(id.to_owned(), uri.clone());
                    media_types.insert(uri.clone(), node.attr("content-type").unwrap_or("application/octet-stream").to_owned());
                    images.insert(uri, node.text().chars().filter(|ch| !ch.is_ascii_whitespace()).collect());
                }
            }
            false
        });
        let cover = root
            .child("description")
            .and_then(|node| node.child("title-info"))
            .and_then(|node| node.child("coverpage"))
            .and_then(|node| node.child("image"))
            .and_then(|node| node.attr("href"))
            .and_then(|href| href.strip_prefix('#'))
            .and_then(|id| image_ids.get(id))
            .cloned();
        rewrite_images(&mut root, &image_ids);
        let resources = Arc::new(Fb2Resources { images, media_types, languages: metadata.languages.clone() });
        Ok(Self { root, metadata, resources, cover })
    }

    pub(crate) fn metadata(&self) -> DeclaredMetadata {
        self.metadata.clone()
    }
    pub(crate) fn covers(&self) -> Vec<String> {
        self.cover.iter().cloned().collect()
    }
    pub(crate) fn images(&self) -> Vec<String> {
        let mut names: Vec<_> = self.resources.images.keys().cloned().collect();
        names.sort();
        names
    }
    pub(crate) fn read_resource(&self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        self.resources.read_bytes(uri, limit)
    }
    pub(crate) fn prepare_text(&self) -> io::Result<PreparedText> {
        prepare_text(&self.root, self.resources.media_types.clone())
    }
    pub(crate) fn toc(&self) -> io::Result<Vec<TocEntry>> {
        Ok(self.prepare_text()?.toc)
    }
    pub(crate) fn prepare_reading(self) -> io::Result<ReadingBook> {
        Ok(self.prepare_text()?.into_reading(ResourceSource::Fb2(self.resources)))
    }
}

fn property(metadata: &mut DeclaredMetadata, key: &str, value: String) {
    if !value.trim().is_empty() {
        metadata.properties.push(MetadataProperty { id: None, property: format!("fb2:{key}"), value, refines: None, scheme: None, language: None });
    }
}

fn declared_metadata(root: &Node) -> DeclaredMetadata {
    let mut metadata = DeclaredMetadata::default();
    let Some(description) = root.child("description") else { return metadata };
    if let Some(info) = description.child("title-info") {
        metadata.title = info.field("book-title");
        metadata.description = info.child("annotation").map(|node| node.children().map(Node::text).collect::<Vec<_>>().join("\n\n")).unwrap_or_default();
        for node in info.children() {
            match node.name.as_str() {
                "author" | "translator" => {
                    let name = [node.field("first-name"), node.field("middle-name"), node.field("last-name")].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");
                    let name = if name.is_empty() { node.field("nickname") } else { name };
                    if !name.is_empty() {
                        let credit = MetadataCredit { id: None, name, file_as: None, role: CreatorRole::new(if node.name == "author" { "aut" } else { "trl" }) };
                        if node.name == "author" {
                            metadata.creators.push(credit);
                        } else {
                            metadata.contributors.push(credit);
                        }
                    }
                }
                "lang" => {
                    let language = node.text().trim().to_owned();
                    if !language.is_empty() {
                        metadata.languages.push(language);
                    }
                }
                "genre" => metadata.subjects.push(MetadataSubject { id: None, value: node.text(), source: "fb2:genre".into(), authority: None, code: None }),
                "date" => metadata.dates.push(MetadataDate { source: "fb2:title-info:date".into(), id: None, value: node.attr("value").map(str::to_owned).unwrap_or_else(|| node.text()), event: Some("creation".into()) }),
                "sequence" => {
                    property(&mut metadata, "sequence:name", node.attr("name").unwrap_or_default().to_owned());
                    property(&mut metadata, "sequence:number", node.attr("number").unwrap_or_default().to_owned());
                }
                _ => {}
            }
        }
    }
    if let Some(info) = description.child("publish-info") {
        // These fields describe the source print edition, not the ebook.
        for key in ["book-name", "publisher", "city", "year", "isbn"] {
            property(&mut metadata, &format!("print:{key}"), info.field(key));
        }
    }
    if let Some(info) = description.child("document-info") {
        for key in ["id", "version", "program-used"] {
            property(&mut metadata, &format!("document:{key}"), info.field(key));
        }
        for node in info.children().filter(|node| node.name == "src-url") {
            metadata.sources.push(node.text());
        }
    }
    metadata
}

fn rewrite_images(node: &mut Node, images: &HashMap<String, String>) {
    if node.name == "image" {
        if let Some(uri) = node.attr("href").and_then(|href| href.strip_prefix('#')).and_then(|id| images.get(id)).cloned() {
            node.attrs.insert("src".into(), uri);
        }
    }
    for item in &mut node.content {
        if let Content::Element(node) = item {
            rewrite_images(node, images);
        }
    }
}

fn prepare_text(root: &Node, mut types: HashMap<String, String>) -> io::Result<PreparedText> {
    let mut documents = Vec::new();
    let mut nodes = Vec::new();
    let mut notes = Vec::new();
    for body in root.children().filter(|node| node.name == "body") {
        let is_notes = body.attr("name") == Some("notes");
        let mut prelude = Vec::new();
        for node in body.children() {
            if node.name == "section" {
                if !prelude.is_empty() {
                    nodes.push(Node { name: "div".into(), attrs: HashMap::new(), content: std::mem::take(&mut prelude) });
                    notes.push(is_notes);
                }
                let mut section = node.clone();
                if is_notes {
                    section.attrs.insert("data-fb2-note".into(), "true".into());
                }
                nodes.push(section);
                notes.push(is_notes);
            } else {
                prelude.push(Content::Element(node.clone()));
            }
        }
        if !prelude.is_empty() {
            nodes.push(Node { name: "div".into(), attrs: HashMap::new(), content: prelude });
            notes.push(is_notes);
        }
    }
    if nodes.is_empty() || nodes.len() > 8192 {
        return Err(invalid("FB2 contains no readable sections or too many sections"));
    }
    let mut links = HashMap::new();
    let mut counter = 0;
    for (index, node) in nodes.iter_mut().enumerate() {
        let uri = format!("fb2/section-{index}.xhtml");
        index_links(node, &uri, &mut links, &mut counter);
        documents.push(uri);
    }
    let mut bytes = HashMap::new();
    let mut weights = Vec::new();
    let mut toc = Vec::new();
    let mut total = 0;
    for ((node, uri), is_notes) in nodes.iter().zip(&documents).zip(notes) {
        let mut html = String::from("<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><meta charset=\"utf-8\"/></head><body>");
        render(node, &links, 0, false, &mut html)?;
        html.push_str("</body></html>");
        total += html.len() as u64;
        if total > MAX_PREPARED_TEXT_BYTES {
            return Err(invalid("FB2 prepared text exceeds the size limit"));
        }
        weights.push(html.len() as u64);
        types.insert(uri.clone(), "application/xhtml+xml".into());
        bytes.insert(uri.clone(), html.into_bytes());
        if !is_notes {
            collect_toc(node, uri, &mut toc);
        }
    }
    Ok(PreparedText { resources: PreparedResources::new(bytes, Some(types), documents), document_weights: weights, toc })
}

fn index_links(node: &mut Node, uri: &str, links: &mut HashMap<String, String>, counter: &mut usize) {
    if node.name == "section" || node.attr("id").is_some() {
        let generated = format!("fb2-node-{counter}");
        *counter += 1;
        if let Some(id) = node.attr("id") {
            links.entry(id.to_owned()).or_insert_with(|| format!("/{uri}#{generated}"));
        }
        node.attrs.insert("data-fb2-id".into(), generated);
    }
    for item in &mut node.content {
        if let Content::Element(child) = item {
            index_links(child, uri, links, counter);
        }
    }
}

fn collect_toc(node: &Node, uri: &str, entries: &mut Vec<TocEntry>) {
    if node.name == "section" {
        let title = node.field("title");
        if !title.is_empty() {
            let mut children = Vec::new();
            for child in node.children().filter(|node| node.name == "section") {
                collect_toc(child, uri, &mut children);
            }
            entries.push(TocEntry { title, link: format!("/{uri}#{}", node.attr("data-fb2-id").unwrap_or_default()), children });
            return;
        }
    }
    for child in node.children() {
        collect_toc(child, uri, entries);
    }
}

fn escaped(text: &str, html: &mut String) {
    html.push_str(&quick_xml::escape::escape(text));
}

fn render(node: &Node, links: &HashMap<String, String>, depth: usize, in_title: bool, html: &mut String) -> io::Result<()> {
    let tag = match node.name.as_str() {
        "section" => {
            if node.attr("data-fb2-note").is_some() {
                "aside"
            } else {
                "section"
            }
        }
        "title" => {
            if depth > 1 {
                "h3"
            } else {
                "h2"
            }
        }
        "subtitle" => "h4",
        "p" if in_title => "span",
        "p" | "strong" | "sub" | "sup" | "code" | "table" | "tr" | "td" | "th" | "a" => &node.name,
        "emphasis" => "em",
        "strikethrough" => "s",
        "cite" | "epigraph" => "blockquote",
        "empty-line" => "br",
        "image" => "img",
        "v" => "p",
        _ => "div",
    };
    html.push('<');
    html.push_str(tag);
    if tag == "aside" {
        html.push_str(" role=\"doc-footnote\" epub:type=\"footnote\"");
    }
    if let Some(id) = node.attr("data-fb2-id") {
        html.push_str(" id=\"");
        escaped(id, html);
        html.push('"');
    }
    if let Some(language) = node.attr("lang") {
        html.push_str(" lang=\"");
        escaped(language, html);
        html.push('"');
    }
    if tag == "img" {
        if let Some(src) = node.attr("src") {
            html.push_str(" src=\"/");
            escaped(src, html);
            html.push('"');
        }
        html.push_str(" alt=\"\"/>");
    } else {
        if tag == "a" {
            if let Some(target) = node.attr("href").and_then(|href| href.strip_prefix('#')).and_then(|id| links.get(id)) {
                html.push_str(" href=\"");
                escaped(target, html);
                html.push('"');
                if node.attr("type") == Some("note") {
                    html.push_str(" epub:type=\"noteref\"");
                }
            }
        }
        if matches!(tag, "td" | "th") {
            for name in ["colspan", "rowspan", "align"] {
                if let Some(value) = node.attr(name) {
                    html.push(' ');
                    html.push_str(name);
                    html.push_str("=\"");
                    escaped(value, html);
                    html.push('"');
                }
            }
        }
        if node.name == "poem" || node.name == "stanza" {
            html.push_str(" style=\"white-space:pre-wrap\"");
        }
        if tag == "br" {
            html.push_str("/>");
            return Ok(());
        }
        html.push('>');
        for item in &node.content {
            match item {
                Content::Text(text) => escaped(text, html),
                Content::Element(child) => render(child, links, depth + usize::from(node.name == "section"), in_title || node.name == "title", html)?,
            }
            if html.len() > MAX_DOCUMENT_BYTES {
                return Err(invalid("FB2 section exceeds the document limit"));
            }
        }
        if tag != "br" {
            html.push_str("</");
            html.push_str(tag);
            html.push('>');
        }
    }
    Ok(())
}
