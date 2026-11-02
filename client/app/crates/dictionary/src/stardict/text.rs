use scraper::{Html, node::Node};

use crate::Result;

/// Strip markup while preserving paragraph breaks and decoding entities.
pub(crate) fn html_text(text: &str) -> String {
    let fragment = Html::parse_fragment(text);
    let mut output = String::new();
    let mut nodes = vec![fragment.tree.root()];
    while let Some(node) = nodes.pop() {
        if node.value().as_element().is_some_and(|element| matches!(element.name(), "script" | "style")) {
            continue;
        }
        match node.value() {
            Node::Text(text) => output.push_str(text),
            Node::Element(element) if matches!(element.name(), "br" | "p" | "div" | "li" | "h1" | "h2" | "h3" | "tr" | "dt" | "dd") => output.push('\n'),
            _ => {}
        }
        nodes.extend(node.children().rev());
    }
    output.lines().map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
}

pub(crate) fn definition(bytes: &[u8], types: &str) -> Result<String> {
    let mut cursor = 0;
    let mut output = Vec::new();
    let sequence = types.as_bytes();
    let mut index = 0;
    while cursor < bytes.len() {
        let kind = if sequence.is_empty() {
            let kind = bytes[cursor];
            cursor += 1;
            kind
        } else {
            *sequence.get(index).ok_or("Dictionary entry has trailing fields")?
        };
        if !kind.is_ascii_alphabetic() {
            return Err("Invalid dictionary entry field".into());
        }
        let last = !sequence.is_empty() && index + 1 == sequence.len();
        let data = if last {
            let data = &bytes[cursor..];
            cursor = bytes.len();
            data
        } else if kind.is_ascii_lowercase() {
            let length = bytes[cursor..].iter().position(|byte| *byte == 0).ok_or("Unterminated dictionary text field")?;
            let data = &bytes[cursor..cursor + length];
            cursor += length + 1;
            data
        } else {
            let length = bytes.get(cursor..cursor + 4).ok_or("Truncated dictionary binary field")?;
            let length = u32::from_be_bytes(length.try_into().map_err(|_| "Invalid field length")?) as usize;
            cursor += 4;
            let data = bytes.get(cursor..cursor + length).ok_or("Truncated dictionary binary field")?;
            cursor += length;
            data
        };
        if kind.is_ascii_lowercase() && !matches!(kind, b'r') {
            let value = std::str::from_utf8(data).map_err(|_| "Dictionary definition is not UTF-8")?.trim_end_matches('\0');
            if !value.is_empty() {
                output.push(if matches!(kind, b'h' | b'g' | b'x' | b'k') { html_text(value) } else { value.to_owned() });
            }
        }
        index += 1;
    }
    Ok(output.join("\n"))
}
