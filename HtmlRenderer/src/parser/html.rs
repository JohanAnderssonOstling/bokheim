use crate::document::{Document, DocumentMode, DocumentTocEntry, DomAttribute, DomElement, DomNode, DomText, ImageResource, ImageSource};
use crate::resources::ResourceProvider;
use scraper::{ElementRef, Html, Node};
use std::mem;
use std::path::PathBuf;
use std::sync::Arc;

pub struct DocumentFactory {
    pub document: Document,
    base_dir: Option<PathBuf>,
    base_uri: Option<String>,
    provider: Option<Arc<dyn ResourceProvider>>,
}

#[allow(dead_code)]
impl DocumentFactory {
    pub fn new() -> Self {
        Self { document: Document::new(), base_dir: None, base_uri: None, provider: None }
    }

    pub fn set_provider(&mut self, provider: Arc<dyn ResourceProvider>) {
        self.provider = Some(provider);
    }

    pub fn set_base_uri(&mut self, base_uri: impl Into<String>) {
        self.base_uri = Some(base_uri.into());
    }

    // ========== NEW: DOM TREE BUILDING METHODS ==========
    //
    // NEW ARCHITECTURE PIPELINE:
    // 1. parse_to_dom() → Build DOM tree from HTML
    // 2. resolve_styles_for_dom() → Apply CSS styles to DOM elements
    // 3. build_box_tree() → Create layout boxes based on computed styles
    // 4. layout() → Position and size boxes
    //
    // This separates parsing from layout and allows CSS display property
    // to control block/inline behavior instead of hardcoded tag names.
    //

    /// Parse HTML and build DOM tree (Phase 1)
    /// Returns a Document with populated dom_nodes and dom_root
    pub fn parse_to_dom(&mut self, html: &str) -> Document {
        self.document.mode = DocumentMode::Html;
        let doc = parse_html_document(html);
        if let Some(root) = self.select_document_root(&doc) {
            let root_idx = self.build_dom_element(root, None);
            self.document.dom_root = Some(root_idx);
        }
        self.document.rebuild_document_toc_entries();
        mem::take(&mut self.document)
    }

    /// Complete new pipeline: Parse HTML → Build DOM → Resolve Styles → Build Boxes
    /// This uses the CSS display property instead of hardcoded tag names
    pub fn parse_with_new_pipeline(&mut self, html: &str, css: Option<&str>) -> Document {
        match css {
            Some(css) => self.parse_with_new_pipeline_css_chunks(html, &[css]),
            None => self.parse_with_new_pipeline_css_chunks(html, &[]),
        }
    }

    /// Complete new pipeline with independently parsed author CSS chunks.
    /// Each chunk is parsed separately so invalid author CSS cannot prevent the
    /// default stylesheet from producing a renderable box tree.
    pub fn parse_with_new_pipeline_css_chunks<'css>(&mut self, html: &str, css_chunks: &[&'css str]) -> Document {
        let parsed_html = parse_html_document(html);
        self.build_pipeline_from_parsed(&parsed_html, css_chunks)
    }

    /// Run the full pipeline (DOM → styles → boxes) from an already-parsed HTML
    /// document. Lets a caller that also needs the title or embedded CSS parse
    /// the HTML only once and share the result across all three.
    pub fn build_pipeline_from_parsed<'css>(&mut self, parsed_html: &Html, css_chunks: &[&'css str]) -> Document {
        use crate::layout::build_box_tree;
        use crate::style::resolver::resolve_styles_for_dom;
        use lightningcss::stylesheet::{ParserOptions, StyleSheet};
        self.document.mode = DocumentMode::Html;
        // Phase 1: Build DOM tree from the parsed HTML
        if let Some(root) = self.select_document_root(parsed_html) {
            let root_idx = self.build_dom_element(root, None);
            self.document.dom_root = Some(root_idx);
        }

        // Phase 2: Resolve styles for DOM elements
        const DEFAULT_CSS: &str = include_str!("../style/default.css");
        let mut stylesheets: Vec<StyleSheet<'css, 'css>> = Vec::new();
        let default_stylesheet = StyleSheet::parse(DEFAULT_CSS, ParserOptions::default()).expect("default CSS must parse");
        stylesheets.push(default_stylesheet);
        for (idx, css) in css_chunks.iter().enumerate() {
            if css.trim().is_empty() {
                continue;
            }
            match StyleSheet::parse(css, ParserOptions::default()) {
                Ok(stylesheet) => stylesheets.push(stylesheet),
                Err(error) => eprintln!("Skipping CSS chunk {idx}: {error}"),
            }
        }
        resolve_styles_for_dom(&mut self.document, &stylesheets);

        // Phase 3: Build box tree from styled DOM
        build_box_tree(&mut self.document);
        self.document.rebuild_document_toc_entries();

        // Return the fully processed document
        mem::take(&mut self.document)
    }

    /// Build a DOM element and its children recursively
    fn build_dom_element(&mut self, elem: ElementRef, parent: Option<u32>) -> u32 {
        let node_idx = self.document.dom_nodes.len() as u32;

        // Intern element info
        let tag_name = elem.value().name();
        let tag = self.document.strings.intern(tag_name);
        let namespace = if elem.value().name.ns.is_empty() { None } else { Some(self.document.strings.intern(elem.value().name.ns.as_ref())) };
        let id = elem.value().attr("id").map(|s| self.document.strings.intern(s));
        let classes = self.document.intern_classes(elem.value().attr("class"));
        let href = elem.value().attr("href").map(|s| self.document.strings.intern(s));
        let image_ref = elem.value().attr("src").or_else(|| elem.value().attr("href")).or_else(|| elem.value().attr("xlink:href"));
        let src = image_ref.map(|s| self.document.strings.intern(s));
        let width = elem.value().attr("width").and_then(|v| v.parse::<u32>().ok());
        let height = elem.value().attr("height").and_then(|v| v.parse::<u32>().ok());
        let image_idx = self.register_image_resource(tag_name, image_ref, width, height);
        let attributes = elem
            .value()
            .attrs
            .iter()
            .map(|(key, value)| {
                let raw_name = match key.prefix.as_ref() {
                    Some(prefix) => format!("{}:{}", prefix.as_ref(), key.local.as_ref()),
                    None => key.local.as_ref().to_string(),
                };
                let namespace = if key.ns.is_empty() { None } else { Some(self.document.strings.intern(key.ns.as_ref())) };

                DomAttribute { name: self.document.strings.intern(&raw_name), local_name: self.document.strings.intern(key.local.as_ref()), namespace, value: self.document.strings.intern(value.as_ref()) }
            })
            .collect();

        // Push element first to reserve node_idx before children are added
        self.document.dom_nodes.push(DomNode::Element(DomElement { tag, id, classes, namespace, parent, children: Vec::new(), href, src, width, height, image_idx, attributes, style: None }));

        // Process children
        let mut children = Vec::new();
        for child in elem.children() {
            match child.value() {
                Node::Element(_) => {
                    if let Some(child_elem) = ElementRef::wrap(child) {
                        let child_idx = self.build_dom_element(child_elem, Some(node_idx));
                        children.push(child_idx);
                    }
                }
                Node::Text(text) => {
                    if !text.is_empty() {
                        let text_idx = self.document.dom_nodes.len() as u32;
                        children.push(text_idx);
                        self.document.dom_nodes.push(DomNode::Text(DomText { text: text.to_string(), parent: Some(node_idx) }));
                    }
                }
                _ => {}
            }
        }

        // Update element with its children
        if let DomNode::Element(ref mut elem) = self.document.dom_nodes[node_idx as usize] {
            elem.children = children;
        }

        self.register_heading_toc(node_idx);

        node_idx
    }

    fn register_heading_toc(&mut self, node_idx: u32) {
        let (tag, existing_id, children) = match self.document.dom_nodes.get(node_idx as usize) {
            Some(DomNode::Element(elem)) => (elem.tag, elem.id, elem.children.clone()),
            _ => return,
        };
        let Some(level) = heading_level(self.document.strings.get(tag)) else {
            return;
        };

        let title = collapse_whitespace(&self.collect_text_content(&children));
        if title.is_empty() {
            return;
        }

        let id_idx = existing_id.unwrap_or_else(|| self.assign_generated_id(node_idx));
        let href = format!("#{}", self.document.strings.get(id_idx));
        let title_idx = self.document.strings.intern(&title);
        let href_idx = self.document.strings.intern(&href);

        self.document.toc_entries.push(DocumentTocEntry { level, title: title_idx, href: href_idx, node_idx });
    }

    fn collect_text_content(&self, children: &[u32]) -> String {
        let mut out = String::new();
        for &child in children {
            self.collect_text_from_node(child, &mut out);
        }
        out
    }

    fn collect_text_from_node(&self, node_idx: u32, out: &mut String) {
        match self.document.dom_nodes.get(node_idx as usize) {
            Some(DomNode::Text(text)) => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&text.text);
            }
            Some(DomNode::Element(elem)) => {
                for &child in &elem.children {
                    self.collect_text_from_node(child, out);
                }
            }
            None => {}
        }
    }

    fn assign_generated_id(&mut self, node_idx: u32) -> u16 {
        let base = format!("heading-{}", node_idx);
        let mut candidate = base.clone();
        let mut suffix = 2u32;

        loop {
            let candidate_idx = self.document.strings.intern(&candidate);
            if !self.id_in_use(candidate_idx) {
                let id_name = self.document.strings.intern("id");
                if let Some(DomNode::Element(elem)) = self.document.dom_nodes.get_mut(node_idx as usize) {
                    elem.id = Some(candidate_idx);
                    elem.attributes.push(DomAttribute { name: id_name, local_name: id_name, namespace: None, value: candidate_idx });
                }
                return candidate_idx;
            }
            candidate = format!("{}-{}", base, suffix);
            suffix += 1;
        }
    }

    fn id_in_use(&self, id_idx: u16) -> bool {
        self.document.dom_nodes.iter().any(|node| matches!(node, DomNode::Element(elem) if elem.id == Some(id_idx)))
    }

    fn register_image_resource(&mut self, tag_name: &str, src: Option<&str>, width: Option<u32>, height: Option<u32>) -> Option<u32> {
        if !matches!(tag_name, "img" | "image") {
            return None;
        }

        let src = src?.trim();
        if src.is_empty() {
            return None;
        }

        let resolved = match (&self.provider, &self.base_uri) {
            (Some(provider), Some(base_uri)) => provider.resolve(base_uri, src),
            _ => src.to_string(),
        };

        let image_idx = self.document.images.len() as u32;
        self.document.images.push(ImageResource { source: ImageSource::Uri(resolved), width: 0, height: 0, width_attr: width, height_attr: height });
        Some(image_idx)
    }

    fn select_document_root<'a>(&self, doc: &'a Html) -> Option<ElementRef<'a>> {
        for selector in ["html", "body", "svg"] {
            let selector = scraper::Selector::parse(selector).ok()?;
            if let Some(root) = doc.select(&selector).next() {
                return Some(root);
            }
        }
        None
    }
}

/// Parse an HTML string into a scraper `Html` document, applying the
/// self-closing-anchor normalization the DOM builder relies on. Callers that
/// need the title, embedded CSS, and DOM should parse once via this helper and
/// share the result rather than re-parsing per concern.
pub fn parse_html_document(html: &str) -> Html {
    let normalized = normalize_self_closing_anchor_tags(html);
    Html::parse_document(&normalized)
}

fn normalize_self_closing_anchor_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < input.len() {
        let Some(rel_start) = input[i..].find("<a") else {
            out.push_str(&input[i..]);
            break;
        };
        let start = i + rel_start;
        out.push_str(&input[i..start]);

        // Only treat real tag starts (`<a` followed by whitespace, `>`, or `/`).
        let after_a = start + 2;
        if after_a > input.len() {
            out.push_str(&input[start..]);
            break;
        }
        let next = input[after_a..].chars().next();
        if !matches!(next, Some(c) if c.is_ascii_whitespace() || c == '>' || c == '/') {
            out.push_str("<a");
            i = after_a;
            continue;
        }

        let Some(rel_end) = input[after_a..].find('>') else {
            out.push_str(&input[start..]);
            break;
        };
        let end = after_a + rel_end;
        let tag_body = &input[after_a..end];
        let trimmed = tag_body.trim_end();

        if trimmed.ends_with('/') {
            // Convert XHTML-style `<a .../>` into explicit open/close tags so
            // the HTML parser does not keep the anchor open across following text.
            let without_slash = trimmed[..trimmed.len() - 1].trim_end();
            out.push_str("<a");
            out.push_str(without_slash);
            out.push_str("></a>");
        } else {
            out.push_str(&input[start..=end]);
        }

        i = end + 1;
    }

    out
}

fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::DocumentFactory;
    use crate::document::DomNode;

    #[test]
    fn keeps_whitespace_only_text_nodes_in_inline_content() {
        let html = "<html><body><span>a</span> <span>b</span></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_to_dom(html);

        let html_idx = document.dom_root.expect("html root should exist");
        let html = match &document.dom_nodes[html_idx as usize] {
            DomNode::Element(e) => e,
            DomNode::Text(_) => panic!("expected html element"),
        };
        let body_idx = html.children.iter().copied().find(|&c| matches!(&document.dom_nodes[c as usize], DomNode::Element(e) if document.strings.get(e.tag) == "body")).expect("body should be a child of html");
        let body = match &document.dom_nodes[body_idx as usize] {
            DomNode::Element(e) => e,
            DomNode::Text(_) => panic!("expected body element"),
        };

        assert_eq!(body.children.len(), 3, "body should keep the space node");

        let middle_text = match &document.dom_nodes[body.children[1] as usize] {
            DomNode::Text(text) => text,
            DomNode::Element(_) => panic!("expected whitespace text node"),
        };

        assert_eq!(middle_text.text, " ");
    }

    #[test]
    fn builds_toc_entries_from_html_headers() {
        let html = "<html><body><h1>Intro</h1><h2 id=\"chapter-a\">Chapter <em>A</em></h2></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_to_dom(html);

        assert_eq!(document.toc_entries.len(), 2);

        let first = &document.toc_entries[0];
        assert_eq!(first.level, 1);
        assert_eq!(document.strings.get(first.title), "Intro");
        assert!(document.strings.get(first.href).starts_with("#heading-"));

        let second = &document.toc_entries[1];
        assert_eq!(second.level, 2);
        assert_eq!(document.strings.get(second.title), "Chapter A");
        assert_eq!(document.strings.get(second.href), "#chapter-a");
    }

    #[test]
    fn handles_self_closing_anchor_without_leaking_link_scope() {
        let html = "<html><body><p><a id=\"p77\"/>After</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_to_dom(html);

        let body_idx = document
            .dom_root
            .and_then(|root| match &document.dom_nodes[root as usize] {
                DomNode::Element(html) => html
                    .children
                    .iter()
                    .copied()
                    .find(|&c| matches!(&document.dom_nodes[c as usize], DomNode::Element(e) if document.strings.get(e.tag) == "body")),
                DomNode::Text(_) => None,
            })
            .expect("body should exist");

        let p_idx = match &document.dom_nodes[body_idx as usize] {
            DomNode::Element(body) => body
                .children
                .iter()
                .copied()
                .find(|&c| matches!(&document.dom_nodes[c as usize], DomNode::Element(e) if document.strings.get(e.tag) == "p"))
                .expect("p should exist"),
            DomNode::Text(_) => panic!("expected body element"),
        };

        let p = match &document.dom_nodes[p_idx as usize] {
            DomNode::Element(p) => p,
            DomNode::Text(_) => panic!("expected p element"),
        };

        assert_eq!(p.children.len(), 2, "self-closed anchor should not wrap following text");
        assert!(matches!(&document.dom_nodes[p.children[0] as usize], DomNode::Element(e) if document.strings.get(e.tag) == "a"));
        assert!(matches!(&document.dom_nodes[p.children[1] as usize], DomNode::Text(t) if t.text == "After"));
    }
}
