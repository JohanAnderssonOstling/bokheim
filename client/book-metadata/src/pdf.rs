use crate::{author_credit, fallback_metadata, MetadataResult};
use library_model::{
    canonical_doi, canonical_isbn, canonical_uuid, BookMetadataState, Contributor, LanguageTag, PublicationCollection, PublicationDate, PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType, PublicationMetadata,
    PublicationProperty, PublicationSubject, PublicationTitle, PublisherCredit, CONTRIBUTOR_MARC_RELATOR_CODE,
};
use lopdf::{Dictionary, Document};
use roxmltree::{Document as XmlDocument, Node};
use std::collections::HashSet;
use std::path::Path;

const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
const XMP_RIGHTS: &str = "http://ns.adobe.com/xap/1.0/rights/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";
const CALIBRE: &str = "http://calibre-ebook.com/xmp-namespace";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const MAX_VALUES: usize = 256;
const MAX_TEXT_BYTES: usize = 4_096;
const MAX_DESCRIPTION_BYTES: usize = 64 * 1_024;
const MAX_XMP_BYTES: usize = 8 * 1_024 * 1_024;
const MAX_EARLY_TEXT_BYTES: usize = 8 * 1_024 * 1_024;
const EARLY_TEXT_PAGES: usize = 20;

pub struct InspectedPdf {
    pub metadata: BookMetadataState,
}

/// Extracts bounded, allow-listed bibliographic metadata from the PDF info
/// dictionary and XMP packet. ISBNs in the first pages are accepted only after
/// checksum validation; arbitrary XMP extensions are never promoted.
pub fn inspect_pdf(path: &Path) -> MetadataResult<InspectedPdf> {
    let document = Document::load(path)?;
    let info = pdf_info_dictionary(&document);
    let xmp = pdf_xmp(&document);
    let parsed = xmp.as_deref().and_then(|xml| XmlDocument::parse(xml).ok());

    let title_values = parsed.as_ref().map(|xml| xmp_values(xml, DC, "title", MAX_TEXT_BYTES)).unwrap_or_default();
    let title = preferred_language_value(&title_values).or_else(|| info.and_then(|dictionary| dictionary_text(dictionary, b"Title"))).unwrap_or_else(|| fallback_metadata(path).title);
    let description_values = parsed.as_ref().map(|xml| xmp_values(xml, DC, "description", MAX_DESCRIPTION_BYTES)).unwrap_or_default();
    let description = preferred_language_value(&description_values).or_else(|| info.and_then(|dictionary| dictionary_text(dictionary, b"Subject"))).unwrap_or_default();

    let creator_values = parsed.as_ref().map(|xml| xmp_values(xml, DC, "creator", MAX_TEXT_BYTES)).unwrap_or_default();
    let mut contributors = creator_values.iter().filter_map(|value| author_credit(value.text.clone(), None).ok()).collect::<Vec<_>>();
    if contributors.is_empty() {
        contributors.extend(info.and_then(|dictionary| dictionary_text(dictionary, b"Author")).and_then(|name| author_credit(name, None).ok()));
    }
    if let Some(xml) = parsed.as_ref() {
        contributors.extend(xmp_values(xml, DC, "contributor", MAX_TEXT_BYTES).into_iter().filter_map(|value| Contributor::new(value.text, None, CONTRIBUTOR_MARC_RELATOR_CODE).ok()));
    }
    dedup_contributors(&mut contributors);

    let mut publication = PublicationMetadata::default();
    if let Some(xml) = parsed.as_ref() {
        publication.identifiers = xmp_identifiers(xml);
        publication.publishers = xmp_values(xml, DC, "publisher", MAX_TEXT_BYTES).into_iter().filter_map(|value| PublisherCredit::new(value.text).ok()).collect();
        publication.languages = xmp_values(xml, DC, "language", MAX_TEXT_BYTES).into_iter().filter_map(|value| LanguageTag::parse(value.text).ok()).collect();
        publication.dates = xmp_values(xml, DC, "date", MAX_TEXT_BYTES).into_iter().filter_map(|value| PublicationDate::new(None, value.text, Some("dc:date".to_owned())).ok()).collect();
        publication.subjects = xmp_values(xml, DC, "subject", MAX_TEXT_BYTES).into_iter().filter_map(|value| PublicationSubject::new(None, value.text, "pdf:xmp:dc:subject", None, None).ok()).collect();
        publication.rights = texts(xmp_values(xml, DC, "rights", MAX_DESCRIPTION_BYTES));
        publication.sources = texts(xmp_values(xml, DC, "source", MAX_TEXT_BYTES));
        publication.types = texts(xmp_values(xml, DC, "type", MAX_TEXT_BYTES));
        publication.formats = texts(xmp_values(xml, DC, "format", MAX_TEXT_BYTES));
        publication.relations = texts(xmp_values(xml, DC, "relation", MAX_TEXT_BYTES));
        publication.coverage = texts(xmp_values(xml, DC, "coverage", MAX_TEXT_BYTES));
        publication.alternate_titles = title_values.iter().filter(|value| value.text != title).filter_map(|value| PublicationTitle::new(None, &value.text, Some("alternative".to_owned()), None).ok()).collect();
        publication.collections = xmp_collections(xml);
        publication.properties = xmp_properties(xml);
    }

    let mut seen_isbns = publication.identifiers.iter().filter(|identifier| identifier.scheme() == &PublicationIdentifierScheme::Isbn).filter_map(PublicationIdentifier::canonical_value).collect::<HashSet<_>>();
    for isbn in early_page_isbns(&document) {
        if seen_isbns.insert(isbn.clone()) {
            let provenance = PublicationIdentifierType::new("early-page-text", Some("bokheim:provenance".to_owned())).ok().into_iter().collect();
            if let Ok(identifier) = PublicationIdentifier::new(isbn, PublicationIdentifierScheme::Isbn, provenance) {
                publication.identifiers.push(identifier);
            }
        }
    }
    dedup_publication(&mut publication);
    Ok(InspectedPdf { metadata: BookMetadataState { title, contributors, description, publication } })
}

#[derive(Clone)]
struct XmpValue {
    text: String,
    language: Option<String>,
    scheme: Option<String>,
}

fn pdf_info_dictionary(document: &Document) -> Option<&Dictionary> {
    document.dereference(document.trailer.get(b"Info").ok()?).ok()?.1.as_dict().ok()
}

fn dictionary_text(dictionary: &Dictionary, key: &[u8]) -> Option<String> {
    bounded_text(&lopdf::decode_text_string(dictionary.get(key).ok()?).ok()?, MAX_DESCRIPTION_BYTES)
}

fn pdf_xmp(document: &Document) -> Option<String> {
    let root = document.dereference(document.trailer.get(b"Root").ok()?).ok()?.1.as_dict().ok()?;
    let metadata = document.dereference(root.get(b"Metadata").ok()?).ok()?.1.as_stream().ok()?;
    if metadata.content.len() > MAX_XMP_BYTES {
        return None;
    }
    let bytes = metadata.decompressed_content().ok()?;
    if bytes.len() > MAX_XMP_BYTES {
        return None;
    }
    std::str::from_utf8(&bytes).ok().map(str::to_owned)
}

fn xmp_values(document: &XmlDocument<'_>, namespace: &str, local_name: &str, max_bytes: usize) -> Vec<XmpValue> {
    let mut values = Vec::new();
    // XMP permits simple properties both as RDF attributes on Description and
    // as child elements. Adobe tools commonly use the former.
    for attribute in document.descendants().filter(|node| node.is_element()).flat_map(|node| node.attributes()) {
        if attribute.namespace() == Some(namespace) && attribute.name().eq_ignore_ascii_case(local_name) {
            if let Some(text) = bounded_text(attribute.value(), max_bytes) {
                values.push(XmpValue { text, language: None, scheme: None });
            }
            if values.len() >= MAX_VALUES {
                return values;
            }
        }
    }
    for property in document.descendants().filter(|node| node.is_element() && node.tag_name().namespace() == Some(namespace) && node.tag_name().name().eq_ignore_ascii_case(local_name)) {
        let items = property.descendants().filter(|node| node.is_element() && node.tag_name().name() == "li").collect::<Vec<_>>();
        if items.is_empty() {
            push_xmp_value(&mut values, property, max_bytes);
        } else {
            for item in items {
                push_xmp_value(&mut values, item, max_bytes);
                if values.len() >= MAX_VALUES {
                    return values;
                }
            }
        }
        if values.len() >= MAX_VALUES {
            break;
        }
    }
    values
}

fn push_xmp_value(values: &mut Vec<XmpValue>, node: Node<'_, '_>, max_bytes: usize) {
    let combined = node.descendants().filter(|child| child.is_text()).filter_map(|child| child.text()).collect::<Vec<_>>().join(" ");
    let Some(text) = bounded_text(&combined, max_bytes) else { return };
    let language = node.attribute((XML, "lang")).map(str::to_owned);
    let scheme = node.attributes().find(|attribute| attribute.name().eq_ignore_ascii_case("Scheme")).map(|attribute| attribute.value().to_owned());
    values.push(XmpValue { text, language, scheme });
}

fn preferred_language_value(values: &[XmpValue]) -> Option<String> {
    values.iter().find(|value| value.language.as_deref() == Some("x-default")).or_else(|| values.first()).map(|value| value.text.clone())
}

fn texts(values: Vec<XmpValue>) -> Vec<String> {
    values.into_iter().map(|value| value.text).collect()
}

fn xmp_identifiers(document: &XmlDocument<'_>) -> Vec<PublicationIdentifier> {
    let mut values = xmp_values(document, DC, "identifier", MAX_TEXT_BYTES);
    values.extend(xmp_values(document, XMP, "Identifier", MAX_TEXT_BYTES));
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().namespace().is_some_and(|namespace| namespace.contains("prismstandard.org")) && matches!(node.tag_name().name().to_ascii_lowercase().as_str(), "isbn" | "doi" | "issn"))
    {
        let mut extra = Vec::new();
        push_xmp_value(&mut extra, node, MAX_TEXT_BYTES);
        for value in &mut extra {
            value.scheme = Some(node.tag_name().name().to_owned());
        }
        values.extend(extra);
    }
    values.into_iter().filter_map(publication_identifier).take(MAX_VALUES).collect()
}

fn publication_identifier(value: XmpValue) -> Option<PublicationIdentifier> {
    let declared = value.scheme.as_deref().unwrap_or_default().to_ascii_lowercase();
    let trimmed = strip_identifier_label(&value.text);
    let (value, scheme) = if declared.contains("isbn") || canonical_isbn(trimmed).is_some() {
        (canonical_isbn(trimmed)?, PublicationIdentifierScheme::Isbn)
    } else if declared.contains("doi") || canonical_doi(trimmed).is_some() {
        (canonical_doi(trimmed)?, PublicationIdentifierScheme::Doi)
    } else if declared.contains("uuid") || canonical_uuid(trimmed).is_some() {
        (canonical_uuid(trimmed)?, PublicationIdentifierScheme::Uuid)
    } else if declared.contains("issn") {
        (trimmed.to_owned(), PublicationIdentifierScheme::Issn)
    } else {
        (trimmed.to_owned(), PublicationIdentifierScheme::Unspecified)
    };
    let provenance = PublicationIdentifierType::new("xmp", Some("bokheim:provenance".to_owned())).ok().into_iter().collect();
    PublicationIdentifier::new(value, scheme, provenance).ok()
}

fn strip_identifier_label(value: &str) -> &str {
    let value = value.trim();
    for label in ["ISBN-13:", "ISBN-10:", "ISBN:", "isbn-13:", "isbn-10:", "isbn:", "DOI:", "doi:"] {
        if let Some(value) = value.strip_prefix(label) {
            return value.trim();
        }
    }
    value
}

fn xmp_collections(document: &XmlDocument<'_>) -> Vec<PublicationCollection> {
    let mut collections = Vec::new();
    let series = xmp_values(document, CALIBRE, "series", MAX_TEXT_BYTES);
    let positions = xmp_values(document, CALIBRE, "series_index", MAX_TEXT_BYTES);
    for (index, series) in series.into_iter().enumerate() {
        let position = positions.get(index).or_else(|| positions.first()).map(|value| value.text.clone());
        if let Ok(collection) = PublicationCollection::new(series.text, Some("series".to_owned()), position) {
            collections.push(collection);
        }
    }
    collections
}

fn xmp_properties(document: &XmlDocument<'_>) -> Vec<PublicationProperty> {
    let fields =
        [(XMP, "CreateDate"), (XMP, "ModifyDate"), (XMP, "MetadataDate"), (XMP, "CreatorTool"), (PDF, "Producer"), (PDF, "Keywords"), (XMP_RIGHTS, "Marked"), (XMP_RIGHTS, "Owner"), (XMP_RIGHTS, "UsageTerms"), (XMP_RIGHTS, "WebStatement")];
    let mut properties = Vec::new();
    for (namespace, name) in fields {
        for value in xmp_values(document, namespace, name, MAX_DESCRIPTION_BYTES) {
            if let Ok(property) = PublicationProperty::new(None, format!("pdf:xmp:{name}"), value.text, None, value.scheme, value.language) {
                properties.push(property);
            }
        }
    }
    properties
}

fn early_page_isbns(document: &Document) -> Vec<String> {
    let pages = document.get_pages().keys().copied().take(EARLY_TEXT_PAGES).collect::<Vec<_>>();
    let Ok(text) = document.extract_text_with_limit(&pages, MAX_EARLY_TEXT_BYTES) else { return Vec::new() };
    isbns_in_early_text(&text)
}

fn isbns_in_early_text(text: &str) -> Vec<String> {
    let mut found = HashSet::new();
    for token in text.split(|character: char| !(character.is_ascii_alphanumeric() || character == '-')) {
        if let Some(isbn) = canonical_isbn(token) {
            found.insert(isbn);
        }
    }
    // Some publishers separate ISBN groups with spaces. Restrict this more
    // permissive scan to text following an explicit ISBN label.
    let lower = text.to_ascii_lowercase();
    for (offset, _) in lower.match_indices("isbn") {
        let tail = &text[offset + 4..text.len().min(offset + 64)];
        let candidate = tail.trim_start_matches(|character: char| !character.is_ascii_digit() && character != 'X' && character != 'x');
        let candidate = candidate.chars().take_while(|character| character.is_ascii_digit() || matches!(character, 'X' | 'x' | '-' | ' ')).collect::<String>();
        if let Some(isbn) = canonical_isbn(candidate.trim()) {
            found.insert(isbn);
        }
    }
    let mut found = found.into_iter().collect::<Vec<_>>();
    found.sort();
    found
}

fn bounded_text(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes || value.chars().any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t')) {
        return None;
    }
    Some(value.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn dedup_contributors(values: &mut Vec<Contributor>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert((value.name().to_owned(), value.role_code())));
}

fn dedup_publication(publication: &mut PublicationMetadata) {
    let mut identifiers = Vec::new();
    publication.identifiers.retain(|value| {
        let key = (value.scheme().clone(), value.lookup_value());
        if identifiers.contains(&key) {
            false
        } else {
            identifiers.push(key);
            true
        }
    });
    let mut subjects = HashSet::new();
    publication.subjects.retain(|value| subjects.insert(value.name().to_owned()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Object, Stream};
    use tempfile::tempdir;

    #[test]
    fn extracts_rich_xmp_and_checksum_valid_identifier() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("rich.pdf");
        let mut document = Document::with_version("1.7");
        let metadata = br#"<?xpacket begin=''?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:xmp='http://ns.adobe.com/xap/1.0/' xmlns:calibre='http://calibre-ebook.com/xmp-namespace' calibre:series='Examples' calibre:series_index='2'><dc:title><rdf:Alt><rdf:li xml:lang='x-default'>Rich title</rdf:li><rdf:li xml:lang='sv'>Rik titel</rdf:li></rdf:Alt></dc:title><dc:creator><rdf:Seq><rdf:li>Ada Author</rdf:li></rdf:Seq></dc:creator><dc:publisher><rdf:Bag><rdf:li>Example Press</rdf:li></rdf:Bag></dc:publisher><dc:date><rdf:Seq><rdf:li>2024-05-06</rdf:li></rdf:Seq></dc:date><dc:language><rdf:Bag><rdf:li>sv</rdf:li></rdf:Bag></dc:language><dc:identifier>urn:isbn:978-0-8213-3827-8</dc:identifier><dc:subject><rdf:Bag><rdf:li>History</rdf:li></rdf:Bag></dc:subject><dc:rights><rdf:Alt><rdf:li xml:lang='x-default'>Copyright example</rdf:li></rdf:Alt></dc:rights></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let metadata_id = document.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, metadata.to_vec()));
        let pages_id = document.add_object(dictionary! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0 });
        let root_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id, "Metadata" => metadata_id });
        document.trailer.set("Root", root_id);
        document.save(&path).unwrap();

        let inspected = inspect_pdf(&path).unwrap();
        assert_eq!(inspected.metadata.title, "Rich title");
        assert_eq!(inspected.metadata.authors().next().unwrap().name(), "Ada Author");
        assert_eq!(inspected.metadata.publication.publishers[0].name().as_str(), "Example Press");
        assert_eq!(inspected.metadata.publication.identifiers[0].canonical_value().as_deref(), Some("9780821338278"));
        assert_eq!(inspected.metadata.publication.subjects[0].name(), "History");
        assert_eq!(inspected.metadata.publication.collections[0].name(), "Examples");
        assert_eq!(inspected.metadata.publication.collections[0].position(), Some("2"));
    }

    #[test]
    fn early_page_text_accepts_only_checksum_valid_isbns() {
        assert_eq!(isbns_in_early_text("ISBN 978 0 8213 3827 8\nInvalid ISBN 978-0-8213-3827-9"), ["9780821338278"]);
    }
}
