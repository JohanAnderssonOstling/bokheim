use crate::{BackendError, BoxedCpuReader};
use book_content::{Book, BookFormat as TextFormat};
use book_model::{AnnotationAnchor, BookFormat, BookTextCheckpoint, BookTextIndex, BookTextLocation, BookTextMatch, BookTextPart, BookTextTarget, PdfAnnotationAnchor};
use device_sqlite::fts5::{prefix_phrase_matches, token_spans, tokens};
use std::collections::{BTreeMap, BTreeSet};
use html_parse::{MarkupSyntax, decode_html_bytes, parse_document, DocumentTextIndex};

fn extract(format: BookFormat, reader: BoxedCpuReader, selected: Option<Vec<String>>) -> Result<Vec<BookTextPart>, BackendError> {
    if format == BookFormat::Pdf {
        let pages = selected.map(|values| values.into_iter().map(|value| value.parse::<usize>().map_err(BackendError::operation)).collect::<Result<Vec<_>, _>>()).transpose()?;
        return pdf_reader_core::extract_search_text(reader, pages).map_err(BackendError::operation);
    }
    let format = match format {
        BookFormat::Epub => TextFormat::Epub,
        BookFormat::Mobi => TextFormat::Mobi,
        BookFormat::Fb2 => TextFormat::Fb2,
        _ => return Err("this book format has no searchable text".into()),
    };
    let mut book = Book::open_reader(format, reader).map_err(BackendError::operation)?;
    let mut documents = book.inspect_documents().map_err(BackendError::operation)?;
    let previews = selected.is_some();
    let uris = selected.unwrap_or_else(|| documents.document_uris().to_vec());
    let mut parts = Vec::with_capacity(uris.len());
    let mut total = 0;
    for uri in uris {
        let part = (|| -> Result<_, BackendError> {
            let bytes = documents.read_prefix(&uri, 24 * 1024 * 1024 + 1).map_err(BackendError::operation)?;
            if bytes.len() > 24 * 1024 * 1024 {
                return Err("book chapter is too large to index".into());
            }
            let source = decode_html_bytes(&bytes, None);
            let media_type = documents.media_type(&uri).map_err(BackendError::operation)?;
            let parsed = parse_document(&source, MarkupSyntax::from_resource(&uri, media_type.as_deref())).map_err(BackendError::operation)?;
            let index = DocumentTextIndex::from_parsed(&parsed).map_err(BackendError::operation)?;
            Ok(BookTextPart { document: uri, text: index.text().to_owned(), bounds: Vec::new() })
        })();
        let part = match part {
            Ok(part) => part,
            Err(_) if previews => continue,
            Err(error) => return Err(error),
        };
        total += part.text.len();
        if total > 64 * 1024 * 1024 {
            return Err("book text is too large to index".into());
        }
        parts.push(part);
    }
    Ok(parts)
}

pub(super) fn index(format: BookFormat, reader: BoxedCpuReader) -> Result<BookTextIndex, BackendError> {
    let parts = extract(format, reader, None)?;
    let counts = device_sqlite::fts5::token_counts(parts.iter().map(|part| part.text.as_str())).map_err(BackendError::operation)?;
    let mut text = String::with_capacity(parts.iter().map(|part| part.text.len() + 1).sum());
    let mut checkpoints = Vec::with_capacity(parts.len());
    let mut start = 0;
    for (part, count) in parts.into_iter().zip(counts) {
        checkpoints.push(BookTextCheckpoint { document: part.document, start });
        start += count;
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&part.text);
    }
    Ok(BookTextIndex { text, checkpoints })
}

/// Extract and resolve a visible window entirely on the CPU host.
pub(super) fn previews(format: BookFormat, reader: BoxedCpuReader, locations: Vec<BookTextLocation>, query: &str) -> Result<Vec<BookTextMatch>, BackendError> {
    let documents = locations.iter().flat_map(|location| location.documents.iter().cloned()).collect::<BTreeSet<_>>();
    let parts = extract(format, reader, Some(documents.into_iter().collect()))?.into_iter().map(|part| (part.document.clone(), part)).collect::<BTreeMap<_, _>>();
    let connection = device_sqlite::open(":memory:", Default::default()).map_err(BackendError::operation)?;
    let phrase = tokens(&connection, query).map_err(BackendError::operation)?;
    let mut matches = vec![BookTextMatch::default(); locations.len()];
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for (index, location) in locations.into_iter().enumerate() {
        groups.entry(location.documents).or_default().push((index, location.token_offset));
    }
    for (documents, mut requests) in groups {
        let Some(part) = documents.first().and_then(|document| parts.get(document)) else { continue };
        // Keep source order across page/chapter boundaries. A missing part
        // ends this window; hits before that boundary can still be resolved.
        let mut text = String::new();
        for document in &documents {
            let Some(part) = parts.get(document) else { break };
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&part.text);
        }
        requests.sort_unstable_by_key(|(_, offset)| *offset);
        let starts = requests.iter().map(|(_, offset)| *offset).collect::<Vec<_>>();
        let spans = token_spans(&connection, &text, &starts, phrase.len()).map_err(BackendError::operation)?;
        for ((index, _), span) in requests.into_iter().zip(spans) {
            let Some(span) = span else { continue };
            let token = span.bytes;
            if prefix_phrase_matches(&connection, &text[token.clone()], &phrase).map_err(BackendError::operation)?.is_empty() {
                continue;
            }
            let hit = &mut matches[index];
            hit.target = if part.bounds.is_empty() {
                Some(BookTextTarget { document: part.document.clone(), character_offset: span.character_offset }.open_target().map_err(BackendError::operation)?)
            } else {
                let rects = part.bounds.get(span.character_offset).copied().flatten().into_iter().collect();
                let page = part.document.parse().map_err(BackendError::operation)?;
                AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(page, rects, None)).open_target()
            };
            let start = text[..token.start].char_indices().rev().nth(55).map_or(0, |(index, _)| index);
            let start = if start == 0 { 0 } else { text[start..token.start].find(char::is_whitespace).map_or(token.start, |index| start + index) };
            let end = text[token.end..].char_indices().nth(180).map_or(text.len(), |(index, _)| token.end + index);
            hit.excerpt = text[start..end].split_whitespace().collect::<Vec<_>>().join(" ");
            hit.excerpt_matches = prefix_phrase_matches(&connection, &hit.excerpt, &phrase).map_err(BackendError::operation)?;
        }
    }
    Ok(matches)
}
