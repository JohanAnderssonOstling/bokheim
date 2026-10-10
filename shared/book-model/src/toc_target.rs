//! What a [`BookTocEntry::target`] means, per format.
//!
//! Every format's navigation reaches the same tree of title, target and
//! children; what differs is only the target value. An EPUB target needs no
//! prefix — it is the publisher's own href, and resolving it to a document and
//! anchor is the renderer's business. A PDF entry points at a page rather than
//! at a document fragment, and an audiobook chapter at an offset, so both carry
//! their value behind a scheme.
//!
//! [`BookTocEntry::target`]: crate::BookTocEntry::target

pub const AUDIOBOOK_TOC_TARGET_PREFIX: &str = "audiobook-position-ms:";

pub fn audiobook_toc_target(position_ms: u64) -> String {
    format!("{AUDIOBOOK_TOC_TARGET_PREFIX}{position_ms}")
}

pub fn audiobook_toc_position(target: &str) -> Option<u64> {
    target.strip_prefix(AUDIOBOOK_TOC_TARGET_PREFIX)?.parse().ok()
}

/// Display cleanup for an audiobook entry, using its one-based position.
/// Keep the original stored heading as matching evidence.
pub fn audiobook_chapter_display_title(title: &str, position: usize) -> String {
    let spaced = title.replace('_', " ");
    let clean = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let digits = clean.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && clean[..digits].parse::<usize>().ok() == Some(position) {
        let remainder = &clean[digits..];
        if remainder.is_empty() {
            return format!("Chapter {position}");
        }
        let separator = |c: char| c.is_whitespace() || matches!(c, '.' | '-' | ':' | ')');
        if remainder.starts_with(separator) {
            let name = remainder.trim_start_matches(separator);
            if !name.is_empty() {
                return name.to_owned();
            }
        }
    }
    clean
}

pub const PDF_TOC_TARGET_PREFIX: &str = "pdf-page:";

pub fn pdf_toc_target(page_index: usize) -> String {
    format!("{PDF_TOC_TARGET_PREFIX}{page_index}")
}

pub fn pdf_toc_page(target: &str) -> Option<usize> {
    target.strip_prefix(PDF_TOC_TARGET_PREFIX)?.parse().ok()
}
