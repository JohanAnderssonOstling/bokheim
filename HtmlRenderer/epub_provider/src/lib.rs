use std::fs::File;
use std::io::{self, Cursor, Read, Seek};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub use html_source::{ResourceProvider, TocEntry};

mod definitions;
pub use definitions::{CreatorRole, IdentifierScheme, parse_creator_role, parse_identifier_scheme};

#[derive(Clone, Debug)]
pub struct EpubIdentifier {
    pub value: String,
    pub scheme: IdentifierScheme,
}

#[derive(Clone, Debug)]
pub struct EpubCreator {
    pub name: String,            // Display name (from text content)
    pub file_as: Option<String>, // Sort name (from opf:file-as)
    pub role: CreatorRole,
}

#[derive(Clone, Debug, Default)]
pub struct EpubMetadata {
    pub title: String,
    pub identifiers: Vec<EpubIdentifier>,
    pub creators: Vec<EpubCreator>,
    pub contributors: Vec<EpubCreator>,
    pub description: String,
    pub publisher: String,
    pub language: String,
    pub date: String,
}

#[derive(Clone, Debug)]
struct ManifestItem {
    id: String,
    href: String,
    media_type: Option<String>,
    properties: Option<String>,
}

#[derive(Clone, Debug)]
struct ParsedManifest {
    items: Vec<ManifestItem>,
    spine: Vec<String>,
    nav_href: Option<String>,
    ncx_href: Option<String>,
}

pub struct EpubProvider {
    archive: Mutex<zip::ZipArchive<Cursor<Vec<u8>>>>,
    opf_path: String,
    opf_dir: String,
    opf_xml: String,
    manifest_cache: OnceLock<ParsedManifest>,
}

impl EpubProvider {
    pub fn try_new(epub_path: PathBuf) -> io::Result<Self> {
        let file = File::open(&epub_path)?;
        Self::try_from_reader(file)
    }

    pub fn try_from_reader<R>(mut reader: R) -> io::Result<Self>
    where
        R: Read + Seek + Send + 'static,
    {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        // Read and parse container.xml to get OPF path
        let container_xml = {
            let mut entry = archive.by_name("META-INF/container.xml").map_err(|err| io::Error::new(io::ErrorKind::NotFound, err))?;
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            use std::io::Read;
            entry.read_to_end(&mut bytes)?;
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let opf_path = Self::parse_container_opf_path(&container_xml).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Missing OPF path in container.xml"))?;

        // Read OPF XML
        let opf_xml = {
            let mut entry = archive.by_name(&opf_path).map_err(|err| io::Error::new(io::ErrorKind::NotFound, err))?;
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            use std::io::Read;
            entry.read_to_end(&mut bytes)?;
            String::from_utf8_lossy(&bytes).into_owned()
        };

        // Pre-compute OPF directory for path resolution
        let opf_dir = opf_path.rsplitn(2, '/').nth(1).unwrap_or("").to_string();

        Ok(Self { archive: Mutex::new(archive), opf_path, opf_dir, opf_xml, manifest_cache: OnceLock::new() })
    }

    pub fn new(epub_path: PathBuf) -> Self {
        Self::try_new(epub_path).expect("Failed to open EPUB file")
    }

    pub fn metadata(&self) -> io::Result<EpubMetadata> {
        Ok(parse_opf_metadata(&self.opf_xml))
    }

    pub fn cover_image_path(&self) -> io::Result<Option<String>> {
        let Some(href) = self.find_cover_href() else {
            return Ok(None);
        };
        let direct = normalize_epub_path(&href);
        if self.exists(&direct) {
            return Ok(Some(direct));
        }

        let resolved = resolve_href(&self.opf_dir, &href);
        Ok(Some(normalize_epub_path(&resolved)))
    }

    fn find_cover_href(&self) -> Option<String> {
        let manifest = self.get_or_parse_manifest();

        // Look for cover-image property
        for item in &manifest.items {
            if let Some(ref props) = item.properties {
                if props.contains("cover-image") {
                    return Some(item.href.clone());
                }
            }
        }

        // Try to find cover by metadata tag in OPF.
        if let Some(cover_id) = parse_opf_cover_id(&self.opf_xml) {
            for item in &manifest.items {
                if item.id == cover_id {
                    if is_manifest_item_html(item) {
                        if let Some(found) = self.find_cover_from_html(&item.href) {
                            return Some(found);
                        }
                    }
                    return Some(item.href.clone());
                }
            }
        }

        // EPUB2 guide reference can point to either image or cover XHTML.
        if let Some(guide_href) = parse_opf_guide_cover_href(&self.opf_xml) {
            if let Some(found) = self.find_cover_from_html(&guide_href) {
                return Some(found);
            }
            if let Some(item) = self.find_manifest_item_by_href(&guide_href) {
                if is_manifest_item_image(item) {
                    return Some(item.href.clone());
                }
            }
            return Some(guide_href);
        }

        // Check first spine item, often coverpage/cover html in EPUB2 files.
        if let Some(first_spine_idref) = parse_opf_first_spine_idref(&self.opf_xml) {
            if let Some(item) = manifest.items.iter().find(|item| item.id == first_spine_idref) {
                if is_manifest_item_html(item) {
                    if let Some(found) = self.find_cover_from_html(&item.href) {
                        return Some(found);
                    }
                } else if is_manifest_item_image(item) {
                    return Some(item.href.clone());
                }
            }
        }

        // Fallback: try parsing likely cover HTML pages.
        for item in &manifest.items {
            let lower_href = item.href.to_ascii_lowercase();
            let lower_id = item.id.to_ascii_lowercase();
            if (lower_href.contains("cover") || lower_id.contains("cover")) && is_manifest_item_html(item) {
                if let Some(found) = self.find_cover_from_html(&item.href) {
                    return Some(found);
                }
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

        // Last resort: pick the best likely front-cover image from the manifest.
        if let Some(best) = pick_best_cover_image_item(&manifest.items) {
            return Some(best.href.clone());
        }

        None
    }

    fn find_cover_from_html(&self, html_href: &str) -> Option<String> {
        let html_href = html_href.split('#').next().unwrap_or(html_href);
        let html_href = html_href.split('?').next().unwrap_or(html_href);
        let resolved = resolve_href(&self.opf_dir, html_href);
        let html_path = normalize_epub_path(&resolved);
        let html = self.read_entry_string(&html_path).ok()?;
        let image_href = parse_first_image_href_from_html(&html)?;
        Some(resolve_toc_href(&html_path, &image_href))
    }

    fn find_manifest_item_by_href(&self, href: &str) -> Option<&ManifestItem> {
        let manifest = self.get_or_parse_manifest();
        let target = normalize_epub_path(&resolve_href(&self.opf_dir, href));
        manifest.items.iter().find(|item| {
            let item_path = normalize_epub_path(&resolve_href(&self.opf_dir, &item.href));
            item_path == target
        })
    }

    pub fn toc(&self) -> io::Result<Option<Vec<TocEntry>>> {
        let manifest = self.get_or_parse_manifest();

        if let Some(ref nav_href) = manifest.nav_href {
            let resolved = resolve_href(&self.opf_dir, nav_href);
            let nav_path = normalize_epub_path(&resolved);
            if let Ok(nav_xml) = self.read_entry_string(&nav_path) {
                if let Some(mut entries) = Self::parse_nav_toc(&nav_xml) {
                    resolve_toc_links(&nav_path, &mut entries);
                    return Ok(Some(entries));
                }
            }
        }

        if let Some(ref ncx_href) = manifest.ncx_href {
            let resolved = resolve_href(&self.opf_dir, ncx_href);
            let ncx_path = normalize_epub_path(&resolved);
            if let Ok(ncx_xml) = self.read_entry_string(&ncx_path) {
                if let Some(mut entries) = Self::parse_ncx_toc(&ncx_xml) {
                    resolve_toc_links(&ncx_path, &mut entries);
                    return Ok(Some(entries));
                }
            }
        }

        Ok(None)
    }

    fn is_html_entry(name: &str) -> bool {
        let len = name.len();
        if len >= 5 && name[len - 5..].eq_ignore_ascii_case(".html") {
            return true;
        }
        if len >= 6 && name[len - 6..].eq_ignore_ascii_case(".xhtml") {
            return true;
        }
        if len >= 4 && name[len - 4..].eq_ignore_ascii_case(".htm") {
            return true;
        }
        if len >= 4 && name[len - 4..].eq_ignore_ascii_case(".svg") {
            return true;
        }
        false
    }

    fn read_entry_string(&self, name: &str) -> io::Result<String> {
        let mut archive = self.archive.lock().unwrap();
        let mut entry = archive.by_name(name).map_err(|err| io::Error::new(io::ErrorKind::NotFound, err))?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        use std::io::Read;
        entry.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn parse_container_opf_path(container_xml: &str) -> Option<String> {
        let mut reader = quick_xml::Reader::from_str(container_xml);
        reader.trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Empty(e)) | Ok(quick_xml::events::Event::Start(e)) => {
                    if e.name().as_ref().ends_with(b"rootfile") {
                        return read_xml_attr(e.attributes(), b"full-path");
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

    fn get_or_parse_manifest(&self) -> &ParsedManifest {
        self.manifest_cache.get_or_init(|| Self::parse_opf_manifest(&self.opf_xml))
    }

    fn parse_opf_manifest(opf_xml: &str) -> ParsedManifest {
        let mut reader = quick_xml::Reader::from_str(opf_xml);
        reader.trim_text(true);
        let mut buf = Vec::new();
        let mut items = Vec::new();
        let mut spine = Vec::new();
        let mut nav_href: Option<String> = None;
        let mut toc_id: Option<String> = None;
        let mut in_manifest = false;
        let mut in_spine = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                    if e.name().as_ref().ends_with(b"manifest") {
                        in_manifest = true;
                    } else if e.name().as_ref().ends_with(b"spine") {
                        in_spine = true;
                        toc_id = read_xml_attr(e.attributes(), b"toc");
                    } else if in_manifest && e.name().as_ref().ends_with(b"item") {
                        let mut id = None;
                        let mut href = None;
                        let mut media_type = None;
                        let mut properties = None;

                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            if key.ends_with(b"id") {
                                id = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            } else if key.ends_with(b"href") {
                                href = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            } else if key.ends_with(b"media-type") {
                                media_type = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            } else if key.ends_with(b"properties") {
                                properties = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            }
                        }

                        if let (Some(id), Some(href)) = (id, href) {
                            if let Some(ref props) = properties {
                                if props.split_whitespace().any(|v| v == "nav") {
                                    nav_href = Some(href.clone());
                                }
                            }
                            items.push(ManifestItem { id, href, media_type, properties });
                        }
                    } else if in_spine && e.name().as_ref().ends_with(b"itemref") {
                        if let Some(idref) = read_xml_attr(e.attributes(), b"idref") {
                            spine.push(idref);
                        }
                    }
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    if e.name().as_ref().ends_with(b"manifest") {
                        in_manifest = false;
                    } else if e.name().as_ref().ends_with(b"spine") {
                        in_spine = false;
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        let ncx_href = toc_id.and_then(|id| items.iter().find(|item| item.id == id).map(|item| item.href.clone()));

        ParsedManifest { items, spine, nav_href, ncx_href }
    }

    fn parse_nav_toc(nav_xml: &str) -> Option<Vec<TocEntry>> {
        let mut reader = quick_xml::Reader::from_str(nav_xml);
        reader.trim_text(true);
        let mut buf = Vec::new();
        let mut in_toc_nav = false;
        let mut toc_nav_depth = 0usize;
        let mut in_root_ol = false;
        let mut list_stack: Vec<Vec<TocEntry>> = vec![Vec::new()];
        let mut parent_idx_stack: Vec<usize> = Vec::new();
        let mut in_a = false;
        let mut current_href: Option<String> = None;
        let mut current_text = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(e)) => {
                    if e.name().as_ref().ends_with(b"nav") {
                        if let Some(type_val) = read_xml_attr(e.attributes(), b"type") {
                            if type_val.split_whitespace().any(|v| v == "toc") {
                                in_toc_nav = true;
                                toc_nav_depth = 1;
                            }
                        } else if in_toc_nav {
                            toc_nav_depth += 1;
                        }
                    } else if in_toc_nav && e.name().as_ref().ends_with(b"ol") {
                        if !in_root_ol {
                            in_root_ol = true;
                        } else if let Some(parent_idx) = list_stack.last().and_then(|list| list.len().checked_sub(1)) {
                            parent_idx_stack.push(parent_idx);
                            list_stack.push(Vec::new());
                        }
                    } else if in_toc_nav && e.name().as_ref().ends_with(b"a") {
                        in_a = true;
                        current_href = read_xml_attr(e.attributes(), b"href");
                        current_text.clear();
                    }
                }
                Ok(quick_xml::events::Event::Empty(e)) => {
                    if in_toc_nav && e.name().as_ref().ends_with(b"a") {
                        if let Some(href) = read_xml_attr(e.attributes(), b"href") {
                            if let Some(list) = list_stack.last_mut() {
                                list.push(TocEntry { title: String::new(), link: href, children: Vec::new() });
                            }
                        }
                    }
                }
                Ok(quick_xml::events::Event::Text(e)) => {
                    if in_a {
                        current_text.push_str(&e.unescape().unwrap_or_default());
                    }
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    if e.name().as_ref().ends_with(b"a") && in_a {
                        in_a = false;
                        if let Some(href) = current_href.take() {
                            let title = current_text.trim().to_string();
                            if let Some(list) = list_stack.last_mut() {
                                list.push(TocEntry { title, link: href, children: Vec::new() });
                            }
                        }
                        current_text.clear();
                    } else if in_toc_nav && e.name().as_ref().ends_with(b"ol") {
                        if list_stack.len() > 1 {
                            let children = list_stack.pop().unwrap_or_default();
                            if let Some(parent_idx) = parent_idx_stack.pop() {
                                if let Some(parent_list) = list_stack.last_mut() {
                                    if let Some(parent) = parent_list.get_mut(parent_idx) {
                                        parent.children = children;
                                    }
                                }
                            }
                        } else {
                            in_root_ol = false;
                        }
                    } else if e.name().as_ref().ends_with(b"nav") && in_toc_nav {
                        toc_nav_depth = toc_nav_depth.saturating_sub(1);
                        if toc_nav_depth == 0 {
                            in_toc_nav = false;
                        }
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        let root = list_stack.pop().unwrap_or_default();
        if root.is_empty() { None } else { Some(root) }
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
                            if attr.key.as_ref().ends_with(b"src") {
                                if let Some(entry) = stack.last_mut() {
                                    entry.link = String::from_utf8_lossy(&attr.value).into_owned();
                                }
                            }
                        }
                    }
                }
                Ok(quick_xml::events::Event::Empty(e)) => {
                    if e.name().as_ref().ends_with(b"content") {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref().ends_with(b"src") {
                                if let Some(entry) = stack.last_mut() {
                                    entry.link = String::from_utf8_lossy(&attr.value).into_owned();
                                }
                            }
                        }
                    }
                }
                Ok(quick_xml::events::Event::Text(e)) => {
                    if in_text {
                        text_buf.push_str(&e.unescape().unwrap_or_default());
                    }
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    if e.name().as_ref().ends_with(b"text") && in_text {
                        in_text = false;
                        if let Some(entry) = stack.last_mut() {
                            entry.title = text_buf.trim().to_string();
                        }
                    } else if e.name().as_ref().ends_with(b"navPoint") {
                        if let Some(entry) = stack.pop() {
                            if let Some(parent) = stack.last_mut() {
                                parent.children.push(entry);
                            } else {
                                root.push(entry);
                            }
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
    fn resolve_spine_entries(&self) -> io::Result<Option<Vec<String>>> {
        let manifest = self.get_or_parse_manifest();

        if manifest.spine.is_empty() {
            return Ok(None);
        }

        let mut out = Vec::new();

        for idref in &manifest.spine {
            let href = manifest.items.iter().find(|item| &item.id == idref).map(|item| &item.href);
            let Some(href) = href else {
                continue;
            };
            let resolved = resolve_href(&self.opf_dir, href);
            let normalized = normalize_epub_path(&resolved);
            if Self::is_html_entry(&normalized) {
                out.push(normalized);
            }
        }

        if out.is_empty() { Ok(None) } else { Ok(Some(out)) }
    }
}

impl ResourceProvider for EpubProvider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        let mut archive = self.archive.lock().unwrap();
        let mut entry = archive.by_name(uri).map_err(|err| io::Error::new(io::ErrorKind::NotFound, err))?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        use std::io::Read;
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn exists(&self, uri: &str) -> bool {
        let mut archive = match self.archive.lock() {
            Ok(archive) => archive,
            Err(_) => return false,
        };
        archive.by_name(uri).is_ok()
    }

    fn resolve(&self, base: &str, href: &str) -> String {
        let href = href.split('#').next().unwrap_or(href);
        let href = href.split('?').next().unwrap_or(href);
        let href = href.trim_start_matches('/');
        let base_dir = base.rsplitn(2, '/').nth(1).unwrap_or("");
        let combined = if base_dir.is_empty() { href.to_string() } else { format!("{}/{}", base_dir.trim_end_matches('/'), href) };
        normalize_epub_path(&combined)
    }

    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        if let Some(spine) = self.resolve_spine_entries()? {
            return Ok(spine);
        }

        let mut archive = self.archive.lock().unwrap();
        let mut candidates = Vec::new();
        for i in 0..archive.len() {
            let name = archive.by_index(i).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?.name().to_string();
            if Self::is_html_entry(&name) {
                candidates.push(name);
            }
        }
        Ok(candidates)
    }

    fn toc(&self) -> io::Result<Option<Vec<TocEntry>>> {
        EpubProvider::toc(self)
    }
}

fn normalize_epub_path(path: &str) -> String {
    let mut parts = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn read_xml_attr(attrs: quick_xml::events::attributes::Attributes, key: &[u8]) -> Option<String> {
    attrs.flatten().find(|a| a.key.as_ref().ends_with(key)).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn resolve_href(base_dir: &str, href: &str) -> String {
    if base_dir.is_empty() { href.to_string() } else { format!("{}/{}", base_dir.trim_end_matches('/'), href) }
}

fn resolve_toc_links(base_path: &str, entries: &mut [TocEntry]) {
    for entry in entries {
        entry.link = resolve_toc_href(base_path, entry.link.as_str());
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
    } else if path_part.starts_with('/') {
        normalize_epub_path(path_part.trim_start_matches('/'))
    } else if path_part.contains(':') {
        return href.to_string();
    } else {
        let base_dir = base_path.rsplitn(2, '/').nth(1).unwrap_or("");
        let combined = if base_dir.is_empty() { path_part.to_string() } else { format!("{}/{}", base_dir.trim_end_matches('/'), path_part) };
        normalize_epub_path(&combined)
    };
    if fragment.is_empty() { resolved_path } else { format!("{}#{}", resolved_path, fragment) }
}

fn compact_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_opf_metadata(opf_xml: &str) -> EpubMetadata {
    let mut reader = quick_xml::Reader::from_str(opf_xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut metadata = EpubMetadata::default();
    let mut current = None;
    let mut current_scheme: Option<IdentifierScheme> = None;
    let mut current_role: Option<CreatorRole> = None;
    let mut current_file_as: Option<String> = None;
    let mut skip_identifier = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"title") {
                    current = Some(MetaField::Title);
                } else if name.as_ref().ends_with(b"identifier") {
                    current = Some(MetaField::Identifier);
                    skip_identifier = false;

                    // Extract scheme attribute - skip identifier if no scheme or unknown scheme
                    if let Some(scheme_str) = read_xml_attr(e.attributes(), b"scheme") {
                        match parse_identifier_scheme(&scheme_str) {
                            Some(scheme) => current_scheme = Some(scheme),
                            None => {
                                // Unknown scheme, skip this identifier
                                skip_identifier = true;
                                current_scheme = None;
                            }
                        }
                    } else {
                        // No scheme attribute, skip this identifier
                        skip_identifier = true;
                        current_scheme = None;
                    }
                } else if name.as_ref().ends_with(b"creator") {
                    current = Some(MetaField::Creator);
                    // Extract role attribute (opf:role), default to Author if not present
                    current_role = Some(read_xml_attr(e.attributes(), b"role").map(|r| parse_creator_role(&r)).unwrap_or(CreatorRole::Author));
                    // Extract file-as attribute (opf:file-as)
                    current_file_as = read_xml_attr(e.attributes(), b"file-as");
                } else if name.as_ref().ends_with(b"contributor") {
                    // Extract role attribute (opf:role), skip if not present
                    current_role = read_xml_attr(e.attributes(), b"role").map(|r| parse_creator_role(&r));

                    // Only process contributors with a role
                    if current_role.is_some() {
                        current = Some(MetaField::Contributor);
                        // Extract file-as attribute (opf:file-as)
                        current_file_as = read_xml_attr(e.attributes(), b"file-as");
                    }
                } else if name.as_ref().ends_with(b"description") {
                    current = Some(MetaField::Description);
                } else if name.as_ref().ends_with(b"publisher") {
                    current = Some(MetaField::Publisher);
                } else if name.as_ref().ends_with(b"language") {
                    current = Some(MetaField::Language);
                } else if name.as_ref().ends_with(b"date") {
                    current = Some(MetaField::Date);
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
                {
                    current = None;
                    current_scheme = None;
                    current_role = None;
                    current_file_as = None;
                    skip_identifier = false;
                }
            }
            Ok(quick_xml::events::Event::Text(t)) => {
                if let Some(field) = current {
                    let text = t.unescape().unwrap_or_default().into_owned();
                    if text.is_empty() {
                        continue;
                    }
                    match field {
                        MetaField::Title if metadata.title.is_empty() => {
                            metadata.title = text;
                        }
                        MetaField::Identifier if !skip_identifier => {
                            // unwrap is safe here because skip_identifier=false means scheme exists
                            metadata.identifiers.push(EpubIdentifier { value: text, scheme: current_scheme.clone().unwrap() });
                        }
                        MetaField::Creator => {
                            // unwrap is safe here because we always set current_role for creators
                            metadata.creators.push(EpubCreator { name: text, file_as: current_file_as.clone(), role: current_role.clone().unwrap() });
                        }
                        MetaField::Contributor => {
                            // unwrap is safe here because we only set current=Contributor if role exists
                            metadata.contributors.push(EpubCreator { name: text, file_as: current_file_as.clone(), role: current_role.clone().unwrap() });
                        }
                        MetaField::Description if metadata.description.is_empty() => {
                            metadata.description = text;
                        }
                        MetaField::Publisher if metadata.publisher.is_empty() => {
                            metadata.publisher = text;
                        }
                        MetaField::Language if metadata.language.is_empty() => {
                            metadata.language = text;
                        }
                        MetaField::Date if metadata.date.is_empty() => {
                            metadata.date = text;
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

    metadata
}

fn parse_creator_name(name: &str) -> String {
    let name = name.trim();
    // Check if it's in "Last, First" format
    if let Some((last, first)) = name.split_once(',') { format!("{} {}", first.trim(), last.trim()) } else { name.to_string() }
}

#[derive(Clone, Copy)]
enum MetaField {
    Title,
    Identifier,
    Creator,
    Contributor,
    Description,
    Publisher,
    Language,
    Date,
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
            Ok(quick_xml::events::Event::End(e)) => {
                if e.name().as_ref().ends_with(b"guide") {
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

fn parse_opf_first_spine_idref(opf_xml: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(opf_xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut in_spine = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let name = e.name();
                if name.as_ref().ends_with(b"spine") {
                    in_spine = true;
                } else if in_spine && name.as_ref().ends_with(b"itemref") {
                    if let Some(idref) = read_xml_attr(e.attributes(), b"idref") {
                        return Some(idref);
                    }
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                if e.name().as_ref().ends_with(b"spine") {
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

fn parse_first_image_href_from_html(html: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(html);
    reader.trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                let tag = e.name();
                if tag.as_ref().ends_with(b"img") {
                    if let Some(src) = read_xml_attr(e.attributes(), b"src") {
                        return Some(src);
                    }
                } else if tag.as_ref().ends_with(b"image") {
                    if let Some(href) = read_xml_attr(e.attributes(), b"href").or_else(|| read_xml_attr(e.attributes(), b"xlink:href")) {
                        return Some(href);
                    }
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

fn pick_best_cover_image_item(items: &[ManifestItem]) -> Option<&ManifestItem> {
    let mut best: Option<(&ManifestItem, i32)> = None;
    for item in items {
        if !is_manifest_item_image(item) {
            continue;
        }
        let text = format!("{} {}", item.id.to_ascii_lowercase(), item.href.to_ascii_lowercase());
        let mut score = 0i32;
        if text.contains("cover") {
            score += 100;
        }
        if text.contains("front") {
            score += 60;
        }
        if text.contains("title") {
            score += 20;
        }
        if text.contains("pub") {
            score += 12;
        }
        if text.contains("image-z") {
            score += 8;
        }
        if text.contains("back") {
            score -= 40;
        }
        if text.contains("logo") {
            score -= 30;
        }
        if text.contains("icon") {
            score -= 25;
        }

        if let Some((_, best_score)) = best {
            if score > best_score {
                best = Some((item, score));
            }
        } else {
            best = Some((item, score));
        }
    }
    best.map(|(item, _)| item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IdentifierScheme::Other;
    use std::fs;
    use std::path::Path;

    #[test]
    #[ignore] // Run with: cargo test -- --ignored --nocapture
    fn test_recursive_metadata_extraction() {
        // Set the directory to scan - change this to your test directory
        let test_dir = std::env::var("EPUB_TEST_DIR").unwrap_or_else(|_| ".".to_string());

        //println!("\nScanning directory: {}", test_dir);
        //println!("{}", "=".repeat(80));

        extract_metadata_recursive(Path::new(&test_dir));
    }

    fn extract_metadata_recursive(dir: &Path) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("Failed to read directory {:?}: {}", dir, e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                extract_metadata_recursive(&path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("epub") {
                // println!("\n📚 File: {}", path.display());
                // println!("{}", "-".repeat(80));

                match EpubProvider::try_new(path.clone()) {
                    Ok(provider) => {
                        match provider.metadata() {
                            Ok(metadata) => {
                                for id in &metadata.identifiers {
                                    match &id.scheme {
                                        //Other(other) => {eprintln!("scheme: {:?}, open: {:?}", other, path)}
                                        _ => (),
                                    }
                                }

                                for creator in &metadata.contributors {
                                    match &creator.role {
                                        CreatorRole::Author => {}
                                        CreatorRole::Other(other) => {
                                            if other.eq("bkp") {
                                                continue;
                                            }
                                            eprintln!("Book: {}", path.display());

                                            eprintln!("Unsupported creator role: {:?} {}", other, creator.name);
                                        }
                                        other => {
                                            println!("Other role: {:?}", other);
                                        }
                                    }
                                }
                            }
                            Err(e) => eprintln!("  ❌ Failed to extract metadata: {}", e),
                        }
                    }
                    Err(e) => eprintln!("  ❌ Failed to open EPUB: {}", e),
                }
            }
        }
    }
}
