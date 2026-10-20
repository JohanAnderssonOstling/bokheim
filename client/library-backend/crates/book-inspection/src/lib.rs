//! Portable book metadata inspection: EPUB, MOBI and PDF documents, and M4B
//! and MP3-folder audiobooks.

use book_content::{Book, BookFormat as ContainerFormat, DeclaredMetadata, TocEntry};
use book_content::{IdentifierScheme, MetadataCredit, MetadataProperty};
use book_model::{
    marc_relator_code_from_metadata_role, unique_book_year, BookFormat, BookMetadata, BookMetadataError, BookRecord, BookSubject, BookTocEntry, Contributor, Identifier, LanguageTag, Scheme, Scope, AUTHOR_MARC_RELATOR_CODE,
};
use std::io::{Read, Seek};
use std::path::Path;
use std::time::Instant;

pub type MetadataResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod book;
pub use book::{AudiobookMetadata, InspectedBook, inspect_book_reader, inspect_book_reader_with_hash};
pub mod evidence;
mod mobi;
pub use mobi::{inspect_mobi_metadata_reader, inspect_mobi_pages_reader, inspect_mobi_reader};
mod document_evidence;
mod filename_identity;
mod filename_identifiers;
mod filename_isbn;
mod title_identity;
mod identity_evidence;
pub use title_identity::{normalize_with_filename, title_readings};
mod pdf;
mod pdf_ranges;
pub use pdf::{inspect_pdf, inspect_pdf_metadata, inspect_pdf_pages_reader, inspect_pdf_reader, InspectedPdf};

/// Inspection cache version for a format. Matches the extractor contract:
/// EPUB/MOBI include the shared declared-metadata projection; PDF follows
/// the shared evidence extractor.
/// Shared by explicit import and the filesystem scanner, which both cache
/// results keyed on this value.
pub fn inspection_version(format: BookFormat) -> i64 {
    match format {
        BookFormat::Epub => i64::from(evidence::INSPECTION_VERSION) + 3,
        BookFormat::Pdf => i64::from(evidence::INSPECTION_VERSION) + 1,
        BookFormat::Mobi => 6,
        _ => 3,
    }
}

/// Reads PDF identity dictionaries without loading page or image streams.
pub fn pdf_identity_document(reader: impl Read + Seek) -> MetadataResult<lopdf::Document> {
    pdf_ranges::document_root(reader)
}

pub struct InspectedEpub {
    pub metadata: BookRecord,
    pub toc: Vec<BookTocEntry>,
    pub timings: EpubInspectionTimings,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EpubInspectionTimings {
    pub container_ms: u64,
    pub evidence_ms: u64,
    pub toc_ms: u64,
}

/// Reads package metadata and navigation from one EPUB provider instance.
/// Authority ingestion uses this instead of maintaining a second EPUB
/// metadata parser; ordinary library scans can continue to omit the TOC.
pub fn inspect_epub(path: &Path) -> MetadataResult<InspectedEpub> {
    let container_started = Instant::now();
    let mut provider = open_epub(path)?;
    let mut metadata = metadata_from_book(path, &mut provider)?;
    let container_ms = u64::try_from(container_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let evidence_started = Instant::now();
    inspect_epub_pages(&mut provider, &mut metadata);
    let evidence_ms = u64::try_from(evidence_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let toc_started = Instant::now();
    let toc = book_toc_entries(provider.toc()?.unwrap_or_default());
    let toc_ms = u64::try_from(toc_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(InspectedEpub { metadata, toc, timings: EpubInspectionTimings { container_ms, evidence_ms, toc_ms } })
}

/// Reads EPUB metadata from any seekable byte source. Browser ingestion uses
/// this entry point, while native scanning passes a file through
/// [`inspect_epub`]; both therefore share the complete metadata projection.
pub fn inspect_epub_reader<R>(source_name: &Path, reader: R) -> MetadataResult<InspectedEpub>
where
    R: Read + Seek + Send + 'static,
{
    let mut provider = Book::open_reader(ContainerFormat::Epub, reader)?;
    let mut metadata = metadata_from_book(source_name, &mut provider)?;
    inspect_epub_pages(&mut provider, &mut metadata);
    let toc = book_toc_entries(provider.toc()?.unwrap_or_default());
    Ok(InspectedEpub { metadata, toc, timings: EpubInspectionTimings::default() })
}

fn book_toc_entries(entries: Vec<TocEntry>) -> Vec<BookTocEntry> {
    let mut count = 0;
    entries.into_iter().filter_map(|entry| book_toc_entry(entry, 0, &mut count)).collect()
}

fn book_toc_entry(entry: TocEntry, depth: usize, count: &mut usize) -> Option<BookTocEntry> {
    const MAX_TOC_DEPTH: usize = 32;
    const MAX_TOC_ENTRIES: usize = 8_192;
    const MAX_TOC_TEXT_BYTES: usize = 4_096;
    if depth >= MAX_TOC_DEPTH || *count >= MAX_TOC_ENTRIES {
        return None;
    }
    let valid_text = |value: String| {
        let value = value.trim();
        (!value.is_empty() && value.len() <= MAX_TOC_TEXT_BYTES && !value.chars().any(char::is_control)).then(|| value.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    let title = valid_text(entry.title)?;
    let target = valid_text(entry.link).unwrap_or_default();
    *count += 1;
    let children = entry.children.into_iter().filter_map(|entry| book_toc_entry(entry, depth + 1, count)).collect();
    Some(BookTocEntry { title, target, children })
}

/// Reads all metadata relevant to indexing. Formats without useful embedded
fn fallback_metadata(path: &Path) -> BookRecord {
    let title = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned();
    BookRecord { metadata: book_model::BookDetails { title, subtitle: None, contributors: Vec::new(), book: Default::default() }, description: String::new() }
}

fn author_credit(name: String) -> Result<Contributor, BookMetadataError> {
    Contributor::new(name, AUTHOR_MARC_RELATOR_CODE)
}

/// One credit from one cleaned author field.
///
/// An authority file lists people under the family name, so "Yakobson, Alexander"
/// becomes "Alexander Yakobson" for display, and display order is what the
/// index sorts by. Anything that does not plainly invert is stored exactly
/// as stated.
fn author_from_field(name: String) -> Option<Contributor> {
    match book_model::given_name_first(&name) {
        Some(display) => author_credit(display).ok(),
        None => author_credit(name).ok(),
    }
}

/// Producers that write themselves into an author field. A conversion tool is
/// not a person, and a record naming one is worse than a record naming
/// nobody: it files books under "Adobe Acrobat" in the author index.
const AUTHOR_TOOL_PREFIXES: &[&str] =
    &["adobe", "acrobat", "abbyy", "pscript", "latex", "microsoft", "word", "pdfcreator", "ghostscript", "quark", "indesign", "administrator", "unknown", "untitled", "author", "user", "owner", "none", "scanner", "www.", "http"];

/// Names stated in one raw author field, common to every format.
///
/// Two of the three transformations here are unambiguous and applied; the third
/// is deliberately absent. `;`, ` and `, ` & ` and ` with ` always separate
/// people, so they are split on. A comma does not: in "Yakobson, Alexander, Gat,
/// Azar" it separates two people written surname-first, and in "Grose, Peter,
/// 1934-" it separates a name from a life date. Splitting on commas therefore
/// stays out of ingestion, where a wrong guess becomes a permanent author
/// record, and belongs with the enrichment review a person can correct.
fn author_names(raw: &str) -> Vec<String> {
    const SEPARATORS: &[&str] = &[";", " and ", " & ", " with ", " AND ", " With "];
    // Tested whole, before splitting: "LaTeX with hyperref package" is one tool
    // string, and splitting it first would leave "hyperref package" behind as a
    // plausible-looking name.
    if is_author_tool(raw) {
        return Vec::new();
    }
    let mut parts = vec![raw.trim().to_owned()];
    for separator in SEPARATORS {
        parts = parts.iter().flat_map(|part| part.split(separator)).map(str::to_owned).collect();
    }
    parts
        .into_iter()
        .map(|part| {
            // Strip role markers, trailing life dates, and full stops from names.
            let mut name = part.trim().to_owned();
            while let Some(open) = name.rfind('(') {
                let Some(close) = name[open..].find(')').map(|offset| open + offset) else { break };
                let inside = name[open + 1..close].trim().to_ascii_lowercase();
                if !matches!(inside.as_str(), "author" | "authors" | "ed" | "ed." | "editor" | "editors" | "trans" | "trans." | "translator" | "illustrator" | "compiler") {
                    break;
                }
                name.replace_range(open..=close, "");
                name = name.trim().to_owned();
            }
            // A life date: "Grose, Peter, 1934-" or "Cicero, 106-43 B.C."
            if let Some(comma) = name.rfind(',') {
                let tail = name[comma + 1..].trim();
                if !tail.is_empty() && tail.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    name.truncate(comma);
                }
            }
            // Removing a marker leaves its space behind: "Piper (Author), Murphy"
            // becomes "Piper , Murphy" unless the gap is closed.
            name = name.split_whitespace().collect::<Vec<_>>().join(" ").replace(" ,", ",").replace(" .", ".");
            name = name.trim().trim_end_matches([',', ';']).trim().to_owned();
            // "Renehan, Edward." keeps a surname-first form but loses the stop; an
            // initial ("Piper, Fred C.") keeps it, because there the stop is the name.
            if name.ends_with('.') {
                let stem = name.trim_end_matches('.');
                let last_word = stem.rsplit([' ', ',']).next().unwrap_or_default();
                if last_word.chars().count() > 1 {
                    name = stem.trim().to_owned();
                }
            }
            name
        })
        .filter(|name| !name.is_empty() && !is_author_tool(name))
        .collect()
}

fn is_author_tool(name: &str) -> bool {
    let lowered = name.trim().to_ascii_lowercase();
    filename_identity::unusable_title(name) || AUTHOR_TOOL_PREFIXES.iter().any(|prefix| lowered.starts_with(prefix))
}

/// The blurb a reader should be given, where a book states two.
///
/// Standard Ebooks writes a one-line teaser in `dc:description` — "A wealthy
/// young woman decides to take on the role of patroness and matchmaker" — and
/// the real blurb in a `se:long-description` refinement beside it, in
/// paragraphs. The short line is written to fit a provider row; a book's own
/// page has room for the one that says something.
///
/// Read from the book's own properties rather than from the modelled
/// ones, because the modelled ones cannot hold this: a synchronized property
/// value carries no control characters, and these are written indented across
/// several lines in the package document. The whitespace that costs it its
/// place there is whitespace HTML would collapse anyway, so collapsing it is
/// what makes the value storable rather than a liberty taken with it.
///
/// Matched on the property's name rather than on what it refines: the parser
/// keeps no id for `dc:description`, and nothing else in the wild calls itself
/// a long description. The value is markup either way — a description arriving
/// as HTML is the normal case, not this convention's doing.
fn long_description(properties: &[MetadataProperty]) -> Option<String> {
    let property = properties.iter().find(|property| property.property.rsplit(':').next() == Some("long-description"))?;
    let collapsed = property.value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

pub fn open_epub(path: &Path) -> MetadataResult<Book> {
    let provider = Book::open_reader(ContainerFormat::Epub, std::fs::File::open(path)?)?;
    Ok(provider)
}

pub fn metadata_from_book(path: &Path, provider: &mut Book) -> MetadataResult<BookRecord> {
    metadata_from_declared(path, provider.metadata()?)
}

fn metadata_from_declared(path: &Path, metadata: DeclaredMetadata) -> MetadataResult<BookRecord> {
    let authors = metadata
        .creators
        .iter()
        .filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) == AUTHOR_MARC_RELATOR_CODE && !is_author_tool(&creator.name))
        .flat_map(|creator| author_names(&creator.name))
        .filter_map(author_from_field)
        .collect::<Vec<_>>();

    let mut identifiers = metadata
        .identifiers
        .into_iter()
        .filter_map(|identifier| {
            let mut scheme = match identifier.scheme {
                IdentifierScheme::Unspecified => Scheme::Unspecified,
                IdentifierScheme::ISBN => Scheme::Isbn,
                IdentifierScheme::ASIN => Scheme::Asin,
                IdentifierScheme::DOI => Scheme::Doi,
                IdentifierScheme::ISSN => Scheme::Issn,
                IdentifierScheme::OCLC => Scheme::Oclc,
                IdentifierScheme::LCCN => Scheme::Lccn,
                IdentifierScheme::UUID => Scheme::Uuid,
                IdentifierScheme::CALIBRE => Scheme::Calibre,
                IdentifierScheme::GOOG => Scheme::Google,
                IdentifierScheme::GOODREADS => Scheme::Goodreads,
                IdentifierScheme::KOBO => Scheme::Kobo,
                IdentifierScheme::Other(value) => Scheme::Other(value),
            };
            // Some producers emit a bare, checksum-valid ISBN without a
            // scheme, or attach an incorrect proprietary scheme. The value is
            // stronger evidence than that missing/mislabelled declaration.
            let mut value = identifier.value;
            if let Some(isbn) = from_metadata_value(&value) {
                value = isbn;
                if scheme != Scheme::Isbn {
                    scheme = Scheme::Isbn;
                }
            } else if scheme == Scheme::Isbn {
                // Preserve the raw declaration without treating invalid placeholders
                // as usable ISBN identity or suppressing fallback extraction.
                scheme = Scheme::Unspecified;
            } else if scheme == Scheme::Asin {
                let asin = value.trim().to_ascii_uppercase();
                if asin.len() != 10 || !asin.starts_with('B') || !asin.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                    return None;
                }
                value = asin;
            }
            Some(Identifier::new(value, scheme, Scope::Book).map_err(Into::into))
        })
        .collect::<Result<Vec<_>, BookMetadataError>>()?;
    let contributor = |creator: MetadataCredit| Contributor::new(creator.name, marc_relator_code_from_metadata_role(creator.role.as_str())).ok();
    let non_author_contributors =
        metadata.contributors.into_iter().chain(metadata.creators.into_iter().filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) != AUTHOR_MARC_RELATOR_CODE)).filter_map(contributor).collect::<Vec<_>>();
    let long_description = long_description(&metadata.properties);
    for property in &metadata.properties {
        if let Some(isbn) = from_metadata_value(&property.value) {
            book_model::push_isbn(&mut identifiers, isbn, Scope::Book)?;
        }
    }
    if let Some(isbn) = metadata.source_path.as_deref().and_then(|path: &str| -> Option<String> {
        let file_name = path.rsplit('/').next()?;
        let stem = file_name.strip_suffix(".opf").or_else(|| file_name.strip_suffix(".OPF"))?;
        let stem = stem.trim().trim_start_matches(|character: char| matches!(character, '_' | '-'));
        let stem = stem.strip_prefix("ISBN").or_else(|| stem.strip_prefix("isbn")).map(|value| value.trim_start_matches(|character: char| matches!(character, '_' | '-' | ':' | ' '))).unwrap_or(stem);
        filename_isbn::from_filename_single(stem)
    }) {
        book_model::push_isbn(&mut identifiers, isbn, Scope::Book)?;
    }
    let title = if metadata.title.trim().is_empty() { fallback_metadata(path).metadata.title } else { metadata.title };
    let subtitle = metadata.alternate_titles.iter().find(|title| title.title_type.as_deref().is_some_and(|kind| kind.eq_ignore_ascii_case("subtitle"))).map(|title| title.value.clone());
    let mut book = BookMetadata {
        identifiers,
        publishers: metadata.publishers,
        // Optional source metadata must not reject an otherwise readable book.
        languages: metadata.languages.into_iter().filter_map(|value| LanguageTag::parse(value).ok()).collect(),
        year: unique_book_year(
            metadata.dates.iter().filter(|date| date.event.as_deref().is_none_or(|event| event.trim().is_empty() || event.eq_ignore_ascii_case("publication") || event.eq_ignore_ascii_case("book"))).map(|date| date.value.as_str()),
        ),
        subjects: metadata
            .subjects
            .into_iter()
            .filter_map(|subject| {
                let mut authority = subject.authority;
                let mut code = subject.code;
                if authority.is_none() {
                    if evidence::usable_lcc(&subject.value) {
                        authority = Some("lcc".to_owned());
                        code = Some(subject.value.clone());
                    } else if !subject_projection::unified_subject_paths(subject_projection::BISAC_SYSTEM_ID, &subject.value).is_empty() {
                        authority = Some("bisac".to_owned());
                        code = Some(subject.value.clone());
                    }
                }
                BookSubject::new(subject.id, subject.value, subject.source, authority, code).ok()
            })
            .collect(),
    };
    book.infer_embedded_subject_isbns()?;
    let contributors = authors.into_iter().chain(non_author_contributors).collect();
    let description = long_description.unwrap_or(metadata.description);
    let mut book = BookRecord { metadata: book_model::BookDetails { title, subtitle, contributors, book }, description };
    book.normalize_title();
    Ok(book)
}

use book_model::from_metadata_value;

#[cfg(test)]
mod tests {
    use super::*;
    use book_content::CreatorRole;
    use book_model::MarcRelatorCode;

    #[test]
    fn declared_non_author_creators_are_never_promoted_to_authors() {
        for role in ["trl", "edt", "ill", "ctb"] {
            let credit = MetadataCredit { role: CreatorRole::new(role), ..MetadataCredit::author("A Contributor".into()) };
            for include_author in [false, true] {
                let mut metadata = DeclaredMetadata { title: "Credits".into(), creators: vec![credit.clone()], ..Default::default() };
                if include_author {
                    metadata.creators.push(MetadataCredit::author("An Author".into()));
                }
                metadata.contributors.push(MetadataCredit { role: CreatorRole::contributor(), ..MetadataCredit::author("Untyped Credit".into()) });
                let record = metadata_from_declared(Path::new("book.epub"), metadata).unwrap();
                let authors = record.authors().map(|author| author.name()).collect::<Vec<_>>();
                assert_eq!(authors, if include_author { vec!["An Author"] } else { Vec::new() }, "{role}");
                let contributor = record.contributors.iter().find(|credit| credit.name() == "A Contributor").unwrap();
                assert_eq!(contributor.role_code().as_str(), role);
                assert_eq!(record.contributors.iter().find(|credit| credit.name() == "Untyped Credit").unwrap().role_code(), MarcRelatorCode(*b"ctb"));
            }
        }
    }

    #[test]
    fn page_inspection_without_images_yields_no_identifiers() {
        // The fixture has 32 text-only pages, identifier-free metadata, and
        // no images, so page inspection has nothing to promote.
        let bytes = include_bytes!("../tests/fixtures/ocr-image-priority.epub");
        let inspected = inspect_epub_pages_reader(Path::new("book.epub"), std::io::Cursor::new(bytes.to_vec())).unwrap();
        assert_eq!(inspected.title, "OCR priority fixture");
        assert!(inspected.book.identifiers.is_empty());
    }

    #[test]
    fn epub_end_cip_is_shared_by_file_and_seekable_reader_paths() {
        let bytes = include_bytes!("../tests/fixtures/back-matter-evidence.epub");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.epub");
        std::fs::write(&path, bytes).unwrap();
        let native = inspect_epub(&path).unwrap().metadata;
        let portable = inspect_epub_reader(&path, std::io::Cursor::new(bytes.to_vec())).unwrap().metadata;
        assert_eq!(native.book.identifiers, portable.book.identifiers);
        assert_eq!(native.book.subjects, portable.book.subjects);
        assert_eq!(native.book.identifiers.len(), 1);
        assert_eq!(native.book.identifiers[0].canonical_value().as_deref(), Some("9780393243277"));
        assert_eq!(native.book.subjects[0].code(), Some("Q125 .A12 2023"));
        assert!(native.book.subjects[0].source().contains("p23.xhtml"));
    }

    #[test]
    fn malformed_optional_languages_do_not_reject_epub() {
        let bytes = include_bytes!("../tests/fixtures/malformed-language.epub").to_vec();
        let inspected = inspect_epub_reader(Path::new("book.epub"), std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(inspected.metadata.title, "Language metadata test");
        assert_eq!(inspected.metadata.book.languages.iter().map(LanguageTag::as_str).collect::<Vec<_>>(), ["en-US", "sv"]);
    }
}

fn inspect_epub_pages(provider: &mut Book, book: &mut BookRecord) {
    let sections = document_evidence::inspect(provider, book);
    evidence::apply_evidence(book, &sections, "epub");
}

/// Page evidence alone, for inspection tools.
pub fn inspect_epub_pages_reader<R: Read + Seek + Send + 'static>(source_name: &Path, reader: R) -> MetadataResult<BookRecord> {
    let mut provider = Book::open_reader(ContainerFormat::Epub, reader)?;
    let mut book = metadata_from_book(source_name, &mut provider)?;
    book.book.identifiers.clear();
    inspect_epub_pages(&mut provider, &mut book);
    Ok(book)
}

/// Package metadata and navigation only; page inspection is a separate stage.
pub fn inspect_epub_metadata_with_toc(path: &Path) -> MetadataResult<InspectedEpub> {
    let mut provider = open_epub(path)?;
    let metadata = metadata_from_book(path, &mut provider)?;
    let toc = book_toc_entries(provider.toc()?.unwrap_or_default());
    Ok(InspectedEpub { metadata, toc, timings: EpubInspectionTimings::default() })
}

#[cfg(test)]
mod title_tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn metadata(titles: &str) -> BookRecord {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opf = format!(
            r#"<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata>{titles}</metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#
        );
        for (name, text) in [("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="book.opf"/></rootfiles></container>"#), ("book.opf", &opf), ("chapter.xhtml", "<html><body>Chapter</body></html>")] {
            archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            archive.write_all(text.as_bytes()).unwrap();
        }
        let mut provider = Book::open_reader(ContainerFormat::Epub, archive.finish().unwrap()).unwrap();
        metadata_from_book(Path::new("fixture.epub"), &mut provider).unwrap()
    }

    #[test]
    fn epub_publisher_names_are_preserved_as_strings() {
        let book = metadata("<dc:title>Example</dc:title><dc:publisher>Oxford University Press</dc:publisher><dc:publisher>Éditions Gallimard</dc:publisher>");
        assert_eq!(book.book.publishers, vec!["Oxford University Press", "Éditions Gallimard"]);
    }

    #[test]
    fn epub_year_uses_publication_dates_and_leaves_missing_or_conflicting_years_absent() {
        for (dates, expected) in [
            ("<dc:date>2004-03-15</dc:date>", Some(2004)),
            (r#"<dc:date event="modification">2025-01-01</dc:date><dc:date event="publication">2004-03-15</dc:date>"#, Some(2004)),
            ("<dc:date>2004</dc:date><dc:date>2004-03-15</dc:date>", Some(2004)),
            ("<dc:date>2004</dc:date><dc:date>2005</dc:date>", None),
            (r#"<dc:date event="modification">2025-01-01</dc:date>"#, None),
            ("<dc:date>unknown</dc:date>", None),
            ("", None),
        ] {
            let book = metadata(&format!("<dc:title>Example</dc:title>{dates}"));
            assert_eq!(book.book.year, expected, "{dates}");
        }
    }

    #[test]
    fn epub_colon_fallback_does_not_confuse_alternate_titles_with_subtitles() {
        let book = metadata("<dc:title>Fluid: The Approach Applied by Geniuses</dc:title><dc:title>Alternative title</dc:title>");
        assert_eq!(book.title, "Fluid");
        assert_eq!(book.subtitle(), Some("The Approach Applied by Geniuses"));
    }

    #[test]
    fn epub_explicit_subtitle_preserves_colon_in_main_title() {
        let book = metadata(r##"<dc:title>Star Trek: Voyager</dc:title><dc:title id="sub">A Guide</dc:title><meta refines="#sub" property="title-type">subtitle</meta>"##);
        assert_eq!(book.title, "Star Trek: Voyager");
        assert_eq!(book.subtitle(), Some("A Guide"));
    }
}
