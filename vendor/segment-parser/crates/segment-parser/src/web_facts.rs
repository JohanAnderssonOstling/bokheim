use std::ops::Range;

use structure::SegmentTree;

use crate::node_facts::{EmailFactKind, NodeFactRef, UrlFactKind};

use crate::parser::web::{expanded_url_span, url_prefix_len, valid_email_domain, valid_email_local};
use crate::stream_util::trim_trailing_punctuation;

pub type SegmentUrlFactRef<'a> = NodeFactRef<'a, UrlFactKind>;
pub type SegmentEmailFactRef<'a> = NodeFactRef<'a, EmailFactKind>;

pub fn url_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentUrlFactRef<'_>> + Clone { tree.preorder_iter().filter_map(NodeFactRef::new) }
pub fn email_facts(tree: &SegmentTree) -> impl Iterator<Item = SegmentEmailFactRef<'_>> + Clone { tree.preorder_iter().filter_map(NodeFactRef::new) }

pub fn detect_urls(text: &str) -> Vec<Range<usize>> { detected_spans(text, detect_url_at) }
pub fn detect_emails(text: &str) -> Vec<Range<usize>> { detected_spans(text, detect_email_at) }

pub fn url_facts_value_equivalent(left: &SegmentUrlFactRef<'_>, right: &SegmentUrlFactRef<'_>) -> bool { comparable_url_bytes(left.text()).eq(comparable_url_bytes(right.text())) }
pub fn email_facts_value_equivalent(left: &SegmentEmailFactRef<'_>, right: &SegmentEmailFactRef<'_>) -> bool { left.text() == right.text() }

fn detected_spans(text: &str, detect: fn(&str, usize) -> Option<Range<usize>>) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut index = 0usize;
    while index < text.len() {
        if let Some(span) = detect(text, index) {
            index = span.end;
            spans.push(span);
            continue;
        }
        index += text[index..].chars().next().map(char::len_utf8).unwrap_or(1);
    }
    spans
}

fn detect_url_at(text: &str, start: usize) -> Option<Range<usize>> {
    let rest = &text[start..];
    let prefix_len = url_prefix_len(rest)?;
    let end = rest.char_indices().take_while(|(_, character)| !character.is_whitespace() && !matches!(character, '<' | '>' | '"' | '\'')).last().map_or(start, |(offset, character)| start + offset + character.len_utf8());
    let trimmed = trim_trailing_punctuation(text.get(start..end)?);
    let trimmed_end = expanded_url_span(text, start..start + trimmed.len()).end;
    (trimmed_end > start + prefix_len).then_some(start..trimmed_end)
}

fn detect_email_at(text: &str, start: usize) -> Option<Range<usize>> {
    let rest = text.get(start..)?;
    let at = rest.find('@')?;
    let local = rest.get(..at)?;
    let domain_start = start + at + 1;
    let domain_end = text.get(domain_start..)?.char_indices().take_while(|(_, character)| character.is_ascii_alphanumeric() || matches!(character, '.' | '-')).last().map_or(domain_start, |(offset, character)| domain_start + offset + character.len_utf8());
    let domain = text.get(domain_start..domain_end)?;
    (valid_email_local(local) && valid_email_domain(domain)).then_some(start..domain_end)
}

fn comparable_url_bytes(text: &str) -> impl Iterator<Item = u8> + '_ { text.bytes().filter(|byte| !byte.is_ascii_whitespace()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, ParseLanguage};

    #[test]
    fn standalone_detection_agrees_with_parsed_web_facts() {
        ["Visit HTTPS://EXAMPLE.COM", "Mail .foo@acme.com", "Mail foo.@acme.com", "Mail foo..bar@acme.com"].into_iter().for_each(|text| {
            let mut tree = crate::tokenized_text_tree(text);
            parse(&mut tree, ParseLanguage::English);
            assert_eq!(detect_urls(text), url_facts(&tree).map(|fact| fact.span()).collect::<Vec<_>>(), "URL: {text}");
            assert_eq!(detect_emails(text), email_facts(&tree).map(|fact| fact.span()).collect::<Vec<_>>(), "email: {text}");
        });
    }

}
