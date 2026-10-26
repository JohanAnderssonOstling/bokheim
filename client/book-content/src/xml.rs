//! Small bounded XML tree for FB2 and ComicInfo. Never resolves DTDs/entities.

use encoding_rs::{Encoding, UTF_8};
use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};
use std::{collections::HashMap, io};

#[derive(Clone, Debug)]
pub(crate) enum Content {
    Text(String),
    Element(Node),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Node {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub content: Vec<Content>,
}

pub(crate) fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl Node {
    pub fn children(&self) -> impl Iterator<Item = &Node> {
        self.content.iter().filter_map(|item| match item {
            Content::Element(node) => Some(node),
            _ => None,
        })
    }
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children().find(|node| node.name == name)
    }
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
    pub fn text(&self) -> String {
        let mut text = String::new();
        for item in &self.content {
            match item {
                Content::Text(value) => text.push_str(value),
                Content::Element(node) => text.push_str(&node.text()),
            }
        }
        text
    }
    pub fn field(&self, name: &str) -> String {
        self.child(name).map(|node| node.text().split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default()
    }
}

pub(crate) fn parse(bytes: &[u8], limit: usize) -> io::Result<Node> {
    if bytes.len() > limit {
        return Err(invalid("XML exceeds the size limit"));
    }
    let (encoding, bom) = Encoding::for_bom(bytes).unwrap_or((UTF_8, 0));
    let encoding = if bom == 0 {
        let mut declaration = Reader::from_reader(bytes);
        match declaration.read_event() {
            Ok(Event::Decl(value)) => match value.encoding().transpose().map_err(io::Error::other)? {
                Some(label) => Encoding::for_label(&label).ok_or_else(|| invalid("unsupported XML encoding"))?,
                None => UTF_8,
            },
            _ => UTF_8,
        }
    } else {
        encoding
    };
    let (text, errors) = encoding.decode_without_bom_handling(&bytes[bom..]);
    if errors || text.len() > limit {
        return Err(invalid("invalid or oversized XML text"));
    }
    let mut reader = Reader::from_str(&text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let mut count = 0;
    loop {
        let event = reader.read_event().map_err(io::Error::other)?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                count += 1;
                if stack.len() >= 64 || count > 1_000_000 {
                    return Err(invalid("XML structure exceeds the limit"));
                }
                let node = element(&start)?;
                if empty {
                    append(node, &mut stack, &mut root)?;
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| invalid("unexpected XML end tag"))?;
                append(node, &mut stack, &mut root)?;
            }
            Event::Text(value) => {
                let value = value.unescape().map_err(io::Error::other)?.into_owned();
                if let Some(node) = stack.last_mut() {
                    node.content.push(Content::Text(value));
                } else if !value.trim().is_empty() {
                    return Err(invalid("text outside XML root"));
                }
            }
            Event::CData(value) => {
                let value = String::from_utf8(value.into_inner().into_owned()).map_err(io::Error::other)?;
                stack.last_mut().ok_or_else(|| invalid("CDATA outside XML root"))?.content.push(Content::Text(value));
            }
            Event::DocType(_) => return Err(invalid("XML document types are unsupported")),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(invalid("truncated XML document"));
    }
    root.ok_or_else(|| invalid("XML has no root element"))
}

fn element(start: &BytesStart<'_>) -> io::Result<Node> {
    let name = String::from_utf8(start.local_name().as_ref().to_vec()).map_err(io::Error::other)?;
    let mut attrs = HashMap::new();
    for attr in start.attributes() {
        let attr = attr.map_err(io::Error::other)?;
        let name = String::from_utf8(attr.key.as_ref().to_vec()).map_err(io::Error::other)?;
        // Keep xmlns intact; attribute consumers use local names for xlink/xml.
        let name = if name.starts_with("xmlns") { name } else { name.rsplit(':').next().unwrap_or(&name).to_owned() };
        attrs.insert(name, attr.unescape_value().map_err(io::Error::other)?.into_owned());
    }
    Ok(Node { name, attrs, content: Vec::new() })
}

fn append(node: Node, stack: &mut [Node], root: &mut Option<Node>) -> io::Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.content.push(Content::Element(node));
    } else if root.replace(node).is_some() {
        return Err(invalid("multiple XML root elements"));
    }
    Ok(())
}
