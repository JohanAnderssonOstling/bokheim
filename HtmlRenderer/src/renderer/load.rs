use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

use scraper::{Html, Selector};

use crate::document::{AnchorPosition, BoxType, Document, InlineKind};
use crate::layout;
use crate::parser::{parse_html_document, DocumentFactory};
use crate::resources::{probe_dimensions, EpubProvider, FileSystemProvider, ResourceProvider};
use crate::text::glyph_cache::GlyphCache;
use crate::text::shaping as text_shaping;

pub(super) fn load_css_with_provider(provider: &dyn ResourceProvider, base_uri: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    for candidate in ["computer_style.css", "style.css"] {
        let uri = provider.resolve(base_uri, candidate);
        if provider.exists(&uri) {
            if let Ok(extra) = provider.read_string(&uri) {
                chunks.push(extra);
            }
        }
    }
    chunks
}

pub(super) fn load_document_with_dom_pipeline(provider: Arc<dyn ResourceProvider>, uri: &str, root_font_size: f32, col_width: f64) -> (Document, GlyphCache) {
    // 1. Load HTML
    let html = provider.read_string(uri).expect("Failed to load HTML");
    // 2. Parse the HTML once; title, embedded CSS, and the DOM all derive from it.
    let parsed_html = parse_html_document(&html);
    // 3. Extract title (reusable)
    let title = extract_title(&parsed_html);
    // 4. Load external CSS (reusable)
    let mut css_chunks = load_css_with_provider(provider.as_ref(), uri);

    // 5. Collect inline CSS from HTML (reusable)
    css_chunks.extend(collect_css_from_html(&parsed_html, provider.as_ref(), uri));
    // 6. Use new DOM pipeline (parsed HTML → DOM → Styled DOM → Boxes)
    let mut factory = DocumentFactory::new();
    factory.set_provider(provider.clone());
    factory.set_base_uri(uri);
    // Root font size must be set before style resolution so rem/em-based
    // computed font sizes and subsequent glyph shaping use the requested value.
    factory.document.root_font_size = root_font_size;
    let css_refs: Vec<&str> = css_chunks.iter().map(String::as_str).collect();
    let mut document = factory.build_pipeline_from_parsed(&parsed_html, &css_refs);

    // 6. Set document metadata
    document.title = title;
    document.root_font_size = root_font_size;
    // 7. Probe image dimensions (reusable)
    for image in &mut document.images {
        if image.width == 0 || image.height == 0 {
            if let Some((w, h)) = probe_dimensions(provider.as_ref(), &image.source) {
                image.width = w;
                image.height = h;
            }
        }
        // Calculate aspect-ratio dimensions
        if let (Some(width), None) = (image.width_attr, image.height_attr) {
            if image.width > 0 && image.height > 0 {
                let scaled = (width as f64 * image.height as f64 / image.width as f64).round() as u32;
                image.height_attr = Some(scaled.max(1));
            }
        } else if let (None, Some(height)) = (image.width_attr, image.height_attr) {
            if image.width > 0 && image.height > 0 {
                let scaled = (height as f64 * image.width as f64 / image.height as f64).round() as u32;
                image.width_attr = Some(scaled.max(1));
            }
        }
    }
    // 8. Shape text (reusable)
    let mut glyph_cache = GlyphCache::new();
    text_shaping::shape_document(&mut document, &mut glyph_cache);
    build_link_glyph_hrefs(&mut document);
    // 9. Build anchor glyphs (reusable)
    build_anchor_glyphs(&mut document);

    // 10. Layout (reusable)
    let mut layout_engine = layout::LayoutEngine::new(col_width, root_font_size as f64);
    let (mut document, glyph_cache) = layout_engine.layout(document, glyph_cache);

    // 11. Build anchor positions (reusable)
    build_anchor_positions(&mut document);

    (document, glyph_cache)
}

pub(crate) fn load_document(provider: Arc<dyn ResourceProvider>, uri: &str, root_font_size: f32, col_width: f64) -> (Document, GlyphCache) {
    // Use the new DOM-based pipeline
    load_document_with_dom_pipeline(provider, uri, root_font_size, col_width)
}

fn extract_title(doc: &Html) -> Option<String> {
    let selector = Selector::parse("head title").ok()?;
    for title in doc.select(&selector) {
        let text = title.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

pub(super) fn build_anchor_positions(document: &mut Document) {
    document.layout.anchor_positions.clear();
    if document.box_tree.boxes.is_empty() {
        return;
    }
    for box_idx in 0..document.box_tree.boxes.len() {
        let Some(id_idx) = document.get_box_id(box_idx) else { continue };
        let mut pos_y = None;
        if let Some(glyph_idx) = document.anchor_glyphs.get(&id_idx).copied() {
            pos_y = line_y_for_glyph(document, glyph_idx);
        }
        if pos_y.is_none() {
            let mut current = Some(box_idx);
            while let Some(idx) = current {
                let node = &document.box_tree.boxes[idx];
                if node.size.height > 0.0 || node.point.y != 0.0 {
                    pos_y = Some(node.point.y);
                    break;
                }
                current = node.parent.map(|p| p as usize);
            }
        }
        let Some(pos_y) = pos_y else { continue };
        let order = document.anchor_glyphs.get(&id_idx).copied().unwrap_or(box_idx as u32);
        match document.layout.anchor_positions.get_mut(&id_idx) {
            Some(existing) => {
                if pos_y < existing.y || (pos_y == existing.y && order < existing.order) {
                    existing.y = pos_y;
                    existing.order = order;
                }
            }
            None => {
                document.layout.anchor_positions.insert(id_idx, AnchorPosition { y: pos_y, order });
            }
        }
    }
}

fn line_y_for_glyph(document: &Document, glyph_idx: u32) -> Option<f64> {
    for line in &document.layout.lines {
        if line.glyphs.start <= glyph_idx && glyph_idx < line.glyphs.end {
            return Some(line.point.y);
        }
    }
    None
}

fn build_link_glyph_hrefs(document: &mut Document) {
    document.link_glyph_hrefs.clear();

    for run in &document.box_tree.inline_runs {
        let InlineKind::Text { glyphs } = &run.kind else {
            continue;
        };
        if glyphs.start >= glyphs.end {
            continue;
        }

        let mut current = Some(run.box_idx as usize);
        let mut href_idx = None;
        while let Some(box_idx) = current {
            if let Some(href) = document.get_box_href(box_idx) {
                href_idx = Some(href);
                break;
            }
            current = document.box_tree.boxes[box_idx].parent.map(|p| p as usize);
        }

        let Some(href_idx) = href_idx else {
            continue;
        };

        for glyph_idx in glyphs.clone() {
            document.link_glyph_hrefs.insert(glyph_idx, href_idx);
        }
    }
}

fn build_anchor_glyphs(document: &mut Document) {
    document.anchor_glyphs.clear();
    if document.box_tree.boxes.is_empty() {
        return;
    }

    let mut min_glyphs: Vec<Option<u32>> = vec![None; document.box_tree.boxes.len()];
    for run in &document.box_tree.inline_runs {
        let InlineKind::Text { glyphs } = &run.kind else {
            continue;
        };
        if glyphs.start >= glyphs.end {
            continue;
        }
        let mut current = Some(run.box_idx as usize);
        while let Some(idx) = current {
            let entry = &mut min_glyphs[idx];
            if entry.map_or(true, |existing| glyphs.start < existing) {
                *entry = Some(glyphs.start);
            }
            current = document.box_tree.boxes[idx].parent.map(|p| p as usize);
        }
    }

    for box_idx in 0..document.box_tree.boxes.len() {
        if document.get_box_id(box_idx).is_none() || min_glyphs[box_idx].is_some() {
            continue;
        }
        let BoxType::Inline(range) = &document.box_tree.boxes[box_idx].layout_mode else {
            continue;
        };
        let run_idx = range.start as usize;
        let anchor_block = block_ancestor_idx(document, box_idx);
        let mut found = None;
        for run in document.box_tree.inline_runs[run_idx..].iter() {
            let InlineKind::Text { glyphs } = &run.kind else { continue };
            if glyphs.start < glyphs.end {
                let run_block = block_ancestor_idx(document, run.box_idx as usize);
                if run_block == anchor_block {
                    found = Some(glyphs.start);
                    break;
                }
            }
        }
        if found.is_none() {
            for run in document.box_tree.inline_runs[..run_idx].iter().rev() {
                let InlineKind::Text { glyphs } = &run.kind else { continue };
                if glyphs.start < glyphs.end {
                    let run_block = block_ancestor_idx(document, run.box_idx as usize);
                    if run_block == anchor_block {
                        found = Some(glyphs.end.saturating_sub(1));
                        break;
                    }
                }
            }
        }
        if let Some(glyph_idx) = found {
            min_glyphs[box_idx] = Some(glyph_idx);
        }
    }

    for idx in 0..document.box_tree.boxes.len() {
        let Some(id_idx) = document.get_box_id(idx) else { continue };
        if let Some(glyph_idx) = min_glyphs[idx] {
            document.anchor_glyphs.insert(id_idx, glyph_idx);
        }
    }
}

fn block_ancestor_idx(document: &Document, box_idx: usize) -> Option<usize> {
    let mut current = Some(box_idx);
    while let Some(idx) = current {
        let node = &document.box_tree.boxes[idx];
        if matches!(node.layout_mode, BoxType::Block(_) | BoxType::Table(_) | BoxType::TableCell(_)) {
            return Some(idx);
        }
        current = node.parent.map(|p| p as usize);
    }
    None
}

pub(crate) fn collect_documents(path: PathBuf) -> (Arc<dyn ResourceProvider>, Vec<String>, usize) {
    if path.is_dir() {
        let provider: Arc<dyn ResourceProvider> = Arc::new(FileSystemProvider::new());
        let root = path.to_string_lossy().into_owned();
        let candidates = provider.list_html_candidates(&root).expect("Failed to list HTML candidates");
        let start_index = candidates.iter().position(|c| c.ends_with("index.html")).unwrap_or(0);
        return (provider, candidates, start_index);
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "epub" {
        let provider: Arc<dyn ResourceProvider> = Arc::new(EpubProvider::new(path));
        let candidates = provider.list_html_candidates("").expect("Failed to list HTML entries in EPUB");
        return (provider, candidates, 0);
    }

    let provider: Arc<dyn ResourceProvider> = Arc::new(FileSystemProvider::new());
    let html_uri = path.to_string_lossy().into_owned();
    (provider, vec![html_uri], 0)
}

pub(crate) fn collect_documents_from_epub_bytes(bytes: Vec<u8>) -> (Arc<dyn ResourceProvider>, Vec<String>, usize) {
    let provider: Arc<dyn ResourceProvider> = Arc::new(EpubProvider::try_from_reader(Cursor::new(bytes)).expect("Failed to open EPUB bytes"));
    let candidates = provider.list_html_candidates("").expect("Failed to list HTML entries in EPUB");
    (provider, candidates, 0)
}

#[cfg(test)]
mod tests {
    use super::{build_link_glyph_hrefs, collect_css_from_html};
    use crate::document::InlineKind;
    use crate::parser::DocumentFactory;
    use crate::resources::ResourceProvider;
    use crate::text::glyph_cache::GlyphCache;
    use crate::text::shaping::shape_document;
    use std::collections::HashMap;
    use std::io;

    #[test]
    fn nested_inline_text_in_anchor_maps_glyphs_to_href() {
        let html = "<html><body><a href=\"chapter2.xhtml#target\"><span>Link</span></a></body></html>";
        let mut factory = DocumentFactory::new();
        let mut document = factory.parse_with_new_pipeline(html, None);

        let mut glyph_cache = GlyphCache::new();
        shape_document(&mut document, &mut glyph_cache);
        build_link_glyph_hrefs(&mut document);

        let href_idx = document.strings.lookup("chapter2.xhtml#target").expect("href should be interned");
        let span_idx = (0..document.box_tree.boxes.len()).find(|&i| document.get_tag(i) == "span").expect("span box should exist");
        let glyphs = document
            .box_tree
            .inline_runs
            .iter()
            .find_map(|run| match &run.kind {
                InlineKind::Text { glyphs } if run.box_idx as usize == span_idx => Some(glyphs.clone()),
                _ => None,
            })
            .expect("span text run should exist");

        assert!(glyphs.start < glyphs.end, "span text should produce glyphs");
        for glyph_idx in glyphs {
            assert_eq!(document.link_glyph_hrefs.get(&glyph_idx).copied(), Some(href_idx));
        }
    }

    #[test]
    fn collects_css_chunks_in_document_order_with_permissive_link_attrs() {
        struct TestProvider {
            files: HashMap<String, String>,
        }

        impl ResourceProvider for TestProvider {
            fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
                self.files.get(uri).map(|css| css.as_bytes().to_vec()).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, uri.to_string()))
            }

            fn exists(&self, uri: &str) -> bool {
                self.files.contains_key(uri)
            }

            fn resolve(&self, _base: &str, href: &str) -> String {
                href.to_string()
            }

            fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
                Ok(Vec::new())
            }

            fn toc(&self) -> io::Result<Option<Vec<crate::TocEntry>>> {
                Ok(None)
            }
        }

        let provider = TestProvider {
            files: HashMap::from([
                ("base.css?v=1".to_string(), "p { color: red; }".to_string()),
                ("override.css".to_string(), "p { color: green; }".to_string()),
            ]),
        };
        let html = r#"
            <html><head>
                <link rel="preload StyleSheet" type="Text/CSS; charset=utf-8" href="base.css?v=1">
                <style>p { color: blue; }</style>
                <link rel="stylesheet" href="override.css">
            </head><body><p>Hello</p></body></html>
        "#;

        let parsed = crate::parser::parse_html_document(html);
        let chunks = collect_css_from_html(&parsed, &provider, "chapter.xhtml");

        assert_eq!(chunks, vec!["p { color: red; }", "p { color: blue; }", "p { color: green; }"]);
    }
}

pub(super) fn collect_css_from_html(doc: &Html, provider: &dyn ResourceProvider, base_uri: &str) -> Vec<String> {
    let mut chunks = Vec::new();

    if let Ok(selector) = Selector::parse("link, style") {
        for element in doc.select(&selector) {
            let tag = element.value().name();
            if tag.eq_ignore_ascii_case("style") {
                let text = element.text().collect::<String>();
                if !text.trim().is_empty() {
                    chunks.push(text);
                }
                continue;
            }

            if !tag.eq_ignore_ascii_case("link") || !link_is_stylesheet(element.value().attr("rel")) || !link_type_is_css(element.value().attr("type")) {
                continue;
            }

            if let Some(href) = element.value().attr("href") {
                let uri = provider.resolve(base_uri, href);
                if let Ok(css) = provider.read_string(&uri) {
                    chunks.push(css);
                }
            }
        }
    }

    chunks
}

fn link_is_stylesheet(rel: Option<&str>) -> bool {
    rel.map(|rel| rel.split_ascii_whitespace().any(|token| token.eq_ignore_ascii_case("stylesheet"))).unwrap_or(false)
}

fn link_type_is_css(mime: Option<&str>) -> bool {
    let Some(mime) = mime else {
        return true;
    };
    let content_type = mime.split(';').next().unwrap_or("").trim();
    content_type.eq_ignore_ascii_case("text/css")
}
