//! Metadata first; bounded native MOBI text inspection is an enrichment fallback.
use super::*;
#[cfg(test)]
use book_content::mobi::{ExthRecord, MobiMetadata};
use regex::Regex;
use std::sync::OnceLock;

pub fn inspect_mobi_metadata_reader(path: &Path, reader: impl Read) -> MetadataResult<BookRecord> {
    metadata_from_declared(path, book_content::mobi::read_metadata(reader)?)
}

/// Metadata and usable navigation, sharing one parsed MOBI container.
pub fn inspect_mobi_reader(path: &Path, reader: impl Read + Seek + Send + 'static) -> MetadataResult<(BookRecord, Vec<BookTocEntry>)> {
    let mut book = Book::open_reader(ContainerFormat::Mobi, reader)?;
    let metadata = metadata_from_book(path, &mut book)?;
    let toc = book_toc_entries(book.toc()?.unwrap_or_default());
    Ok((metadata, toc))
}

#[cfg(test)]
fn metadata_from_raw(path: &Path, raw: &MobiMetadata) -> MetadataResult<BookRecord> {
    metadata_from_declared(path, book_content::mobi::declared_metadata(raw))
}

pub fn inspect_mobi_pages_reader(path: &Path, reader: impl Read) -> MetadataResult<BookRecord> {
    let (metadata, text) = book_content::mobi::inspect_text(reader)?;
    let mut book = metadata_from_declared(path, metadata)?;
    static TAGS: OnceLock<Result<Regex, String>> = OnceLock::new();
    static BLOCKS: OnceLock<Result<Regex, String>> = OnceLock::new();
    static PAGES: OnceLock<Result<Regex, String>> = OnceLock::new();
    static HIDDEN: OnceLock<Result<Regex, String>> = OnceLock::new();
    static ENTITIES: OnceLock<Result<Regex, String>> = OnceLock::new();
    static CIP: OnceLock<Result<Regex, String>> = OnceLock::new();
    let markup = text.as_str();
    let end = markup.char_indices().nth(128 * 1024).map_or(markup.len(), |(i, _)| i);
    let markup = &markup[..end];
    let visible = cached_regex(&HIDDEN, r"(?is)<(script|style|head)\b[^>]*>.*?</(?:script|style|head)>")?.replace_all(markup, "");
    let pages = cached_regex(&PAGES, r"(?is)<mbp:pagebreak\b[^>]*>")?.replace_all(&visible, "\x0c");
    let blocks = cached_regex(&BLOCKS, r"(?is)</?(?:p|div|br|h[1-6])\b[^>]*>")?.replace_all(&pages, "\n");
    // Inline tags must not split digits or cutters in a printed code.
    let plain = cached_regex(&TAGS, r"(?s)<[^>]*>")?.replace_all(&blocks, "");
    let plain = plain.replace("&nbsp;", " ").replace("&copy;", "©").replace("&#x00A9;", "©");
    // One malformed/unknown entity must not prevent numeric entities elsewhere
    // (including ISBN hyphens) from being decoded.
    let plain = cached_regex(&ENTITIES, r"&(?:#[0-9]+|#x[0-9a-fA-F]+|amp|lt|gt|quot|apos);")?
        .replace_all(&plain, |captures: &regex::Captures<'_>| quick_xml::escape::unescape(&captures[0]).map(|value| value.into_owned()).unwrap_or_else(|_| captures[0].to_owned()));
    let mut sections = Vec::new();
    for (i, page) in plain.split('\x0c').take(64).enumerate() {
        if page.len() <= 16_384 {
            let section = evidence::try_inspect_section(&book, &format!("mobi:frontmatter:{i}"), page)?;
            if section.accepted {
                sections.push(section);
            }
        }
        // Some files omit a page break between a series list and its CIP block.
        // Anchor an additional bounded section to the printed CIP heading.
        for marker in cached_regex(&CIP, r"(?i)library of congress catalog(?:uing|ing)[ -]in[ -]book")?.find_iter(page).take(2) {
            if marker.start() < 2048 {
                continue;
            }
            let tail = &page[marker.start()..];
            let end = tail.char_indices().nth(6500).map_or(tail.len(), |(i, _)| i);
            let section = evidence::try_inspect_section(&book, &format!("mobi:frontmatter:{i}:cip"), &tail[..end])?;
            if section.accepted {
                sections.push(section);
            }
        }
    }
    evidence::apply_evidence(&mut book, &sections, "mobi");
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_asin_uuid_is_not_identity_and_mislabelled_isbn_is_recovered() {
        let mut raw = MobiMetadata::default();
        raw.exth.records.insert(ExthRecord::Asin, vec![b"9780192854216".to_vec(), b"814e491c-9ed1-43a5-b4ee-517c1a9b772c".to_vec()]);
        let book = metadata_from_raw(Path::new("book.mobi"), &raw).unwrap();
        assert_eq!(book.book.identifiers.len(), 1);
        assert_eq!(book.book.identifiers[0].scheme(), &Scheme::Isbn);
    }
}
