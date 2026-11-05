//! Local annotation export planning and portable document writers.
//! Files stay local; external annotations are preserved without importing them.
use library_model::*;
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};
use sync_common::LibraryId;

mod failure;
mod foliate;
mod markdown;
mod readest;
#[cfg(not(target_arch = "wasm32"))]
mod reconcile;
pub use foliate::foliate;
pub use markdown::markdown;
pub use readest::readest;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod reader_profiles;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{export, export_prepared};

pub fn library_destination(base: &Path, library_name: &str) -> PathBuf {
    base.join(file_stem(library_name))
}

/// Produce portable names without allowing a title to introduce directories.
pub fn file_stem(title: &str) -> String {
    let mut result = title.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect::<String>();
    result = result.trim().trim_matches('.').trim().to_owned();
    if result.len() > 160 {
        let mut end = 160;
        while !result.is_char_boundary(end) {
            end -= 1;
        }
        result.truncate(end);
        result = result.trim_end_matches(['.', ' ']).to_owned();
    }
    if result.is_empty() {
        result = "Untitled".into();
    }
    let first = result.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(first.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (first.len() == 4 && (first.starts_with("COM") || first.starts_with("LPT")) && matches!(first.as_bytes()[3], b'1'..=b'9')) {
        result.insert(0, '_');
    }
    result
}

pub fn plan(library_id: LibraryId, books: &[AnnotationExportBook], options: &AnnotationExportOptions) -> io::Result<AnnotationExportPlan> {
    if options.formats.is_empty() {
        return Err(io::Error::other("select at least one annotation export format"));
    }
    let mut items = Vec::new();
    let mut names = HashSet::new();
    let mut seen = HashSet::new();
    for book in books {
        for placement in &book.placements {
            let relative = RelativeBookPath::parse(&placement.relative_path).map_err(io::Error::other)?;
            let parent = Path::new(relative.as_str().trim_start_matches('/')).parent().unwrap_or(Path::new(""));
            for &format in &options.formats {
                if !seen.insert((book.book_id, placement.directory_id, format as u8)) {
                    continue;
                }
                if (format == AnnotationExportFormat::Pdf && book.format != book_model::BookFormat::Pdf) || (matches!(format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) && book.format != book_model::BookFormat::Epub)
                {
                    continue;
                }
                let explicit = options.overrides.iter().find(|item| item.book_id == book.book_id && item.directory_id == placement.directory_id && item.format == format);
                if format == AnnotationExportFormat::Pdf && explicit.is_some() {
                    return Err(io::Error::other("PDF annotations must be written into their existing library file"));
                }
                let path = if format == AnnotationExportFormat::Pdf {
                    PathBuf::from(&placement.relative_path)
                } else if let Some(explicit) = explicit {
                    explicit.path.clone()
                } else {
                    let extension = match format {
                        AnnotationExportFormat::Markdown => "md",
                        AnnotationExportFormat::Foliate => "foliate.json",
                        AnnotationExportFormat::Readest => "readest.json",
                        AnnotationExportFormat::Pdf => unreachable!("PDFs use their existing path"),
                    };
                    let stem = file_stem(&book.metadata.title);
                    let folder = options.destination.join(parent);
                    let mut candidate = folder.join(format!("{stem}.{extension}"));
                    let mut index = 2;
                    while names.contains(&candidate.to_string_lossy().to_lowercase()) {
                        candidate = folder.join(format!("{stem} ({index}).{extension}"));
                        index += 1;
                    }
                    candidate
                };
                if format != AnnotationExportFormat::Pdf && !names.insert(path.to_string_lossy().to_lowercase()) {
                    return Err(io::Error::other("two exports have the same destination"));
                }
                items.push(AnnotationExportDestination { book_id: book.book_id, directory_id: placement.directory_id, format, path });
            }
        }
    }
    Ok(AnnotationExportPlan { library_id, items })
}

#[cfg(test)]
mod tests;
