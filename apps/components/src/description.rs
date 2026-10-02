//! Book blurbs, which arrive as HTML and have to be drawn as text.
//!
//! Descriptions are stored exactly as the book wrote them, and
//! publishers write them in markup: `<p class="description">`, `<h3>Product
//! Description</h3>`, `<em>` around titles. Set as a raw string those tags are
//! what the reader sees.
//!
//! This is deliberately not the HTML renderer. `html-view-gpui` is a paginating
//! document engine — it owns a resource pipeline, an async host loop and a focus
//! handle it claims on construction.
//! A blurb needs three things from HTML: where the paragraphs end, what is
//! emphasised, and what the entities say. That is what this reads, and it
//! ignores the rest of the language rather than pretending to implement it.
//!
//! Malformed input is the normal case, not the error case: unclosed `<P>`, bare
//! `&`, tags in the wrong order. Nothing here can fail — the worst input yields
//! its own text back.

use std::ops::Range;

use gpui::{FontStyle, FontWeight, HighlightStyle};

/// Emphasis carried by a run of the flattened text.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Emphasis {
    italic: usize,
    bold: usize,
}

impl Emphasis {
    const NONE: Self = Self { italic: 0, bold: 0 };

    fn highlight(self) -> Option<HighlightStyle> {
        if self == Self::NONE {
            return None;
        }
        Some(HighlightStyle { font_style: (self.italic > 0).then_some(FontStyle::Italic), font_weight: (self.bold > 0).then_some(FontWeight::BOLD), ..Default::default() })
    }
}

/// The flattened blurb: text to set, and the ranges of it that are emphasised.
///
/// Byte ranges, because that is what `StyledText` indexes by.
pub(crate) fn rich_text(html: &str) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut text = String::with_capacity(html.len());
    let mut runs: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut emphasis = Emphasis::NONE;
    let mut run_start = 0;
    let mut rest = html;

    // Whitespace in the source is layout, not content: the renderer decides
    // where lines break, so runs of it collapse to one space and a block
    // boundary is the only thing that forces a new line.
    let push_text = |text: &mut String, chunk: &str| {
        for character in chunk.chars() {
            if character.is_whitespace() {
                if !matches!(text.chars().last(), None | Some(' ') | Some('\n')) {
                    text.push(' ');
                }
            } else {
                text.push(character);
            }
        }
    };

    while let Some(open) = rest.find('<') {
        let (before, after) = rest.split_at(open);
        push_text(&mut text, &decode_entities(before));
        // A `<` that begins no tag — "a < b" — is content. Only a name or a
        // closing slash starts markup.
        let starts_tag = after[1..].chars().next().is_some_and(|character| character.is_ascii_alphabetic() || character == '/' || character == '!');
        let Some(close) = after.find('>').filter(|_| starts_tag) else {
            push_text(&mut text, "<");
            rest = &after[1..];
            continue;
        };
        let tag = &after[1..close];
        rest = &after[close + 1..];

        // Comments and doctypes carry nothing to set.
        if tag.starts_with('!') {
            continue;
        }
        let closing = tag.starts_with('/');
        let name = tag.trim_start_matches('/').split(|character: char| character.is_whitespace() || character == '/').next().unwrap_or_default().to_ascii_lowercase();

        let next = match name.as_str() {
            "em" | "i" | "cite" | "var" => Emphasis { italic: if closing { emphasis.italic.saturating_sub(1) } else { emphasis.italic + 1 }, ..emphasis },
            "strong" | "b" => Emphasis { bold: if closing { emphasis.bold.saturating_sub(1) } else { emphasis.bold + 1 }, ..emphasis },
            // Scripts and styles are the one case where dropping the tag but
            // keeping its content would put code on the card.
            "script" | "style" => {
                if !closing && let Some(end) = rest.to_ascii_lowercase().find(&format!("</{name}")) {
                    rest = &rest[end..];
                }
                emphasis
            }
            _ => {
                if is_block(&name) {
                    end_line(&mut text);
                }
                emphasis
            }
        };

        if next != emphasis {
            if let Some(highlight) = emphasis.highlight()
                && run_start < text.len()
            {
                runs.push((run_start..text.len(), highlight));
            }
            run_start = text.len();
            emphasis = next;
        }
    }
    push_text(&mut text, &decode_entities(rest));
    if let Some(highlight) = emphasis.highlight()
        && run_start < text.len()
    {
        runs.push((run_start..text.len(), highlight));
    }

    while text.ends_with(['\n', ' ']) {
        text.pop();
    }
    let trimmed = text.trim_start().len();
    if trimmed < text.len() {
        let removed = text.len() - trimmed;
        text.drain(..removed);
        for (range, _) in &mut runs {
            *range = range.start.saturating_sub(removed)..range.end.saturating_sub(removed);
        }
    }
    runs.retain(|(range, _)| range.start < range.end && range.end <= text.len());
    (text, runs)
}

/// Tags that end the line they are on. A blurb's structure is paragraphs and
/// the occasional heading; everything else is inline as far as a card cares.
fn is_block(name: &str) -> bool {
    matches!(name, "p" | "br" | "div" | "li" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "section" | "article" | "hr" | "table" | "tr")
}

fn end_line(text: &mut String) {
    while text.ends_with(' ') {
        text.pop();
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
}

/// The entities that actually turn up in publisher blurbs, plus numeric ones.
/// An unknown entity is left as written rather than swallowed — `&foo;` is
/// likelier to be a stray ampersand than a character the reader is missing.
fn decode_entities(source: &str) -> String {
    if !source.contains('&') {
        return source.to_owned();
    }
    let mut decoded = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(';').filter(|end| *end <= 8) else {
            decoded.push('&');
            rest = after;
            continue;
        };
        let entity = &after[..end];
        let replacement = match entity.to_ascii_lowercase().as_str() {
            "amp" => Some("&".to_owned()),
            "lt" => Some("<".to_owned()),
            "gt" => Some(">".to_owned()),
            "quot" => Some("\"".to_owned()),
            "apos" | "#39" => Some("'".to_owned()),
            "nbsp" => Some(" ".to_owned()),
            "mdash" => Some("—".to_owned()),
            "ndash" => Some("–".to_owned()),
            "hellip" => Some("…".to_owned()),
            "lsquo" => Some("‘".to_owned()),
            "rsquo" => Some("’".to_owned()),
            "ldquo" => Some("“".to_owned()),
            "rdquo" => Some("”".to_owned()),
            _ => numeric_entity(entity),
        };
        match replacement {
            Some(replacement) => {
                decoded.push_str(&replacement);
                rest = &after[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = after;
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn numeric_entity(entity: &str) -> Option<String> {
    let digits = entity.strip_prefix('#')?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse().ok()?,
    };
    char::from_u32(code).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(html: &str) -> String {
        rich_text(html).0
    }

    #[test]
    fn publisher_markup_becomes_text() {
        assert_eq!(plain("<p class=\"description\">Drawing on extensive interviews</p>"), "Drawing on extensive interviews");
        assert_eq!(plain("<h3>Product Description</h3><p>Kissinger&rsquo;s lasting contribution.</p>"), "Product Description\nKissinger’s lasting contribution.");
        assert_eq!(plain("<P><B>A brilliant biography</B></P><P>Only two Americans</P>"), "A brilliant biography\nOnly two Americans");
    }

    #[test]
    fn emphasis_survives_as_runs() {
        let (text, runs) = rich_text("covers <em>White House Years</em> and after");
        assert_eq!(text, "covers White House Years and after");
        assert_eq!(runs.len(), 1);
        assert_eq!(&text[runs[0].0.clone()], "White House Years");
        assert_eq!(runs[0].1.font_style, Some(FontStyle::Italic));
        assert_eq!(runs[0].1.font_weight, None);
    }

    #[test]
    fn nested_emphasis_carries_both() {
        let (text, runs) = rich_text("<b>bold <i>and italic</i></b>");
        assert_eq!(text, "bold and italic");
        let italic = runs.iter().find(|(range, _)| &text[range.clone()] == "and italic").expect("the nested run");
        assert_eq!(italic.1.font_weight, Some(FontWeight::BOLD));
        assert_eq!(italic.1.font_style, Some(FontStyle::Italic));
    }

    /// The normal case, not the error case.
    #[test]
    fn malformed_input_yields_its_own_text() {
        assert_eq!(plain("Unclosed <b>bold and <em>italic"), "Unclosed bold and italic");
        assert_eq!(plain("plain text, no markup at all"), "plain text, no markup at all");
        assert_eq!(plain("2 < 3 and 5 > 4"), "2 < 3 and 5 > 4");
        assert_eq!(plain(""), "");
        assert_eq!(plain("<p></p>"), "");
    }

    #[test]
    fn whitespace_collapses_but_paragraphs_do_not() {
        assert_eq!(plain("  spaced\n\n   out  \t words "), "spaced out words");
        assert_eq!(plain("<p>One</p>\n  \n<p>Two</p>"), "One\nTwo");
        assert_eq!(plain("line<br/>break"), "line\nbreak");
    }

    #[test]
    fn scripts_and_comments_carry_nothing() {
        assert_eq!(plain("before<script>alert('x')</script>after"), "beforeafter");
        assert_eq!(plain("<!-- a comment -->kept"), "kept");
    }

    #[test]
    fn numeric_entities_decode_and_strays_survive() {
        assert_eq!(plain("caf&#233; &#x2014; open"), "café — open");
        assert_eq!(plain("Ben &amp; Jerry"), "Ben & Jerry");
        assert_eq!(plain("A & B"), "A & B");
        assert_eq!(plain("&unknown; stays"), "&unknown; stays");
    }
}
