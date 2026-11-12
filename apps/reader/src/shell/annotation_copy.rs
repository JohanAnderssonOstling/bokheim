use book_model::{Annotation, AnnotationAnchor};
use gpui::{App, ClipboardItem};
use annotation_format::{annotation_markdown, annotation_source_label};
use library_model::{BookLocator, ReaderLink};

pub(crate) fn copy_annotation(locator: &BookLocator, title: &str, section: Option<&str>, annotation: &Annotation, cx: &mut App) {
    let (quote, note) = if annotation.is_bookmark() { ("", "") } else { (annotation.exact_text.as_str(), annotation.note.as_str()) };
    copy_passage(locator, title, section, quote, note, Some(annotation.id.clone()), annotation.anchor.clone(), cx);
}

pub(crate) fn copy_passage(locator: &BookLocator, title: &str, section: Option<&str>, quote: &str, note: &str, annotation_id: Option<String>, anchor: AnnotationAnchor, cx: &mut App) {
    let label = annotation_source_label(title, section, &anchor);
    let source = ReaderLink { locator: locator.clone(), annotation_id, anchor };
    cx.write_to_clipboard(ClipboardItem::new_string(annotation_markdown(quote, note, &label, &source)));
}

/// Copy only the navigable source, without quoting the selected passage.
pub(crate) fn copy_link(locator: &BookLocator, title: &str, section: Option<&str>, anchor: AnnotationAnchor, cx: &mut App) {
    let label = annotation_source_label(title, section, &anchor);
    let source = ReaderLink { locator: locator.clone(), annotation_id: None, anchor };
    cx.write_to_clipboard(ClipboardItem::new_string(annotation_markdown("", "", &label, &source).trim_end().to_owned()));
}
