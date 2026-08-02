use html_dom::{Document, ImageSource, RootFontSize};
use html_layout::PreparedDocument;
use html_parse::{ParsedHtml, StylesheetReference};
use html_resources::ResourceProvider;
use html_style::StylesheetEntry;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use html_parse::parse_html_document;

pub struct DocumentFactory {
    root_font_size: RootFontSize,
    title: Option<String>,
    resources: Option<ResourceContext>,
}

struct ResourceContext {
    provider: Arc<dyn ResourceProvider>,
    base_uri: String,
}

#[allow(dead_code)]
impl DocumentFactory {
    pub fn new() -> Self {
        Self { root_font_size: RootFontSize::default(), title: None, resources: None }
    }

    pub fn set_resource_context(&mut self, provider: Arc<dyn ResourceProvider>, base_uri: impl Into<String>) {
        self.resources = Some(ResourceContext { provider, base_uri: base_uri.into() });
    }

    pub fn set_root_font_size(&mut self, root_font_size: RootFontSize) {
        self.root_font_size = root_font_size;
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    pub fn parse_to_dom(&mut self, html: &str) -> Document {
        let parsed = parse_html_document(html);
        self.configured_dom(&parsed)
    }

    pub fn parse_with_new_pipeline(&mut self, html: &str, css: Option<&str>) -> PreparedDocument {
        match css {
            Some(css) => self.parse_with_new_pipeline_css_chunks(html, &[css]),
            None => self.parse_with_new_pipeline_css_chunks(html, &[]),
        }
    }

    pub fn parse_with_new_pipeline_css_chunks<'css>(&mut self, html: &str, css_chunks: &[&'css str]) -> PreparedDocument {
        let parsed_html = parse_html_document(html);
        self.build_pipeline_from_parsed_impl(&parsed_html, css_chunks, None)
    }

    pub fn parse_with_new_pipeline_css_chunks_timed<'css>(&mut self, html: &str, css_chunks: &[&'css str]) -> (PreparedDocument, BuildPipelineTimings) {
        let parsed_html = parse_html_document(html);
        self.build_pipeline_from_parsed_timed(&parsed_html, css_chunks)
    }

    pub fn build_pipeline_from_parsed<'css>(&mut self, parsed_html: &ParsedHtml, css_chunks: &[&'css str]) -> PreparedDocument {
        self.build_pipeline_from_parsed_impl(parsed_html, css_chunks, None)
    }

    pub fn build_pipeline_from_parsed_timed<'css>(&mut self, parsed_html: &ParsedHtml, css_chunks: &[&'css str]) -> (PreparedDocument, BuildPipelineTimings) {
        let mut timings = BuildPipelineTimings::default();
        let document = self.build_pipeline_from_parsed_impl(parsed_html, css_chunks, Some(&mut timings));
        (document, timings)
    }

    fn build_pipeline_from_parsed_impl<'css>(&mut self, parsed_html: &ParsedHtml, css_chunks: &[&'css str], mut timings: Option<&mut BuildPipelineTimings>) -> PreparedDocument {
        let dom_started = Instant::now();
        let mut document = self.configured_dom(parsed_html);
        if let Some(timings) = timings.as_deref_mut() {
            timings.build_dom_tree += dom_started.elapsed();
        }

        let toc_started = Instant::now();
        document.rebuild_document_toc_entries();
        if let Some(timings) = timings.as_deref_mut() {
            timings.rebuild_document_toc += toc_started.elapsed();
        }

        let css_started = Instant::now();
        let mut expanded_css = Vec::new();
        let base_uri = self.resources.as_ref().map_or("", |resources| resources.base_uri.as_str()).to_owned();
        for css in css_chunks {
            self.expand_stylesheet(css, &base_uri, None, &mut HashSet::new(), 0, &mut expanded_css);
        }
        for stylesheet in parsed_html.head_metadata().stylesheets {
            match stylesheet {
                StylesheetReference::Inline(css) => self.expand_stylesheet(&css, &base_uri, None, &mut HashSet::new(), 0, &mut expanded_css),
                StylesheetReference::External(href) => {
                    if let Some(resources) = self.resources.as_ref() {
                        let uri = resources.provider.resolve(&resources.base_uri, &href);
                        if let Ok(css) = resources.provider.read_string(&uri) {
                            self.expand_stylesheet(&css, &uri, Some(&uri), &mut HashSet::new(), 0, &mut expanded_css);
                        }
                    }
                }
            }
        }
        if let Some(timings) = timings.as_deref_mut() {
            timings.resolve_css_imports += css_started.elapsed();
        }
        let all_css = expanded_css.iter().map(String::as_str).collect::<Vec<_>>();
        let (styled, style_timings) = html_style::style_document_with_timings(document, &all_css);
        if let Some(timings) = timings.as_deref_mut() {
            timings.parse_default_css += style_timings.parse_default_css;
            timings.parse_author_css += style_timings.parse_author_css;
            timings.prepare_style_rules += style_timings.prepare_rules;
            timings.resolve_styles += style_timings.resolve_styles;
            timings.selector_index += style_timings.selector_index;
            timings.resolver_setup += style_timings.resolver_setup;
            timings.selector_matching += style_timings.selector_matching;
            timings.cascade += style_timings.cascade;
            timings.style_store += style_timings.style_store;
        }

        let layout_started = Instant::now();
        let (document, styles) = styled.into_parts();
        let prepared = PreparedDocument::try_new(document, styles).expect("style resolver must produce complete styles for its document");
        if let Some(timings) = timings.as_deref_mut() {
            timings.build_layout_inputs += layout_started.elapsed();
        }
        prepared
    }

    fn configured_dom(&self, parsed: &ParsedHtml) -> Document {
        let metadata = parsed.head_metadata();
        let mut document = parsed.build_dom();
        document.set_root_font_size(self.root_font_size);
        document.set_title(self.title.clone().or(metadata.title));
        if let Some(resources) = self.resources.as_ref() {
            for image in document.images_mut() {
                let ImageSource::Uri(uri) = &mut image.source;
                *uri = resources.provider.resolve(&resources.base_uri, uri);
            }
        }
        document
    }

    fn expand_stylesheet(&self, css: &str, base_uri: &str, source_uri: Option<&str>, active_imports: &mut HashSet<String>, depth: usize, output: &mut Vec<String>) {
        const MAX_IMPORT_DEPTH: usize = 32;
        if depth > MAX_IMPORT_DEPTH {
            return;
        }
        if let Some(uri) = source_uri
            && !active_imports.insert(uri.to_owned())
        {
            return;
        }

        for entry in html_style::stylesheet_entries(css) {
            match entry {
                StylesheetEntry::Css(css) if !css.trim().is_empty() => output.push(css),
                StylesheetEntry::Css(_) => {}
                StylesheetEntry::Import(import) => {
                    let Some(resources) = self.resources.as_ref() else { continue };
                    let uri = resources.provider.resolve(base_uri, &import.url);
                    if active_imports.contains(&uri) {
                        continue;
                    }
                    let Ok(imported_css) = resources.provider.read_string(&uri) else { continue };
                    let mut imported_output = Vec::new();
                    self.expand_stylesheet(&imported_css, &uri, Some(&uri), active_imports, depth + 1, &mut imported_output);
                    if !imported_output.is_empty() {
                        output.push(import.wrap_resolved_css(&imported_output.join(" ")));
                    }
                }
            }
        }

        if let Some(uri) = source_uri {
            active_imports.remove(uri);
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct BuildPipelineTimings {
    pub build_dom_tree: Duration,
    pub parse_default_css: Duration,
    pub parse_author_css: Duration,
    pub resolve_css_imports: Duration,
    pub prepare_style_rules: Duration,
    pub resolve_styles: Duration,
    pub selector_index: Duration,
    pub resolver_setup: Duration,
    pub selector_matching: Duration,
    pub cascade: Duration,
    pub style_store: Duration,
    pub build_layout_inputs: Duration,
    pub rebuild_document_toc: Duration,
}

#[cfg(test)]
mod tests {
    use super::DocumentFactory;
    use html_dom::{Document, ElementRef, NodeRef};
    use html_resources::ResourceProvider;
    use std::collections::HashMap;
    use std::io;
    use std::sync::Arc;

    struct MemoryProvider {
        resources: HashMap<String, Vec<u8>>,
    }

    impl ResourceProvider for MemoryProvider {
        fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
            self.resources.get(uri).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, uri.to_owned()))
        }

        fn exists(&self, uri: &str) -> bool {
            self.resources.contains_key(uri)
        }

        fn resolve(&self, base: &str, href: &str) -> String {
            let parent = base.rsplit_once('/').map(|(parent, _)| parent).unwrap_or("");
            if parent.is_empty() { href.to_owned() } else { format!("{parent}/{href}") }
        }

        fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn keeps_whitespace_only_text_nodes_in_inline_content() {
        let html = "<html><body><span>a</span> <span>b</span></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_to_dom(html);

        let body = find_body(&document);
        let children: Vec<_> = body.children().collect();

        assert_eq!(children.len(), 3, "body should keep the space node");

        let middle_text = document.text_ref(children[1]).expect("expected whitespace text node");

        assert_eq!(middle_text.text(), " ");
    }

    #[test]
    fn builds_toc_entries_from_html_headers() {
        let html = "<html><body><h1>Intro</h1><h2 id=\"chapter-a\">Chapter <em>A</em></h2></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_to_dom(html);

        assert_eq!(document.toc_entries().len(), 2);

        let first = &document.toc_entries()[0];
        assert_eq!(first.level, 1);
        assert_eq!(document.string(first.title), "Intro");
        assert!(document.string(first.href).starts_with("#heading-"));

        let second = &document.toc_entries()[1];
        assert_eq!(second.level, 2);
        assert_eq!(document.string(second.title), "Chapter A");
        assert_eq!(document.string(second.href), "#chapter-a");
    }

    #[test]
    fn handles_self_closing_anchor_without_leaking_link_scope() {
        let html = "<html><body><p><a id=\"p77\"/>After</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_to_dom(html);

        let body = find_body(&document);
        let p = child_element_by_tag(&document, body, "p");
        let children: Vec<_> = p.children().collect();

        assert_eq!(children.len(), 2, "self-closed anchor should not wrap following text");
        assert_eq!(document.element_ref(children[0]).expect("first child should be anchor").tag(), "a");
        assert!(matches!(document.node_ref(children[1]), Some(NodeRef::Text(text)) if text.text() == "After"));
    }

    #[test]
    fn resolves_linked_and_inline_stylesheets_in_document_order() {
        let provider = Arc::new(MemoryProvider { resources: HashMap::from([("book/first.css".to_owned(), b"p { color: red; }".to_vec()), ("book/last.css".to_owned(), b"p { color: blue; }".to_vec())]) });
        let html = r#"
            <html><head>
                <link rel="stylesheet" href="first.css">
                <style>p { color: green; }</style>
                <link rel="stylesheet" href="last.css">
            </head><body><p>text</p></body></html>
        "#;
        let mut factory = DocumentFactory::new();
        factory.set_resource_context(provider, "book/chapter.xhtml");

        let prepared = factory.parse_with_new_pipeline(html, None);
        let paragraph = (0..prepared.box_count()).find(|&box_idx| prepared.get_tag(box_idx) == "p").expect("paragraph box");
        assert_eq!(prepared.box_text_color(paragraph), Some(0x0000FFFF));
    }

    #[test]
    fn resolves_nested_imports_relative_to_the_importing_stylesheet_and_breaks_cycles() {
        let provider = Arc::new(MemoryProvider { resources: HashMap::from([("book/a.css".to_owned(), b"@import 'b.css'; p { color: red; }".to_vec()), ("book/b.css".to_owned(), b"@import 'a.css'; p { font-weight: 700; }".to_vec())]) });
        let html = "<html><head><style>@import 'a.css';</style></head><body><p>text</p></body></html>";
        let mut factory = DocumentFactory::new();
        factory.set_resource_context(provider, "book/chapter.xhtml");

        let prepared = factory.parse_with_new_pipeline(html, None);
        let paragraph = (0..prepared.box_count()).find(|&box_idx| prepared.get_tag(box_idx) == "p").expect("paragraph box");
        assert_eq!(prepared.box_text_color(paragraph), Some(0xFF0000FF));
    }

    #[test]
    fn import_conditions_and_layers_survive_resource_expansion() {
        let provider = Arc::new(MemoryProvider { resources: HashMap::from([("book/theme.css".to_owned(), b"p { color: red !important; }".to_vec())]) });
        let html = "<html><head><style>@import 'theme.css' layer(theme) supports(width: 1px) screen; p { color: green !important; }</style></head><body><p>text</p></body></html>";
        let mut factory = DocumentFactory::new();
        factory.set_resource_context(provider, "book/chapter.xhtml");

        let prepared = factory.parse_with_new_pipeline(html, None);
        let paragraph = (0..prepared.box_count()).find(|&box_idx| prepared.get_tag(box_idx) == "p").expect("paragraph box");
        assert_eq!(prepared.box_text_color(paragraph), Some(0xFF0000FF), "important layered imports outrank important unlayered author rules");
    }

    fn find_body(document: &Document) -> ElementRef<'_> {
        let html_idx = document.dom_root().expect("html root should exist");
        let html = document.element_ref(html_idx).expect("html should be element");
        child_element_by_tag(document, html, "body")
    }

    fn child_element_by_tag<'a>(document: &'a Document, parent: ElementRef<'a>, tag: &str) -> ElementRef<'a> {
        parent.children().find_map(|child_idx| document.element_ref(child_idx).filter(|element| element.tag() == tag)).expect("child element should exist")
    }
}
