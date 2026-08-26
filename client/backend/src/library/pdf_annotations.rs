//! Library-path adapter for the framework-independent PDF document codec.

use std::path::{Path, PathBuf};
use sync_common::ContentHash;

use app_contract::reader_state::{AnnotationStyle, PdfAnnotationRect, ReaderAnnotation};

#[derive(Clone, Debug)]
pub(crate) struct ParsedPdfAnnotation {
    pub page_index: u32,
    pub annotation_index: usize,
    pub name: Option<String>,
    pub bokheim_id: Option<String>,
    pub style: AnnotationStyle,
    pub rects: Vec<PdfAnnotationRect>,
    pub color: String,
    pub opacity: f32,
    pub note: String,
    pub author: String,
}

fn reader_style(style: library_model::AnnotationStyle) -> AnnotationStyle {
    match style {
        library_model::AnnotationStyle::Highlight => AnnotationStyle::Highlight,
        library_model::AnnotationStyle::Underline => AnnotationStyle::Underline,
        library_model::AnnotationStyle::Squiggly => AnnotationStyle::Squiggly,
        library_model::AnnotationStyle::Strikethrough => AnnotationStyle::Strikethrough,
    }
}

fn model_style(style: &AnnotationStyle) -> library_model::AnnotationStyle {
    match style {
        AnnotationStyle::Highlight => library_model::AnnotationStyle::Highlight,
        AnnotationStyle::Underline => library_model::AnnotationStyle::Underline,
        AnnotationStyle::Squiggly => library_model::AnnotationStyle::Squiggly,
        AnnotationStyle::Strikethrough => library_model::AnnotationStyle::Strikethrough,
    }
}

pub(crate) fn parse(path: &Path) -> Result<Vec<ParsedPdfAnnotation>, Box<dyn std::error::Error>> {
    book_metadata::pdf_annotations::parse(path).map(|annotations| {
        annotations
            .into_iter()
            .map(|annotation| ParsedPdfAnnotation {
                page_index: annotation.page_index,
                annotation_index: annotation.annotation_index,
                name: annotation.name,
                bokheim_id: annotation.bokheim_id,
                style: reader_style(annotation.style),
                rects: annotation.rects.into_iter().map(|rect| PdfAnnotationRect::new(rect.left, rect.top, rect.width, rect.height)).collect(),
                color: annotation.color,
                opacity: annotation.opacity,
                note: annotation.note,
                author: annotation.author,
            })
            .collect()
    })
}

pub(crate) fn upsert(path: &Path, annotation: &ReaderAnnotation) -> Result<(), Box<dyn std::error::Error>> {
    let anchor = annotation.pdf_anchor().ok_or("PDF annotation has no PDF anchor")?;
    book_metadata::pdf_annotations::upsert(
        path,
        &book_metadata::pdf_annotations::PdfAnnotation {
            id: annotation.id.clone(),
            content_hash: annotation.content_hash.clone(),
            page_index: anchor.page_index,
            rects: anchor.rects.into_iter().map(|rect| library_model::PdfAnnotationRect::new(rect.left, rect.top, rect.width, rect.height)).collect(),
            style: model_style(&annotation.style),
            color: annotation.color.clone(),
            note: annotation.note.clone(),
        },
    )
}

pub(crate) fn remove(path: &Path, annotation_id: &str) -> Result<bool, Box<dyn std::error::Error>> {
    book_metadata::pdf_annotations::remove(path, annotation_id)
}

fn base_staging_dir(library_root: &Path) -> PathBuf {
    library_root.join(crate::APP_HIDDEN_DIR).join("staging/pdf-bases")
}

pub(crate) fn stage_base(library_root: &Path, working_path: &Path, content_hash: &ContentHash) -> Result<PathBuf, Box<dyn std::error::Error>> {
    book_metadata::pdf_annotations::stage_base(&base_staging_dir(library_root), working_path, content_hash)
}

pub(crate) fn is_staged_base(library_root: &Path, path: &Path) -> bool {
    book_metadata::pdf_annotations::is_staged_base(&base_staging_dir(library_root), path)
}
