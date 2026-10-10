//! Applies Audiobookshelf-style text NFO fields to the common audiobook model.

use super::{author_credit, recording_book, AudiobookMetadata};
use crate::MetadataResult;

fn year(raw: &str) -> Option<i32> {
    let raw = raw.trim();
    (raw.len() == 4 && raw.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| raw.parse::<i32>().ok())
        .flatten()
        .filter(|year| (1000..=2100).contains(year))
}

fn names(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(';').map(str::trim).filter(|part| !part.is_empty())
}

pub(super) fn apply(input: &str, audio: &mut AudiobookMetadata) -> MetadataResult<book_model::BookMetadata> {
    let mut book_year = None;
    let mut language = None;
    let mut genres = Vec::new();
    let mut description_lines = Vec::new();
    let mut description_section = false;
    let mut replaced_authors = false;
    let mut replaced_narrators = false;
    let mut replaced_publishers = false;
    let mut replaced_asins = false;
    let mut replaced_isbns = false;
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("Book Description") {
            description_section = true;
            continue;
        }
        if description_section {
            if !trimmed.chars().all(|character| character == '=') { description_lines.push(line); }
            continue;
        }
        let Some((key, raw)) = line.split_once(':') else { continue };
        let raw = raw.trim();
        if raw.is_empty() { continue; }
        match key.trim().to_ascii_lowercase().as_str() {
            "title" => audio.title = raw.to_owned(),
            "subtitle" => audio.recording.subtitle = Some(raw.to_owned()),
            "author" | "authors" => {
                if !replaced_authors { audio.authors.clear(); replaced_authors = true; }
                audio.authors.extend(names(raw).filter_map(|name| author_credit(name.to_owned()).ok()));
            },
            "read by" | "narrator" | "narrators" => {
                if !replaced_narrators { audio.recording.narrators.clear(); replaced_narrators = true; }
                audio.recording.narrators.extend(names(raw).map(str::to_owned));
            },
            "publisher" => {
                if !replaced_publishers { audio.recording.publishers.clear(); replaced_publishers = true; }
                audio.recording.publishers.push(raw.to_owned());
            },
            "description" => audio.description = raw.to_owned(),
            "asin" if raw.len() == 10 && raw.bytes().all(|byte| byte.is_ascii_alphanumeric()) => {
                if !replaced_asins { audio.recording.asins.clear(); replaced_asins = true; }
                audio.recording.asins.push(raw.to_ascii_uppercase());
            },
            "isbn" => {
                if let Some(isbn) = book_model::from_metadata_value(raw) {
                    if !replaced_isbns { audio.recording.isbns.clear(); replaced_isbns = true; }
                    audio.recording.isbns.push(isbn);
                }
            },
            "language" => language = book_model::LanguageTag::parse(raw).ok(),
            "release date" => { audio.recording.recording_release_date = Some(raw.to_owned()); audio.recording.recording_release_year = None; },
            "release year" => {
                if let Some(parsed) = year(raw) { audio.recording.recording_release_year = Some(parsed); audio.recording.recording_release_date = None; }
            },
            "publication year" | "publish year" => book_year = year(raw),
            "genre" | "genres" => genres.extend(raw.split(',').map(str::trim).filter(|genre| !genre.is_empty()).map(str::to_owned)),
            _ => {},
        }
    }
    if !description_lines.is_empty() {
        let description = description_lines.join("\n").trim().to_owned();
        if !description.is_empty() { audio.description = description; }
    }
    let mut book = recording_book(&audio.recording)?;
    if let Some(language) = language { book.languages.push(language); }
    book.year = book_year;
    for genre in genres {
        if let Ok(subject) = book_model::BookSubject::new(None, genre, "nfo:genre", None, None) { book.subjects.push(subject); }
    }
    Ok(book)
}
