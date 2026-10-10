//! Storage-independent collision naming rules.

use std::collections::HashSet;
use std::path::Path;

use crate::library_state::portable_name_key;

pub fn numbered_folder_name(base: &str, number: usize) -> String {
    crate::folder_names::numbered(base, number)
}

pub fn numbered_file_name(base: &str, number: usize) -> String {
    use crate::filenames::{MAX_FILE_NAME_BYTES, truncate_utf8};
    let path = Path::new(base);
    let suffix = format!(" {number}");
    let (stem, extension) = match (path.file_stem().and_then(|value| value.to_str()), path.extension().and_then(|value| value.to_str())) {
        (Some(stem), Some(extension)) => {
            // Leave room for a full UTF-8 character even with an unusually long extension.
            let extension = truncate_utf8(extension, MAX_FILE_NAME_BYTES - suffix.len() - 5);
            (stem, format!(".{extension}"))
        }
        _ => (base, String::new()),
    };
    let stem = truncate_utf8(stem, MAX_FILE_NAME_BYTES - suffix.len() - extension.len());
    format!("{stem}{suffix}{extension}")
}

// The caller supplies occupied names from its database or filesystem. These
// rules own normalization and suffixes; storage owns no-clobber retries.
fn name_candidates<'a>(requested: String, occupied: impl IntoIterator<Item = &'a str>, numbered: fn(&str, usize) -> String) -> impl Iterator<Item = String> {
    let mut occupied = occupied.into_iter().map(portable_name_key).collect::<HashSet<_>>();
    (1..).filter_map(move |number| {
        let candidate = if number == 1 { requested.clone() } else { numbered(&requested, number) };
        occupied.insert(portable_name_key(&candidate)).then_some(candidate)
    })
}

/// Collision-free candidates, including names rejected by earlier attempts.
pub fn folder_name_candidates<'a>(requested: &str, occupied: impl IntoIterator<Item = &'a str>) -> impl Iterator<Item = String> {
    name_candidates(crate::project_folder_name(requested), occupied, numbered_folder_name)
}

pub fn file_name_candidates<'a>(requested: &str, occupied: impl IntoIterator<Item = &'a str>) -> impl Iterator<Item = String> {
    name_candidates(requested.to_owned(), occupied, numbered_file_name)
}

pub fn unique_folder_name<'a>(requested: &str, occupied: impl IntoIterator<Item = &'a str>) -> String {
    folder_name_candidates(requested, occupied).next().expect("numeric name suffix space is unbounded")
}

pub fn unique_file_name<'a>(requested: &str, occupied: impl IntoIterator<Item = &'a str>) -> String {
    file_name_candidates(requested, occupied).next().expect("numeric name suffix space is unbounded")
}
