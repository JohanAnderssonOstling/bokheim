//! Synchronous archive access for scanning, import, and thumbnail extraction.
//! Interactive readers use async entry reads and immutable manifest metadata.

use super::*;
use crate::source::Source;
use crate::toc::root_href;
use futures_lite::future::block_on;
use std::collections::HashSet;
#[cfg(test)]
use std::io::{Read, Seek};
use std::sync::Arc;

/// Sequential EPUB scanning, import, and cover extraction with one owned cursor.
pub(crate) struct EpubSession {
    pub(super) archive: ArchiveReader,
    pub(crate) package: Arc<EpubPackage>,
}
impl EpubSession {
    #[cfg(test)]
    pub(crate) fn try_from_reader<R: Read + Seek + Send + 'static>(reader: R) -> io::Result<Self> {
        block_on(Self::open(Source::from_reader(reader)))
    }

    pub(crate) async fn open(source: Source) -> io::Result<Self> {
        let mut archive = ArchiveReader::open(source).await?;
        let package = Arc::new(EpubPackage::open(&mut archive).await?);
        if package.document_uris()?.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "EPUB book contains no readable documents"));
        }
        Ok(Self { archive, package })
    }

    pub(crate) async fn into_provider(self) -> io::Result<EpubProvider> {
        Ok(EpubProvider { archive: self.archive.into_archive().await?, package: self.package })
    }
    pub(crate) async fn read_bytes_bounded_async(&mut self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        self.archive.read(uri, limit).await
    }

    /// Extract bibliographic metadata for scanning and import. Reader opening
    /// uses the manifest and spine without running this inspection stage.
    pub(crate) fn metadata(&mut self) -> io::Result<DeclaredMetadata> {
        let mut metadata = parse_opf_metadata(&self.package.opf_xml);
        metadata.source_path = Some(self.package.opf_path.clone());
        add_metadata_isbns(&mut metadata, self.ncx_property_values());
        Ok(metadata)
    }

    fn ncx_property_values(&mut self) -> Vec<String> {
        if let Some(href) = self.package.get_or_parse_manifest().ncx_href.clone() {
            let path = resolve_resource_href(&self.package.opf_path, &href);
            if let Ok(bytes) = block_on(self.archive.read_prefix(&path, 65_536)) {
                return metadata_block_values(&String::from_utf8_lossy(&bytes), "head");
            }
        }
        Vec::new()
    }

    /// Resolves package-declared and cover-named artwork. Callers apply their
    /// own geometry checks before considering reading-order image fallbacks.
    pub(crate) fn declared_cover_image_path(&mut self) -> io::Result<Option<String>> {
        let href = self.find_declared_cover_href();
        Ok(self.resolve_cover_path(href))
    }

    fn resolve_cover_path(&self, href: Option<String>) -> Option<String> {
        let href = href?;
        let direct = normalize_epub_path(&href);
        Some(if self.package.exists(&direct) { direct } else { resolve_resource_href(&self.package.opf_path, &href) })
    }

    /// Returns image resources in reading order for cover fallback heuristics.
    /// Images referenced by spine documents come first, in DOM order, followed
    /// by any remaining manifest images in package order.
    pub(crate) fn image_paths_in_reading_order(&mut self) -> io::Result<Vec<String>> {
        let manifest = self.package.get_or_parse_manifest().clone();
        let mut paths = Vec::new();
        let mut seen = HashSet::new();

        for idref in &manifest.spine {
            let Some(item) = manifest.items.iter().find(|item| &item.id == idref) else { continue };
            if !is_manifest_item_html(item) {
                if is_manifest_item_image(item) {
                    let path = resolve_resource_href(&self.package.opf_path, &item.href);
                    if seen.insert(path.clone()) {
                        paths.push(path);
                    }
                }
                continue;
            }
            let document_path = resolve_resource_href(&self.package.opf_path, &item.href);
            let document = match self.read_entry_string(&document_path) {
                Ok(document) => document,
                Err(_) => continue,
            };
            for href in parse_image_hrefs_from_html(&document) {
                let path = resolve_toc_href(&document_path, &href);
                if seen.insert(path.clone()) {
                    paths.push(path);
                }
            }
        }

        for item in &manifest.items {
            if !is_manifest_item_image(item) {
                continue;
            }
            let path = resolve_resource_href(&self.package.opf_path, &item.href);
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    #[cfg(test)]
    pub(super) fn read_string(&mut self, uri: &str) -> io::Result<String> {
        self.read_entry_string(uri)
    }

    pub(crate) async fn read_document_prefix(&mut self, uri: &str, max_bytes: u64) -> io::Result<Vec<u8>> {
        self.archive.read_prefix(uri, max_bytes).await
    }

    pub(crate) async fn toc(&mut self) -> io::Result<Option<Vec<TocEntry>>> {
        let manifest = self.package.get_or_parse_manifest().clone();

        if let Some(ref nav_href) = manifest.nav_href {
            let nav_path = resolve_resource_href(&self.package.opf_path, nav_href);
            if let Ok(nav_bytes) = self.archive.read(&nav_path, MAX_MARKUP_RESOURCE_BYTES).await
                && let Some(mut entries) = EpubPackage::parse_nav_toc(&String::from_utf8_lossy(&nav_bytes))
            {
                resolve_toc_links(&nav_path, &mut entries);
                return Ok(Some(entries));
            }
        }

        if let Some(ref ncx_href) = manifest.ncx_href {
            let ncx_path = resolve_resource_href(&self.package.opf_path, ncx_href);
            if let Ok(ncx_bytes) = self.archive.read(&ncx_path, MAX_MARKUP_RESOURCE_BYTES).await
                && let Some(mut entries) = EpubPackage::parse_ncx_toc(&String::from_utf8_lossy(&ncx_bytes))
            {
                resolve_toc_links(&ncx_path, &mut entries);
                return Ok(Some(entries));
            }
        }

        Ok(None)
    }

    fn read_entry_string(&mut self, name: &str) -> io::Result<String> {
        let bytes = block_on(self.archive.read(name, MAX_MARKUP_RESOURCE_BYTES))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn find_declared_cover_href(&mut self) -> Option<String> {
        let manifest = self.package.get_or_parse_manifest().clone();

        // Look for cover-image property
        for item in &manifest.items {
            if let Some(ref props) = item.properties
                && props.contains("cover-image")
            {
                return Some(item.href.clone());
            }
        }

        // Try to find cover by metadata tag in OPF.
        if let Some(cover_id) = parse_opf_cover_id(&self.package.opf_xml) {
            for item in &manifest.items {
                if item.id == cover_id {
                    if is_manifest_item_html(item)
                        && let Some(found) = self.find_cover_from_html(&item.href)
                    {
                        return Some(found);
                    }
                    return Some(item.href.clone());
                }
            }
        }

        // EPUB2 guide reference can point to either image or cover XHTML.
        if let Some(guide_href) = parse_opf_guide_cover_href(&self.package.opf_xml) {
            if let Some(found) = self.find_cover_from_html(&guide_href) {
                return Some(found);
            }
            if let Some(item) = self.find_manifest_item_by_href(&guide_href)
                && is_manifest_item_image(item)
            {
                return Some(item.href.clone());
            }
            return Some(guide_href);
        }

        // Fallback: try parsing likely cover HTML pages.
        for item in &manifest.items {
            let lower_href = item.href.to_ascii_lowercase();
            let lower_id = item.id.to_ascii_lowercase();
            if (lower_href.contains("cover") || lower_id.contains("cover"))
                && is_manifest_item_html(item)
                && let Some(found) = self.find_cover_from_html(&item.href)
            {
                return Some(found);
            }
        }

        // Final fallback: find image item by cover-like name.
        for item in &manifest.items {
            let lower_href = item.href.to_ascii_lowercase();
            let lower_id = item.id.to_ascii_lowercase();
            if (lower_href.contains("cover") || lower_id.contains("cover")) && is_manifest_item_image(item) {
                return Some(item.href.clone());
            }
        }

        None
    }

    fn find_cover_from_html(&mut self, html_href: &str) -> Option<String> {
        let html_href = html_href.split('#').next().unwrap_or(html_href);
        let html_href = html_href.split('?').next().unwrap_or(html_href);
        let html_path = resolve_resource_href(&self.package.opf_path, html_href);
        let html = self.read_entry_string(&html_path).ok()?;
        let image_href = parse_first_image_href_from_html(&html)?;
        Some(resolve_toc_href(&html_path, &image_href))
    }

    fn find_manifest_item_by_href(&self, href: &str) -> Option<&ManifestItem> {
        let manifest = self.package.get_or_parse_manifest();
        let target = resolve_resource_href(&self.package.opf_path, href);
        manifest.items.iter().find(|item| {
            let item_path = resolve_resource_href(&self.package.opf_path, &item.href);
            item_path == target
        })
    }
}

fn infer_identifier_scheme(value: &str) -> IdentifierScheme {
    let value = value.trim().to_ascii_lowercase();
    let compact = value.chars().filter(|character| !character.is_ascii_whitespace() && *character != '-').collect::<String>();
    let looks_like_bare_isbn = (compact.len() == 13 && compact.bytes().all(|byte| byte.is_ascii_digit()))
        || (compact.len() == 10 && compact.bytes().take(9).all(|byte| byte.is_ascii_digit()) && (compact.as_bytes()[9].is_ascii_digit() || compact.as_bytes()[9] == b'x'));
    if value.starts_with("urn:isbn:") || value.starts_with("isbn:") || looks_like_bare_isbn {
        IdentifierScheme::ISBN
    } else if value.starts_with("urn:uuid:") || value.starts_with("uuid:") {
        IdentifierScheme::UUID
    } else if value.starts_with("urn:asin:") || value.starts_with("asin:") {
        IdentifierScheme::ASIN
    } else if value.starts_with("doi:") || value.starts_with("https://doi.org/") || value.starts_with("http://dx.doi.org/") {
        IdentifierScheme::DOI
    } else if value.starts_with("urn:issn:") || value.starts_with("issn:") {
        IdentifierScheme::ISSN
    } else if value.starts_with("oclc:") || value.starts_with("urn:oclc:") {
        IdentifierScheme::OCLC
    } else if value.starts_with("lccn:") || value.starts_with("urn:lccn:") {
        IdentifierScheme::LCCN
    } else if value.starts_with("calibre:") || value.starts_with("urn:calibre:") {
        IdentifierScheme::CALIBRE
    } else {
        IdentifierScheme::Unspecified
    }
}

fn identifier_scheme_from_type(identifier_type: &MetadataIdentifierType) -> Option<IdentifierScheme> {
    let type_scheme = identifier_type.scheme.as_deref().unwrap_or_default().to_ascii_lowercase();
    if type_scheme.contains("onix") && type_scheme.contains("codelist5") {
        return match identifier_type.value.trim() {
            "02" | "15" | "24" => Some(IdentifierScheme::ISBN),
            "06" => Some(IdentifierScheme::DOI),
            "13" => Some(IdentifierScheme::LCCN),
            "23" => Some(IdentifierScheme::OCLC),
            _ => None,
        };
    }
    let parsed = parse_identifier_scheme(&identifier_type.value);
    (!matches!(parsed, IdentifierScheme::Unspecified | IdentifierScheme::Other(_))).then_some(parsed)
}

fn parse_opf_metadata(opf_xml: &str) -> DeclaredMetadata {
    let mut reader = quick_xml::Reader::from_str(opf_xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut metadata = DeclaredMetadata::default();
    let mut current = None;
    let mut current_scheme = IdentifierScheme::Unspecified;
    let mut current_identifier_id: Option<String> = None;
    let mut current_identifier_type_target: Option<String> = None;
    let mut current_identifier_type_scheme: Option<String> = None;
    let mut current_property: Option<MetadataProperty> = None;
    let mut current_value_id: Option<String> = None;
    let mut current_subject_source: Option<String> = None;
    let mut current_subject_authority: Option<String> = None;
    let mut current_date_event: Option<String> = None;
    let mut current_role: Option<CreatorRole> = None;
    let mut current_file_as: Option<String> = None;
    let mut parsed_identifiers = Vec::<(Option<String>, MetadataIdentifier)>::new();
    let mut identifier_type_refinements = Vec::<(String, MetadataIdentifierType)>::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"title") {
                    current = Some(MetaField::Title);
                    current_value_id = read_xml_attr(e.attributes(), b"id");
                } else if name.as_ref().ends_with(b"identifier") {
                    current = Some(MetaField::Identifier);
                    current_scheme = read_xml_attr(e.attributes(), b"scheme").map(|scheme| parse_identifier_scheme(&scheme)).unwrap_or(IdentifierScheme::Unspecified);
                    current_identifier_id = read_xml_attr(e.attributes(), b"id");
                } else if name.as_ref().ends_with(b"meta") && read_xml_attr(e.attributes(), b"property").as_deref().is_some_and(|property| property.rsplit(':').next() == Some("identifier-type")) {
                    current_identifier_type_target = read_xml_attr(e.attributes(), b"refines").and_then(|target| target.strip_prefix('#').map(str::to_owned));
                    current_identifier_type_scheme = read_xml_attr(e.attributes(), b"scheme");
                    current = current_identifier_type_target.as_ref().map(|_| MetaField::IdentifierType);
                } else if name.as_ref().ends_with(b"meta") {
                    if let Some(property) = read_xml_attr(e.attributes(), b"property") {
                        current_property = Some(MetadataProperty {
                            id: read_xml_attr(e.attributes(), b"id"),
                            property,
                            value: String::new(),
                            refines: read_xml_attr(e.attributes(), b"refines"),
                            scheme: read_xml_attr(e.attributes(), b"scheme"),
                            language: read_xml_attr(e.attributes(), b"lang"),
                        });
                        current = Some(MetaField::Property);
                    }
                } else if name.as_ref().ends_with(b"creator") {
                    current = Some(MetaField::Creator);
                    // Extract role attribute (opf:role), default to Author if not present
                    current_role = Some(read_xml_attr(e.attributes(), b"role").map(|r| parse_creator_role(&r)).unwrap_or_else(CreatorRole::author));
                    current_value_id = read_xml_attr(e.attributes(), b"id");
                    // Extract file-as attribute (opf:file-as)
                    current_file_as = read_xml_attr(e.attributes(), b"file-as");
                } else if name.as_ref().ends_with(b"contributor") {
                    current_role = Some(read_xml_attr(e.attributes(), b"role").map(|r| parse_creator_role(&r)).unwrap_or_else(CreatorRole::contributor));
                    current_value_id = read_xml_attr(e.attributes(), b"id");
                    current = Some(MetaField::Contributor);
                    current_file_as = read_xml_attr(e.attributes(), b"file-as");
                } else if name.as_ref().ends_with(b"description") {
                    current = Some(MetaField::Description);
                } else if name.as_ref().ends_with(b"publisher") {
                    current = Some(MetaField::Publisher);
                } else if name.as_ref().ends_with(b"language") {
                    current = Some(MetaField::Language);
                } else if name.as_ref().ends_with(b"date") {
                    current = Some(MetaField::Date);
                    current_value_id = read_xml_attr(e.attributes(), b"id");
                    current_date_event = read_xml_attr(e.attributes(), b"event");
                } else if name.as_ref().ends_with(b"subject") {
                    current = Some(MetaField::Subject);
                    current_value_id = read_xml_attr(e.attributes(), b"id");
                    current_subject_source = Some(String::from_utf8_lossy(name.as_ref()).into_owned());
                    current_subject_authority = read_xml_attr(e.attributes(), b"scheme");
                } else if name.as_ref().ends_with(b"rights") {
                    current = Some(MetaField::Rights);
                } else if name.as_ref().ends_with(b"source") {
                    current = Some(MetaField::Source);
                } else if name.as_ref().ends_with(b"type") {
                    current = Some(MetaField::Type);
                } else if name.as_ref().ends_with(b"format") {
                    current = Some(MetaField::Format);
                } else if name.as_ref().ends_with(b"relation") {
                    current = Some(MetaField::Relation);
                } else if name.as_ref().ends_with(b"coverage") {
                    current = Some(MetaField::Coverage);
                }
            }
            Ok(quick_xml::events::Event::Empty(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"meta") && read_xml_attr(e.attributes(), b"property").as_deref().is_some_and(|property| property.rsplit(':').next() == Some("identifier-type")) {
                    if let (Some(target), Some(value)) = (read_xml_attr(e.attributes(), b"refines").and_then(|target| target.strip_prefix('#').map(str::to_owned)), read_xml_attr(e.attributes(), b"content")) {
                        let value = value.trim();
                        if !value.is_empty() {
                            identifier_type_refinements.push((target, MetadataIdentifierType { value: value.to_owned(), scheme: read_xml_attr(e.attributes(), b"scheme") }));
                        }
                    }
                } else if name.as_ref().ends_with(b"meta") {
                    let property = read_xml_attr(e.attributes(), b"property").or_else(|| read_xml_attr(e.attributes(), b"name"));
                    if let (Some(property), Some(value)) = (property, read_xml_attr(e.attributes(), b"content")) {
                        let value = value.trim();
                        if !value.is_empty() {
                            metadata.properties.push(MetadataProperty {
                                id: read_xml_attr(e.attributes(), b"id"),
                                property,
                                value: value.to_owned(),
                                refines: read_xml_attr(e.attributes(), b"refines"),
                                scheme: read_xml_attr(e.attributes(), b"scheme"),
                                language: read_xml_attr(e.attributes(), b"lang"),
                            });
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"title")
                    || name.as_ref().ends_with(b"identifier")
                    || name.as_ref().ends_with(b"creator")
                    || name.as_ref().ends_with(b"contributor")
                    || name.as_ref().ends_with(b"description")
                    || name.as_ref().ends_with(b"publisher")
                    || name.as_ref().ends_with(b"language")
                    || name.as_ref().ends_with(b"date")
                    || name.as_ref().ends_with(b"subject")
                    || name.as_ref().ends_with(b"rights")
                    || name.as_ref().ends_with(b"source")
                    || name.as_ref().ends_with(b"type")
                    || name.as_ref().ends_with(b"format")
                    || name.as_ref().ends_with(b"relation")
                    || name.as_ref().ends_with(b"coverage")
                    || name.as_ref().ends_with(b"meta")
                {
                    current = None;
                    current_scheme = IdentifierScheme::Unspecified;
                    current_identifier_id = None;
                    current_identifier_type_target = None;
                    current_identifier_type_scheme = None;
                    current_role = None;
                    current_file_as = None;
                    current_property = None;
                    current_value_id = None;
                    current_subject_source = None;
                    current_subject_authority = None;
                    current_date_event = None;
                }
            }
            Ok(quick_xml::events::Event::Text(t)) => {
                if let Some(field) = current {
                    let text = t.unescape().unwrap_or_default().into_owned();
                    if text.is_empty() {
                        continue;
                    }
                    match field {
                        MetaField::Title => {
                            if metadata.title.is_empty() {
                                metadata.title = text;
                            } else {
                                metadata.alternate_titles.push(MetadataTitle { id: current_value_id.clone(), value: text, title_type: None, sort_as: None });
                            }
                        }
                        MetaField::Identifier => {
                            let value = text.trim();
                            if !value.is_empty() {
                                let scheme = if matches!(current_scheme, IdentifierScheme::Unspecified) { infer_identifier_scheme(value) } else { current_scheme.clone() };
                                parsed_identifiers.push((current_identifier_id.clone(), MetadataIdentifier { value: value.to_owned(), scheme, identifier_types: Vec::new() }));
                            }
                        }
                        MetaField::IdentifierType => {
                            if let Some(target) = current_identifier_type_target.clone() {
                                let value = text.trim();
                                if !value.is_empty() {
                                    identifier_type_refinements.push((target, MetadataIdentifierType { value: value.to_owned(), scheme: current_identifier_type_scheme.clone() }));
                                }
                            }
                        }
                        MetaField::Creator => {
                            // unwrap is safe here because we always set current_role for creators
                            metadata.creators.push(MetadataCredit { id: current_value_id.clone(), name: text, file_as: current_file_as.clone(), role: current_role.clone().unwrap() });
                        }
                        MetaField::Contributor => {
                            // unwrap is safe here because we only set current=Contributor if role exists
                            metadata.contributors.push(MetadataCredit { id: current_value_id.clone(), name: text, file_as: current_file_as.clone(), role: current_role.clone().unwrap() });
                        }
                        MetaField::Description if metadata.description.is_empty() => {
                            metadata.description = text;
                        }
                        MetaField::Publisher => {
                            metadata.publishers.push(text);
                        }
                        MetaField::Language => {
                            metadata.languages.push(text);
                        }
                        MetaField::Date => {
                            metadata.dates.push(MetadataDate { source: "epub:package:date".to_owned(), id: current_value_id.clone(), value: text, event: current_date_event.clone() });
                        }
                        MetaField::Subject => {
                            let code = current_subject_authority.as_deref().filter(|authority| authority.eq_ignore_ascii_case("bisac")).filter(|_| looks_like_bisac_code(&text)).map(|_| text.trim().to_ascii_uppercase());
                            metadata.subjects.push(MetadataSubject {
                                id: current_value_id.clone(),
                                value: text,
                                source: current_subject_source.clone().unwrap_or_else(|| "dc:subject".to_owned()),
                                authority: current_subject_authority.clone(),
                                code,
                            });
                        }
                        MetaField::Rights => metadata.rights.push(text),
                        MetaField::Source => metadata.sources.push(text),
                        MetaField::Type => metadata.types.push(text),
                        MetaField::Format => metadata.formats.push(text),
                        MetaField::Relation => metadata.relations.push(text),
                        MetaField::Coverage => metadata.coverage.push(text),
                        MetaField::Property => {
                            if let Some(property) = current_property.clone() {
                                metadata.properties.push(MetadataProperty { value: text, ..property });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    for (source_id, mut identifier) in parsed_identifiers {
        if let Some(source_id) = source_id {
            identifier.identifier_types.extend(identifier_type_refinements.iter().filter(|(target, _)| target == &source_id).map(|(_, identifier_type)| identifier_type.clone()));
        }
        if matches!(identifier.scheme, IdentifierScheme::Unspecified)
            && let Some(scheme) = identifier.identifier_types.iter().find_map(identifier_scheme_from_type)
        {
            identifier.scheme = scheme;
        }
        // A checksum-valid whole value outranks the producer's scheme,
        // including EPUB 3 identifier-type refinements.
        if let Some(isbn) = book_model::from_metadata_value(&identifier.value) {
            identifier.value = isbn;
            identifier.scheme = IdentifierScheme::ISBN;
        } else if matches!(identifier.scheme, IdentifierScheme::ISBN) {
            identifier.scheme = IdentifierScheme::Unspecified;
        }
        metadata.identifiers.push(identifier);
    }

    for property in &metadata.properties {
        if property.property.rsplit(':').next() != Some("role") {
            continue;
        }
        let Some(target) = property.refines.as_deref().and_then(|target| target.strip_prefix('#')) else {
            continue;
        };
        if let Some(credit) = metadata.creators.iter_mut().chain(metadata.contributors.iter_mut()).find(|credit| credit.id.as_deref() == Some(target)) {
            credit.role = parse_creator_role(&property.value);
        }
    }

    let refinement = |id: &Option<String>, property_name: &str| {
        let target = id.as_deref().map(|id| format!("#{id}"));
        metadata.properties.iter().find(|property| property.refines == target && property.property.rsplit(':').next() == Some(property_name)).map(|property| property.value.clone())
    };
    for title in &mut metadata.alternate_titles {
        title.title_type = refinement(&title.id, "title-type");
        title.sort_as = refinement(&title.id, "file-as");
    }
    for subject in &mut metadata.subjects {
        subject.authority = refinement(&subject.id, "authority").or_else(|| subject.authority.take());
        subject.code = refinement(&subject.id, "term").or_else(|| subject.code.take());
        if subject.code.is_none() && subject.authority.as_deref().is_some_and(|authority| authority.eq_ignore_ascii_case("bisac")) && looks_like_bisac_code(&subject.value) {
            subject.code = Some(subject.value.trim().to_ascii_uppercase());
        }
    }

    let property_subjects = metadata.properties.iter().flat_map(subjects_from_property).collect::<Vec<_>>();
    metadata.subjects.extend(property_subjects);

    add_metadata_isbns(&mut metadata, metadata_block_values(opf_xml, "metadata"));
    metadata
}

fn add_metadata_isbns(metadata: &mut DeclaredMetadata, values: impl IntoIterator<Item = String>) {
    for value in values {
        let Some(isbn) = book_model::from_metadata_value(&value) else { continue };
        if metadata.identifiers.iter().any(|id| matches!(id.scheme, IdentifierScheme::ISBN) && book_model::from_metadata_value(&id.value).as_ref() == Some(&isbn)) {
            continue;
        }
        metadata.identifiers.push(MetadataIdentifier { value: isbn, scheme: IdentifierScheme::ISBN, identifier_types: Vec::new() });
    }
}

fn looks_like_bisac_code(value: &str) -> bool {
    let value = value.trim().as_bytes();
    value.len() == 9 && value[..3].iter().all(u8::is_ascii_alphabetic) && value[3..].iter().all(u8::is_ascii_digit)
}

fn subjects_from_property(property: &MetadataProperty) -> Vec<MetadataSubject> {
    let local_name = property.property.rsplit(':').next().unwrap_or(&property.property);
    if property.property.eq_ignore_ascii_case("calibre:user_metadata") {
        return calibre_user_metadata_subjects(&property.property, &property.value);
    }
    if !matches_subject_property(local_name) {
        return Vec::new();
    }
    if local_name.eq_ignore_ascii_case("subject") {
        return subject_values(&property.property, &serde_json::Value::String(property.value.clone()));
    }
    match serde_json::from_str::<serde_json::Value>(&property.value) {
        Ok(serde_json::Value::Object(object)) => object.get("#value#").map(|value| subject_values(&property.property, value)).unwrap_or_default(),
        Ok(value @ serde_json::Value::Array(_)) | Ok(value @ serde_json::Value::String(_)) => subject_values(&property.property, &value),
        _ => subject_values(&property.property, &serde_json::Value::String(property.value.clone())),
    }
}

fn calibre_user_metadata_subjects(source: &str, raw: &str) -> Vec<MetadataSubject> {
    let Ok(serde_json::Value::Object(properties)) = serde_json::from_str(raw) else { return Vec::new() };
    let mut subjects = Vec::new();
    for (key, value) in properties {
        let serde_json::Value::Object(property) = value else { continue };
        let identity = format!("{} {} {}", key, property.get("name").and_then(serde_json::Value::as_str).unwrap_or_default(), property.get("label").and_then(serde_json::Value::as_str).unwrap_or_default());
        if !identity.split(|character: char| !character.is_alphanumeric()).any(matches_subject_property) {
            continue;
        }
        if let Some(value) = property.get("#value#") {
            subjects.extend(subject_values(&format!("{source}/{key}"), value));
        }
    }
    subjects
}

fn matches_subject_property(value: &str) -> bool {
    value.eq_ignore_ascii_case("subject") || value.eq_ignore_ascii_case("genre") || value.eq_ignore_ascii_case("tag") || value.eq_ignore_ascii_case("tags")
}

fn subject_values(source: &str, value: &serde_json::Value) -> Vec<MetadataSubject> {
    match value {
        serde_json::Value::String(value) => {
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
            if value.is_empty() { Vec::new() } else { vec![MetadataSubject { id: None, value, source: source.to_owned(), authority: None, code: None }] }
        }
        serde_json::Value::Array(values) => values.iter().flat_map(|value| subject_values(source, value)).collect(),
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy)]
enum MetaField {
    Title,
    Identifier,
    IdentifierType,
    Creator,
    Contributor,
    Description,
    Publisher,
    Language,
    Date,
    Subject,
    Rights,
    Source,
    Type,
    Format,
    Relation,
    Coverage,
    Property,
}

fn parse_opf_cover_id(opf_xml: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(opf_xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut in_metadata = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"metadata") {
                    in_metadata = true;
                } else if in_metadata && name.as_ref().ends_with(b"meta") {
                    let mut meta_name: Option<String> = None;
                    let mut meta_content: Option<String> = None;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().ends_with(b"name") {
                            meta_name = Some(String::from_utf8_lossy(&attr.value).into_owned());
                        } else if attr.key.as_ref().ends_with(b"content") {
                            meta_content = Some(String::from_utf8_lossy(&attr.value).into_owned());
                        }
                    }
                    if meta_name.as_deref() == Some("cover") {
                        return meta_content;
                    }
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"metadata") {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    None
}

fn parse_opf_guide_cover_href(opf_xml: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(opf_xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut in_guide = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"guide") {
                    in_guide = true;
                } else if in_guide && name.as_ref().ends_with(b"reference") {
                    let mut ref_type: Option<String> = None;
                    let mut href: Option<String> = None;
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().ends_with(b"type") {
                            ref_type = Some(String::from_utf8_lossy(&attr.value).into_owned());
                        } else if attr.key.as_ref().ends_with(b"href") {
                            href = Some(String::from_utf8_lossy(&attr.value).into_owned());
                        }
                    }
                    if ref_type.as_deref().map(|s| s.eq_ignore_ascii_case("cover")).unwrap_or(false) {
                        return href;
                    }
                }
            }
            Ok(quick_xml::events::Event::End(e)) if e.name().as_ref().ends_with(b"guide") => {
                break;
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    None
}

fn parse_first_image_href_from_html(html: &str) -> Option<String> {
    parse_image_hrefs_from_html(html).into_iter().next()
}

fn parse_image_hrefs_from_html(html: &str) -> Vec<String> {
    let mut reader = quick_xml::Reader::from_str(html);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut images = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let tag = e.name();
                if tag.as_ref().ends_with(b"img") {
                    if let Some(src) = read_xml_attr(e.attributes(), b"src") {
                        images.push(src);
                    }
                } else if tag.as_ref().ends_with(b"image")
                    && let Some(href) = read_xml_attr(e.attributes(), b"href").or_else(|| read_xml_attr(e.attributes(), b"xlink:href"))
                {
                    images.push(href);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    images
}

fn is_manifest_item_html(item: &ManifestItem) -> bool {
    if let Some(media_type) = item.media_type.as_deref() {
        let media = media_type.to_ascii_lowercase();
        if media == "application/xhtml+xml" || media == "text/html" {
            return true;
        }
    }
    let href = item.href.to_ascii_lowercase();
    href.ends_with(".xhtml") || href.ends_with(".html") || href.ends_with(".htm")
}

fn is_manifest_item_image(item: &ManifestItem) -> bool {
    if let Some(media_type) = item.media_type.as_deref() {
        let media = media_type.to_ascii_lowercase();
        if media.starts_with("image/") {
            return true;
        }
    }
    let href = item.href.to_ascii_lowercase();
    href.ends_with(".jpg") || href.ends_with(".jpeg") || href.ends_with(".png") || href.ends_with(".webp") || href.ends_with(".gif")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opf_metadata_applies_epub3_role_refinements_and_keeps_untyped_contributors() {
        let metadata = parse_opf_metadata(
            r##"<package xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
                <metadata>
                    <dc:title>Credits</dc:title>
                    <dc:creator id="translator">A Translator</dc:creator>
                    <meta refines="#translator" property="role" scheme="marc:relators">trl</meta>
                    <dc:creator opf:role="aut">An Author</dc:creator>
                    <dc:contributor id="editor">An Editor</dc:contributor>
                    <meta refines="#editor" property="role" scheme="marc:relators" content="edt"/>
                    <dc:contributor>Untyped Credit</dc:contributor>
                </metadata>
            </package>"##,
        );

        assert_eq!(metadata.creators[0].id.as_deref(), Some("translator"));
        assert_eq!(metadata.creators[0].role.as_str(), "trl");
        assert_eq!(metadata.creators[1].role.as_str(), "aut");
        assert_eq!(metadata.contributors[0].role.as_str(), "edt");
        assert_eq!(metadata.contributors[1].role.as_str(), "ctb");
    }
}

/// Keep element text intact across XML text events so a valid substring in a
/// larger value cannot masquerade as a complete ISBN.
fn metadata_block_values(xml: &str, block: &str) -> Vec<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.expand_empty_elements(true);
    let mut inside = false;
    let mut stack: Vec<String> = Vec::new();
    let mut values = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if !inside {
                    if e.local_name().as_ref() == block.as_bytes() {
                        inside = true;
                    }
                    continue;
                }
                stack.push(String::new());
                for attribute in e.attributes().flatten() {
                    if let Ok(value) = attribute.unescape_value() {
                        values.push(value.into_owned());
                    }
                }
            }
            Ok(Event::Text(e)) if inside => {
                if let Ok(text) = e.unescape() {
                    for value in &mut stack {
                        value.push_str(&text);
                    }
                }
            }
            Ok(Event::CData(e)) if inside => {
                for value in &mut stack {
                    value.push_str(&String::from_utf8_lossy(&e));
                }
            }
            Ok(Event::End(e)) if inside => {
                if stack.is_empty() && e.local_name().as_ref() == block.as_bytes() {
                    break;
                }
                if let Some(value) = stack.pop() {
                    values.push(value);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        if values.len() >= 4096 || stack.len() > 64 {
            break;
        }
    }
    values
}

#[cfg(test)]
mod bibliographic_property_tests {
    use super::metadata_block_values;
    #[test]
    fn scans_values_not_digit_fragments_and_stops_at_head_end() {
        let values =
            metadata_block_values(r#"<ncx><head><meta name="dtb:uid" content="978-1-934356-55-5"/><meta name="other">prefix<![CDATA[9781934356555]]>suffix</meta></head><navMap><meta content="9780191507052"/></navMap></ncx>"#, "head");
        assert!(values.iter().any(|v| v == "978-1-934356-55-5"));
        assert!(values.iter().any(|v| v == "prefix9781934356555suffix"));
        assert!(!values.iter().any(|v| v == "9781934356555" || v == "9780191507052"));
    }
}

#[cfg(test)]
mod mislabeled_isbn_tests {
    use super::*;
    #[test]
    fn whole_values_override_schemes_and_include_arbitrary_properties() {
        let metadata = parse_opf_metadata(
            r#"<package><metadata>
          <identifier scheme="ASIN">978-1-934356-55-5</identifier>
          <identifier scheme="ASIN">B005E835K0</identifier>
          <meta name="unknown" content="9780191507052"/>
          <source>9780191085048</source>
          <meta name="uuid" content="7872d3e9-8394-4530-8a39-773505e19c9b"/>
          <meta name="cover" content="9780199588503.jpg"/>
          <identifier scheme="ISBN">0000000000</identifier>
        </metadata></package>"#,
        );
        let isbns: Vec<_> = metadata.identifiers.iter().filter(|id| matches!(id.scheme, IdentifierScheme::ISBN)).map(|id| id.value.as_str()).collect();
        assert_eq!(isbns, ["9781934356555", "9780191507052", "9780191085048"]);
        assert!(metadata.identifiers.iter().any(|id| id.value == "B005E835K0" && matches!(id.scheme, IdentifierScheme::ASIN)));
    }
}

impl EpubPackage {
    fn parse_nav_toc(nav_xml: &str) -> Option<Vec<TocEntry>> {
        let html = scraper::Html::parse_document(nav_xml);
        let nav = scraper::Selector::parse("nav").unwrap();
        let toc = html.select(&nav).find(|element| element.value().attr("epub:type").or_else(|| element.value().attr("type")).is_some_and(|kind| kind.split_whitespace().any(|kind| kind == "toc")))?;
        let entries = crate::toc::html_entries(toc, |element| element.value().attr("href").map(str::to_owned));
        (!entries.is_empty()).then_some(entries)
    }

    fn parse_ncx_toc(ncx_xml: &str) -> Option<Vec<TocEntry>> {
        let mut reader = quick_xml::Reader::from_str(ncx_xml);
        reader.trim_text(true);
        let mut buf = Vec::new();
        let mut stack: Vec<TocEntry> = Vec::new();
        let mut root: Vec<TocEntry> = Vec::new();
        let mut in_text = false;
        let mut text_buf = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(e)) => {
                    if e.name().as_ref().ends_with(b"navPoint") {
                        stack.push(TocEntry::default());
                    } else if e.name().as_ref().ends_with(b"text") {
                        in_text = true;
                        text_buf.clear();
                    } else if e.name().as_ref().ends_with(b"content") {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref().ends_with(b"src")
                                && let Some(entry) = stack.last_mut()
                            {
                                entry.link = String::from_utf8_lossy(&attr.value).into_owned();
                            }
                        }
                    }
                }
                Ok(quick_xml::events::Event::Empty(e)) if e.name().as_ref().ends_with(b"content") => {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref().ends_with(b"src")
                            && let Some(entry) = stack.last_mut()
                        {
                            entry.link = String::from_utf8_lossy(&attr.value).into_owned();
                        }
                    }
                }
                Ok(quick_xml::events::Event::Text(e)) if in_text => {
                    text_buf.push_str(&e.unescape().unwrap_or_default());
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    if e.name().as_ref().ends_with(b"text") && in_text {
                        in_text = false;
                        if let Some(entry) = stack.last_mut() {
                            entry.title = text_buf.trim().to_string();
                        }
                    } else if e.name().as_ref().ends_with(b"navPoint")
                        && let Some(entry) = stack.pop()
                    {
                        if let Some(parent) = stack.last_mut() {
                            parent.children.push(entry);
                        } else {
                            root.push(entry);
                        }
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        if root.is_empty() { None } else { Some(root) }
    }
}

fn resolve_toc_links(base_path: &str, entries: &mut [TocEntry]) {
    for entry in entries {
        entry.link = root_href(resolve_toc_href(base_path, entry.link.as_str()));
        resolve_toc_links(base_path, &mut entry.children);
    }
}

fn resolve_toc_href(base_path: &str, href: &str) -> String {
    if href.is_empty() {
        return href.to_string();
    }
    if href.starts_with("http:") || href.starts_with("https:") {
        return href.to_string();
    }
    let (path_part, fragment) = href.split_once('#').unwrap_or((href, ""));
    let path_part = path_part.trim();
    let resolved_path = if path_part.is_empty() {
        base_path.to_string()
    } else if !is_container_resource(path_part) {
        return href.to_string();
    } else {
        resolve_resource_href(base_path, path_part)
    };
    if fragment.is_empty() { resolved_path } else { format!("{}#{}", resolved_path, fragment) }
}
