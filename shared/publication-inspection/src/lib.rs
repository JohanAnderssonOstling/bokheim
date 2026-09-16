//! Portable EPUB and PDF metadata inspection.

use epub_provider::EpubProvider;
use publication_model::{
    canonical_isbn, marc_relator_code_from_metadata_role, BookMetadataState, Collection, CollectionKind, Contributor, Identifier, LanguageTag, PublicationDate, PublicationMetadata, PublicationMetadataError, PublicationProperty,
    PublicationSubject, PublicationTitle, PublicationTocEntry, PublisherCredit, Scheme, Scope, AUTHOR_MARC_RELATOR_CODE,
};
use std::io::{Read, Seek};
use std::path::Path;
use std::time::Instant;

pub type MetadataResult<T> = Result<T, Box<dyn std::error::Error>>;

pub mod evidence;
#[cfg(feature = "mobi")]
mod mobi;
#[cfg(feature = "mobi")]
pub use mobi::{inspect_mobi_metadata_reader, inspect_mobi_pages_reader};
mod epub_evidence;
#[cfg(not(target_arch = "wasm32"))]
mod native_ocr;
pub use publication_enrichment::related_isbn;
pub use publication_enrichment::title_identity::normalize_with_filename;
mod pdf;
mod pdf_ranges;
pub use pdf::{inspect_pdf, inspect_pdf_reader, inspect_pdf_pages_reader, inspect_pdf_metadata, inspect_pdf_ocr_reader, InspectedPdf};

/// Reads PDF identity dictionaries without loading page or image streams.
pub fn pdf_identity_catalog(reader: impl Read + Seek) -> MetadataResult<lopdf::Document> {
    pdf_ranges::catalog(reader)
}

pub struct InspectedEpub {
    pub metadata: BookMetadataState,
    pub toc: Vec<PublicationTocEntry>,
    pub timings: EpubInspectionTimings,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EpubInspectionTimings {
    pub container_ms: u64,
    pub evidence_ms: u64,
    pub toc_ms: u64,
}

/// Reads package metadata and navigation from one EPUB provider instance.
/// Catalogue ingestion uses this instead of maintaining a second EPUB
/// metadata parser; ordinary library scans can continue to omit the TOC.
pub fn inspect_epub(path: &Path) -> MetadataResult<InspectedEpub> {
    let container_started = Instant::now();
    let provider = open_epub(path)?;
    let mut metadata = epub_metadata_from_provider(path, &provider)?;
    let container_ms = u64::try_from(container_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let evidence_started = Instant::now();
    inspect_epub_evidence(&provider, &mut metadata);
    let evidence_ms = u64::try_from(evidence_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let toc_started = Instant::now();
    let mut count = 0;
    let toc = provider.toc()?.unwrap_or_default().into_iter().filter_map(|entry| publication_toc_entry(entry, 0, &mut count)).collect();
    let toc_ms = u64::try_from(toc_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(InspectedEpub { metadata, toc, timings: EpubInspectionTimings { container_ms, evidence_ms, toc_ms } })
}

/// Cheap EPUB metadata only: no page evidence extraction or OCR.
pub fn inspect_epub_metadata(path: &Path) -> MetadataResult<BookMetadataState> {
    epub_metadata_from_provider(path, &open_epub(path)?)
}

/// Reads EPUB metadata from any seekable byte source. Browser ingestion uses
/// this entry point, while native scanning passes a file through
/// [`inspect_epub`]; both therefore share the complete metadata projection.
pub fn inspect_epub_reader<R>(source_name: &Path, reader: R) -> MetadataResult<InspectedEpub>
where
    R: Read + Seek + Send + 'static,
{
    let provider = EpubProvider::try_from_reader(reader)?;
    if provider.document_uris()?.is_empty() {
        return Err("EPUB publication contains no readable documents".into());
    }
    let mut metadata = epub_metadata_from_provider(source_name, &provider)?;
    inspect_epub_evidence(&provider, &mut metadata);
    let mut count = 0;
    let toc = provider.toc()?.unwrap_or_default().into_iter().filter_map(|entry| publication_toc_entry(entry, 0, &mut count)).collect();
    Ok(InspectedEpub { metadata, toc, timings: EpubInspectionTimings::default() })
}

fn publication_toc_entry(entry: epub_provider::TocEntry, depth: usize, count: &mut usize) -> Option<PublicationTocEntry> {
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
    let children = entry.children.into_iter().filter_map(|entry| publication_toc_entry(entry, depth + 1, count)).collect();
    Some(PublicationTocEntry { title, target, children })
}

/// Reads all metadata relevant to indexing. Formats without useful embedded
fn fallback_metadata(path: &Path) -> BookMetadataState {
    let title = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned();
    BookMetadataState { title, contributors: Vec::new(), description: String::new(), publication: Default::default() }
}

fn author_credit(name: String, sort_name: Option<String>) -> Result<Contributor, PublicationMetadataError> {
    Contributor::new(name, sort_name, AUTHOR_MARC_RELATOR_CODE)
}

/// One credit from one cleaned author field.
///
/// A catalogue files people under the family name, so "Yakobson, Alexander"
/// becomes "Alexander Yakobson" for display while the filing form is kept as
/// the contributor's sort name — the index still sorts under Y, and the page
/// still reads as the person writes it. Anything that does not plainly invert
/// is stored exactly as stated.
fn author_from_field(name: String) -> Option<Contributor> {
    match publication_model::given_name_first(&name) {
        Some(display) => author_credit(display, Some(name)).ok(),
        None => author_credit(name, None).ok(),
    }
}

/// Producers that write themselves into an author field. A conversion tool is
/// not a person, and a record naming one is worse than a record naming
/// nobody: it files books under "Adobe Acrobat" in the author index.
const AUTHOR_TOOL_PREFIXES: &[&str] = &[
    "adobe", "acrobat", "abbyy", "pscript", "latex", "microsoft", "word", "pdfcreator", "ghostscript", "quark", "indesign",
    "administrator", "unknown", "untitled", "author", "user", "owner", "none", "scanner", "www.", "http",
];

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
        .map(|part| clean_author_name(&part))
        .filter(|name| !name.is_empty() && !is_author_tool(name))
        .collect()
}

/// Strips what catalogues and tools leave behind: a role marker in brackets, a
/// trailing life date, and the full stop that abbreviated catalogue entries end
/// with. What remains is the name as a person would write it.
fn clean_author_name(value: &str) -> String {
    let mut name = value.trim().to_owned();
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
}

fn is_author_tool(name: &str) -> bool {
    let lowered = name.trim().to_ascii_lowercase();
    publication_enrichment::filename_identity::unusable_title(name)
        || AUTHOR_TOOL_PREFIXES.iter().any(|prefix| lowered.starts_with(prefix))
}

fn non_empty_optional(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

/// The blurb a reader should be given, where a publication states two.
///
/// Standard Ebooks writes a one-line teaser in `dc:description` — "A wealthy
/// young woman decides to take on the role of patroness and matchmaker" — and
/// the real blurb in a `se:long-description` refinement beside it, in
/// paragraphs. The short line is written to fit a catalogue row; a book's own
/// page has room for the one that says something.
///
/// Read from the publication's own properties rather than from the modelled
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
fn long_description(properties: &[epub_provider::EpubMetadataProperty]) -> Option<String> {
    let property = properties.iter().find(|property| property.property.rsplit(':').next() == Some("long-description"))?;
    let collapsed = property.value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

fn publication_properties(properties: Vec<epub_provider::EpubMetadataProperty>) -> Vec<PublicationProperty> {
    // EPUB package metadata is extensible. A malformed, oversized, or
    // unsupported optional property must not make core metadata such as title
    // and authors unusable. Keep every property that fits the synchronized
    // model and omit only the property that does not.
    properties
        .into_iter()
        .filter_map(|property| {
            PublicationProperty::new(non_empty_optional(property.id), property.property, property.value, non_empty_optional(property.refines), non_empty_optional(property.scheme), non_empty_optional(property.language), "epub:package:meta")
                .ok()
        })
        .collect()
}
fn open_epub(path: &Path) -> MetadataResult<EpubProvider> {
    let provider = std::fs::File::open(path).and_then(EpubProvider::try_from_reader)?;
    if provider.document_uris()?.is_empty() {
        return Err("EPUB publication contains no readable documents".into());
    }
    Ok(provider)
}

fn epub_metadata_from_provider(path: &Path, provider: &EpubProvider) -> MetadataResult<BookMetadataState> {
    let metadata = provider.metadata()?;
    let mut authors = metadata
        .creators
        .iter()
        .filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) == AUTHOR_MARC_RELATOR_CODE && !is_author_tool(&creator.name))
        .map(|creator| author_credit(creator.name.clone(), creator.file_as.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    if authors.is_empty() {
        authors.extend(metadata.creators.first().filter(|creator| !is_author_tool(&creator.name)).map(|creator| author_credit(creator.name.clone(), creator.file_as.clone())).transpose()?);
    }

    let mut identifiers = metadata
        .identifiers
        .into_iter()
        .map(|identifier| {
            let mut scheme = match identifier.scheme {
                epub_provider::IdentifierScheme::Unspecified => Scheme::Unspecified,
                epub_provider::IdentifierScheme::ISBN => Scheme::Isbn,
                epub_provider::IdentifierScheme::ASIN => Scheme::Asin,
                epub_provider::IdentifierScheme::DOI => Scheme::Doi,
                epub_provider::IdentifierScheme::ISSN => Scheme::Issn,
                epub_provider::IdentifierScheme::OCLC => Scheme::Oclc,
                epub_provider::IdentifierScheme::LCCN => Scheme::Lccn,
                epub_provider::IdentifierScheme::UUID => Scheme::Uuid,
                epub_provider::IdentifierScheme::CALIBRE => Scheme::Calibre,
                epub_provider::IdentifierScheme::GOOG => Scheme::Google,
                epub_provider::IdentifierScheme::GOODREADS => Scheme::Goodreads,
                epub_provider::IdentifierScheme::KOBO => Scheme::Kobo,
                epub_provider::IdentifierScheme::Other(value) => Scheme::Other(value),
            };
            let mut source = "epub:package:identifier";
            // Some producers emit a bare, checksum-valid ISBN without a
            // scheme, or attach an incorrect proprietary scheme. The value is
            // stronger evidence than that missing/mislabelled declaration.
            let mut value = identifier.value;
            if let Some(isbn) = metadata_isbn(&value) {
                value = isbn;
                if scheme != Scheme::Isbn {
                scheme = Scheme::Isbn;
                source = "epub:checksum-inferred";
                }
            } else if scheme == Scheme::Isbn {
                // Preserve the raw declaration without treating invalid placeholders
                // as usable ISBN identity or suppressing fallback extraction.
                scheme = Scheme::Unspecified;
            }
            Identifier::new(value, scheme, Scope::Publication, source).map_err(Into::into)
        })
        .collect::<Result<Vec<_>, PublicationMetadataError>>()?;
    let contributor = |creator: epub_provider::EpubCreator| Contributor::new(creator.name, creator.file_as, marc_relator_code_from_metadata_role(creator.role.as_str()));
    let non_author_contributors =
        metadata.contributors.into_iter().chain(metadata.creators.into_iter().filter(|creator| marc_relator_code_from_metadata_role(creator.role.as_str()) != AUTHOR_MARC_RELATOR_CODE)).map(contributor).collect::<Result<Vec<_>, _>>()?;
    let long_description = long_description(&metadata.properties);
    let properties = publication_properties(metadata.properties);
    if let Some(isbn) = isbn_from_opf_path(provider.package_document_path()) {
        push_epub_isbn(&mut identifiers, isbn, "opf-filename")?;
    }
    let collections = properties
        .iter()
        .filter(|property| property.property().rsplit(':').next() == Some("belongs-to-collection"))
        .filter_map(|property| {
            let target = property.id().map(|id| format!("#{id}"));
            let refinement = |name: &str| properties.iter().find(|candidate| candidate.refines() == target.as_deref() && candidate.property().rsplit(':').next() == Some(name)).map(|candidate| candidate.value().to_owned());
            let kind = refinement("collection-type").and_then(|value| CollectionKind::from_label(value).ok());
            Collection::new(property.value(), kind, refinement("group-position"), "epub:package:belongs-to-collection").ok()
        })
        .collect();
    let title = if metadata.title.trim().is_empty() { fallback_metadata(path).title } else { metadata.title };
    let mut publication = PublicationMetadata {
        identifiers,
        publishers: metadata.publishers.into_iter().map(PublisherCredit::new).collect::<Result<Vec<_>, _>>()?,
        // Optional source metadata must not reject an otherwise readable EPUB.
        languages: metadata.languages.into_iter().filter_map(|value| LanguageTag::parse(value).ok()).collect(),
        dates: metadata.dates.into_iter().filter_map(|date| PublicationDate::new(non_empty_optional(date.id), date.value, non_empty_optional(date.event), "epub:package:date").ok()).collect(),
        subjects: metadata.subjects.into_iter().map(|subject| PublicationSubject::new(subject.id, subject.value, subject.source, subject.authority, subject.code)).collect::<Result<Vec<_>, _>>()?,
        collections,
        alternate_titles: metadata.alternate_titles.into_iter().map(|title| PublicationTitle::new(title.id, title.value, title.title_type, title.sort_as, "epub:package:title")).collect::<Result<Vec<_>, _>>()?,
        rights: metadata.rights,
        sources: metadata.sources,
        types: metadata.types,
        formats: metadata.formats,
        relations: metadata.relations,
        coverage: metadata.coverage,
        properties,
    };
    publication.infer_embedded_subject_isbns()?;
    let contributors = authors.into_iter().chain(non_author_contributors).collect();
    let description = long_description.unwrap_or(metadata.description);
    let mut book = BookMetadataState { title, contributors, description, publication };
    book.normalize_title();
    Ok(book)
}

use publication_enrichment::metadata_isbn;

fn isbn_from_opf_path(path: &str) -> Option<String> {
    let file_name = path.rsplit('/').next()?;
    let stem = file_name.strip_suffix(".opf").or_else(|| file_name.strip_suffix(".OPF"))?;
    let stem = stem.trim().trim_start_matches(|character: char| matches!(character, '_' | '-'));
    let stem = stem.strip_prefix("ISBN").or_else(|| stem.strip_prefix("isbn")).map(|value| value.trim_start_matches(|character: char| matches!(character, '_' | '-' | ':' | ' '))).unwrap_or(stem);
    canonical_isbn(stem)
}

fn push_epub_isbn(identifiers: &mut Vec<Identifier>, isbn: String, provenance: &str) -> Result<(), PublicationMetadataError> {
    if identifiers.iter().any(|identifier| identifier.scheme() == &Scheme::Isbn && identifier.canonical_value().as_deref() == Some(isbn.as_str())) {
        return Ok(());
    }
    identifiers.push(Identifier::new(isbn, Scheme::Isbn, Scope::Publication, provenance)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_page_inspection_never_runs_ocr() {
        let bytes = include_bytes!("../tests/fixtures/ocr-image-priority.epub");
        let inspected = inspect_epub_pages_reader(Path::new("book.epub"), std::io::Cursor::new(bytes.to_vec())).unwrap();
        assert!(inspected.publication.properties.iter().any(|p| p.property() == "inspection:page-pass"));
        assert!(!inspected.publication.properties.iter().any(|p| p.property().starts_with("inspection:ocr")));
    }

    #[test]
    fn a_long_description_beside_a_teaser_is_the_one_a_reader_gets() {
        let property = |name: &str, value: &str| epub_provider::EpubMetadataProperty {
            id: Some("long-description".to_owned()),
            property: name.to_owned(),
            value: value.to_owned(),
            refines: Some("#description".to_owned()),
            scheme: None,
            language: None,
        };
        // Indented across several lines, which is how Standard Ebooks writes it
        // and why the modelled property cannot carry it.
        let emma = property("se:long-description", "\n\t\t\t<p><i>Emma</i> is one of Jane Austen’s best-loved novels.</p>\n\t\t\t<p>The novel provides a light-hearted insight.</p>\n\t\t");
        assert_eq!(
            long_description(std::slice::from_ref(&emma)).as_deref(),
            Some("<p><i>Emma</i> is one of Jane Austen’s best-loved novels.</p> <p>The novel provides a light-hearted insight.</p>"),
            "the paragraphs survive; the indentation between them does not"
        );

        let other = property("belongs-to-collection", "Novels");
        assert_eq!(long_description(&[other.clone()]), None, "a publication that states one description keeps it");
        assert_eq!(long_description(&[]), None);
        assert_eq!(long_description(&[other, property("se:long-description", " \n\t ")]), None, "a refinement that says nothing is not an improvement on the line that does");
    }

    #[test]
    fn malformed_identifier_wrapper_retains_only_complete_isbn_values() {
        assert_eq!(metadata_isbn("urn:uuid:isbn 0-330-26272-6").as_deref(), Some("0330262726"));
        assert_eq!(metadata_isbn("URN:UUID:ISBN: 0-330-26272-6").as_deref(), Some("0330262726"));
        assert!(metadata_isbn("urn:uuid:12345678-1234-1234-1234-123456789abc").is_none());
        assert!(metadata_isbn("https://example.org/0330262726").is_none());
        assert!(metadata_isbn("urn:uuid:isbn 0-330-26272-7").is_none());
    }

    #[test]
    fn publisher_resource_numbers_are_separate_verified_lookup_hints() {
        let paths = ["OEBPS/actrade-9780192802231-chapter-1.xhtml", "OEBPS/actrade-9780192802231-chapter-2.xhtml"].map(str::to_owned);
        assert_eq!(resource_isbn_hints(&paths), ["9780192802231"]);
        assert!(resource_isbn_hints(&paths[..1]).is_empty());
        assert!(resource_isbn_hints(&[paths[0].clone(), paths[0].clone()]).is_empty());
        let property = PublicationProperty::new(None, "inspection:resource-isbn-candidates", "[\"9780192802231\"]", None, None, None, "inspection:epub-resource").unwrap();
        assert_eq!(related_isbn::resource_candidates(std::slice::from_ref(&property)), ["9780192802231"]);
        assert!(related_isbn::lookup_candidates(&[property]).is_empty());
    }

    #[test]
    fn epub_end_cip_is_shared_by_file_and_seekable_reader_paths() {
        let bytes = include_bytes!("../tests/fixtures/back-matter-evidence.epub");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.epub");
        std::fs::write(&path, bytes).unwrap();
        let native = inspect_epub(&path).unwrap().metadata;
        let portable = inspect_epub_reader(&path, std::io::Cursor::new(bytes.to_vec())).unwrap().metadata;
        assert_eq!(native.publication.identifiers, portable.publication.identifiers);
        assert_eq!(native.publication.subjects, portable.publication.subjects);
        assert_eq!(native.publication.identifiers.len(), 1);
        assert_eq!(native.publication.identifiers[0].canonical_value().as_deref(), Some("9780393243277"));
        assert_eq!(native.publication.subjects[0].code(), Some("Q125 .A12 2023"));
        assert!(native.publication.subjects[0].source().contains("p23.xhtml"));
    }

    #[test]
    fn malformed_optional_languages_do_not_reject_epub() {
        let bytes = include_bytes!("../tests/fixtures/malformed-language.epub").to_vec();
        let inspected = inspect_epub_reader(Path::new("book.epub"), std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(inspected.metadata.title, "Language metadata test");
        assert_eq!(inspected.metadata.publication.languages.iter().map(LanguageTag::as_str).collect::<Vec<_>>(), ["en-US", "sv"]);
    }

    #[test]
    fn opf_filename_isbn_requires_a_valid_checksum() {
        assert_eq!(isbn_from_opf_path("OPS/9781501168697.opf").as_deref(), Some("9781501168697"));
        assert_eq!(isbn_from_opf_path("OPS/ISBN_978-1-5011-6869-7.OPF").as_deref(), Some("9781501168697"));
        assert_eq!(isbn_from_opf_path("OPS/9781501168698.opf"), None);
        assert_eq!(isbn_from_opf_path("OPS/package.opf"), None);
    }
}

fn inspect_epub_evidence(provider: &EpubProvider, book: &mut BookMetadataState) {
    if publication_enrichment::defer_initial_page_inspection(&book.publication) { return; }
    inspect_epub_pages(provider, book);
}

fn inspect_epub_pages(provider: &EpubProvider, book: &mut BookMetadataState) {
    let paths = provider.document_uris().unwrap_or_default();
    let hints = resource_isbn_hints(&paths);
    if !hints.is_empty() {
        if let Ok(property) = PublicationProperty::new(None, "inspection:resource-isbn-candidates", serde_json::to_string(&hints).unwrap(), None, None, None, "inspection:epub-resource") {
            book.publication.properties.push(property);
        }
    }
    let sections = epub_evidence::inspect(provider, book);
    evidence::apply_evidence(book, &sections, "epub");
    record_page_pass(book);
}

fn resource_isbn_hints(paths: &[String]) -> Vec<String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for path in paths.iter().take(8192).collect::<std::collections::BTreeSet<_>>() {
        let Some(tail) = path.rsplit('/').next().and_then(|name| name.strip_prefix("actrade-")) else { continue };
        let Some((number, _)) = tail.split_once('-') else { continue };
        if number.len() == 13 && (number.starts_with("978") || number.starts_with("979")) {
            if let Some(isbn) = canonical_isbn(number) { *counts.entry(isbn).or_default() += 1; }
        }
    }
    // Repeated publisher document IDs are lookup hints, never publication identity.
    counts.into_iter().filter(|(_, count)| *count >= 2).map(|(isbn, _)| isbn).take(16).collect()
}

/// Text-only fallback after metadata ISBN lookup produced no usable subject.
/// File metadata is retained by the caller. OCR is a separate final stage.
pub fn inspect_epub_pages_reader<R: Read + Seek + Send + 'static>(source_name: &Path, reader: R) -> MetadataResult<BookMetadataState> {
    let provider = EpubProvider::try_from_reader(reader)?;
    let mut book = epub_metadata_from_provider(source_name, &provider)?;
    book.publication.identifiers.clear();
    inspect_epub_pages(&provider, &mut book);
    Ok(book)
}

/// Explicit last-resort OCR. Earlier text/metadata ISBN misses must not suppress
/// searching images for an additional ISBN. The caller merges evidence only.
pub fn inspect_epub_ocr_reader<R: Read + Seek + Send + 'static>(source_name: &Path, reader: R) -> MetadataResult<BookMetadataState> {
    let provider = EpubProvider::try_from_reader(reader)?;
    let mut book = epub_metadata_from_provider(source_name, &provider)?;
    book.publication.identifiers.clear();
    #[cfg(not(target_arch = "wasm32"))]
    native_ocr::inspect_epub(&provider, &mut book);
    record_ocr_pass(&mut book);
    Ok(book)
}

pub(crate) fn record_ocr_pass(book: &mut BookMetadataState) {
    if let Ok(property) = PublicationProperty::new(None, "inspection:ocr-pass", evidence::INSPECTION_VERSION.to_string(), None, None, None, "inspection:local") {
        if !book.publication.properties.contains(&property) { book.publication.properties.push(property); }
    }
}

pub(crate) fn record_page_pass(book: &mut BookMetadataState) {
    if let Ok(property) = PublicationProperty::new(None, "inspection:page-pass", evidence::INSPECTION_VERSION.to_string(), None, None, None, "inspection:local") {
        if !book.publication.properties.contains(&property) { book.publication.properties.push(property); }
    }
}

/// Package metadata and navigation only; page inspection is a separate stage.
pub fn inspect_epub_metadata_with_toc(path: &Path) -> MetadataResult<InspectedEpub> {
    let provider = open_epub(path)?;
    let metadata = epub_metadata_from_provider(path, &provider)?;
    let mut count = 0;
    let toc = provider.toc()?.unwrap_or_default().into_iter().filter_map(|entry| publication_toc_entry(entry, 0, &mut count)).collect();
    Ok(InspectedEpub { metadata, toc, timings: EpubInspectionTimings::default() })
}

#[cfg(test)]
mod title_tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn metadata(titles: &str) -> BookMetadataState {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opf = format!(r#"<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata>{titles}</metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#);
        for (name, text) in [
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="book.opf"/></rootfiles></container>"#),
            ("book.opf", &opf), ("chapter.xhtml", "<html><body>Chapter</body></html>"),
        ] {
            archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            archive.write_all(text.as_bytes()).unwrap();
        }
        let provider = EpubProvider::try_from_reader(archive.finish().unwrap()).unwrap();
        epub_metadata_from_provider(Path::new("fixture.epub"), &provider).unwrap()
    }

    #[test]
    fn epub_declared_subtitle_is_stored_separately_without_needing_a_colon() {
        let book = metadata(r##"<dc:title id="main">Moby Dick</dc:title><dc:title id="sub">Or, The Whale</dc:title><meta refines="#sub" property="title-type">subtitle</meta>"##);
        assert_eq!(book.title, "Moby Dick");
        assert_eq!(book.subtitle(), Some("Or, The Whale"));
    }

    #[test]
    fn epub_colon_fallback_does_not_confuse_alternate_titles_with_subtitles() {
        let book = metadata("<dc:title>Fluid: The Approach Applied by Geniuses</dc:title><dc:title>Alternative title</dc:title>");
        assert_eq!(book.title, "Fluid");
        assert_eq!(book.subtitle(), Some("The Approach Applied by Geniuses"));
        assert!(book.publication.alternate_titles.iter().any(|title| title.value() == "Alternative title"));
    }

    #[test]
    fn epub_explicit_subtitle_preserves_colon_in_main_title() {
        let book = metadata(r##"<dc:title>Star Trek: Voyager</dc:title><dc:title id="sub">A Guide</dc:title><meta refines="#sub" property="title-type">subtitle</meta>"##);
        assert_eq!(book.title, "Star Trek: Voyager");
        assert_eq!(book.subtitle(), Some("A Guide"));
    }
}
