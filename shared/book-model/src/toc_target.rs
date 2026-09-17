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

pub const PDF_TOC_TARGET_PREFIX: &str = "pdf-page:";

pub fn pdf_toc_target(page_index: usize) -> String {
    format!("{PDF_TOC_TARGET_PREFIX}{page_index}")
}

pub fn pdf_toc_page(target: &str) -> Option<usize> {
    target.strip_prefix(PDF_TOC_TARGET_PREFIX)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toc_targets_round_trip_and_reject_other_formats() {
        let audiobook_target = audiobook_toc_target(3_726_500);
        assert_eq!(audiobook_target, "audiobook-position-ms:3726500");
        assert_eq!(audiobook_toc_position(&audiobook_target), Some(3_726_500));
        assert_eq!(audiobook_toc_position("chapter.xhtml"), None);
        assert_eq!(audiobook_toc_position("audiobook-position-ms:not-a-number"), None);

        let pdf_target = pdf_toc_target(42);
        assert_eq!(pdf_target, "pdf-page:42");
        assert_eq!(pdf_toc_page(&pdf_target), Some(42));
        assert_eq!(pdf_toc_page("chapter.xhtml#part2"), None);
        assert_eq!(pdf_toc_page(&audiobook_target), None);
        assert_eq!(pdf_toc_page("pdf-page:"), None);
    }
}
