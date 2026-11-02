use book_model::{pdf_toc_page, Annotation, BookTocEntry};
use chrono::{DateTime, SecondsFormat, Utc};
use library_model::{annotation_markdown, annotation_source_label, AnnotationExportBook, AnnotationExportPlacement, BookLocator, ReaderLink};
use serde_json::{json, Value};
use std::{fmt::Write, io};
use sync_common::LibraryId;

pub(crate) fn timestamp(millis: u64) -> io::Result<String> {
    let millis = i64::try_from(millis).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    DateTime::<Utc>::from_timestamp_millis(millis).map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true)).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "annotation timestamp is out of range"))
}

pub fn markdown(library: LibraryId, book: &AnnotationExportBook, placement: &AnnotationExportPlacement, exported_at: u64) -> io::Result<String> {
    let mut out = String::from("---\n");
    let mut field = |name: &str, value: Value| {
        if !value.is_null() {
            writeln!(out, "{name}: {value}").unwrap();
        }
    };
    field("title", json!(book.metadata.title));
    field("subtitle", json!(book.metadata.subtitle));
    field("authors", json!(book.metadata.authors().map(|author| author.name()).collect::<Vec<_>>()));
    field("identifiers", json!(book.metadata.book.identifiers));
    field("languages", json!(book.metadata.book.languages));
    field("publishers", json!(book.metadata.book.publishers));
    field("year", json!(book.metadata.book.year));
    field("metadata", json!(book.metadata));
    field("library_id", json!(library));
    field("book_id", json!(book.book_id));
    field("format", json!(book.format.canonical_extension()));
    field("canonical_content_hash", json!(book.canonical_hash));
    field("local_content_hash", json!(placement.local_hash));
    field("exported_at", json!(timestamp(exported_at)?));
    out.push_str("---\n\n# ");
    out.push_str(&heading(&book.metadata.title));
    out.push_str("\n\n");
    let mut headings = Vec::new();
    flatten(&book.toc, &mut Vec::new(), &mut headings);
    let ordinal = |annotation: &Annotation| {
        annotation.toc_ordinal.and_then(|n| usize::try_from(n).ok()).filter(|&n| n < headings.len()).or_else(|| {
            let page = annotation.anchor.pdf_anchor()?.page_index as usize;
            headings.iter().enumerate().filter_map(|(index, (_, target))| pdf_toc_page(target).filter(|&start| start <= page).map(|_| index)).last()
        })
    };
    let mut annotations = book.annotations.iter().filter(|a| !a.deleted).collect::<Vec<_>>();
    annotations.sort_by(|a, b| {
        ordinal(a)
            .unwrap_or(usize::MAX)
            .cmp(&ordinal(b).unwrap_or(usize::MAX))
            .then_with(|| a.anchor.pdf_anchor().map(|v| v.page_index).cmp(&b.anchor.pdf_anchor().map(|v| v.page_index)))
            .then_with(|| a.progress.unwrap_or(0.0).total_cmp(&b.progress.unwrap_or(0.0)))
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut previous = Vec::<(usize, String)>::new();
    for annotation in annotations {
        let path = ordinal(annotation).map(|ordinal| headings[ordinal].0.clone()).unwrap_or_else(|| vec![(usize::MAX, "Other annotations".into())]);
        let common = previous.iter().zip(&path).take_while(|(a, b)| a.0 == b.0).count();
        for (depth, (_, title)) in path.iter().enumerate().skip(common) {
            writeln!(out, "{} {}\n", "#".repeat((depth + 2).min(6)), heading(title)).unwrap();
        }
        previous = path;
        let section = ordinal(annotation).and_then(|ordinal| headings[ordinal].0.last()).map(|(_, title)| title.as_str());
        let label = annotation_source_label(&book.metadata.title, section, &annotation.anchor);
        let link = ReaderLink { locator: BookLocator::new(library, book.book_id), annotation_id: Some(annotation.id.clone()), anchor: annotation.anchor.clone() };
        out.push_str(&annotation_markdown(&annotation.exact_text, &annotation.note, &label, &link));
    }
    Ok(out)
}

fn heading(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace('\\', "\\\\").replace('#', "\\#").replace('[', "\\[").replace(']', "\\]").replace('<', "&lt;").replace('>', "&gt;")
}

fn flatten(entries: &[BookTocEntry], parents: &mut Vec<(usize, String)>, out: &mut Vec<(Vec<(usize, String)>, String)>) {
    for entry in entries {
        parents.push((out.len(), entry.title.clone()));
        out.push((parents.clone(), entry.target.clone()));
        flatten(&entry.children, parents, out);
        parents.pop();
    }
}
