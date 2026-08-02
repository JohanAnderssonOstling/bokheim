use html_dom::{Document, DocumentBuildError, DocumentBuilder, DocumentMode, DocumentTocEntry, DomAttribute, DomElementAttributeCache, DomElementData, DomNodeId, ImageResource, ImageSource, NodeRef, Rootless};
use html5ever::driver::{self, ParseOpts};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::{QuirksMode as Html5everQuirksMode, TreeBuilderOpts};
use html5ever::{LocalName, Namespace, QualName};
use scraper::{ElementRef, Html, Node};

pub struct ParsedHtml {
    document: Html,
}

pub struct ParsedHtmlFragment {
    fragment: Html,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HtmlScriptingMode {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtmlParserOptions {
    pub scripting: HtmlScriptingMode,
    pub exact_errors: bool,
}

impl Default for HtmlParserOptions {
    fn default() -> Self {
        Self { scripting: HtmlScriptingMode::Disabled, exact_errors: false }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlNamespace {
    Html,
    MathMl,
    Svg,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HtmlFragmentContext {
    pub namespace: HtmlNamespace,
    pub local_name: String,
}

impl HtmlFragmentContext {
    pub fn html(local_name: impl Into<String>) -> Self {
        Self { namespace: HtmlNamespace::Html, local_name: local_name.into() }
    }

    pub fn svg(local_name: impl Into<String>) -> Self {
        Self { namespace: HtmlNamespace::Svg, local_name: local_name.into() }
    }

    pub fn math_ml(local_name: impl Into<String>) -> Self {
        Self { namespace: HtmlNamespace::MathMl, local_name: local_name.into() }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlQuirksMode {
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HtmlSyntaxTree {
    pub nodes: Vec<HtmlSyntaxNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HtmlSyntaxNode {
    Doctype { name: String, public_id: String, system_id: String },
    Comment(String),
    Text(String),
    Element(HtmlSyntaxElement),
    TemplateContents(Vec<HtmlSyntaxNode>),
    ProcessingInstruction { target: String, data: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HtmlSyntaxElement {
    pub local_name: String,
    pub namespace: Option<String>,
    pub attributes: Vec<HtmlSyntaxAttribute>,
    pub children: Vec<HtmlSyntaxNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HtmlSyntaxAttribute {
    pub name: String,
    pub local_name: String,
    pub namespace: Option<String>,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StylesheetReference {
    Inline(String),
    External(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HeadMetadata {
    pub title: Option<String>,
    pub stylesheets: Vec<StylesheetReference>,
}

pub fn parse_html_document(html: &str) -> ParsedHtml {
    parse_html_document_with_options(html, HtmlParserOptions::default())
}

pub fn parse_html_document_with_options(html: &str, options: HtmlParserOptions) -> ParsedHtml {
    let parser = driver::parse_document(scraper::HtmlTreeSink::new(Html::new_document()), html5ever_options(options));
    ParsedHtml { document: parser.one(normalize_self_closing_anchor_tags(html)) }
}

pub fn parse_html_fragment(html: &str, context: &HtmlFragmentContext, options: HtmlParserOptions) -> ParsedHtmlFragment {
    let context_name = QualName::new(None, context_namespace(context.namespace), LocalName::from(context.local_name.as_str()));
    let parser = driver::parse_fragment(scraper::HtmlTreeSink::new(Html::new_fragment()), html5ever_options(options), context_name, Vec::new(), options.scripting == HtmlScriptingMode::Enabled);
    ParsedHtmlFragment { fragment: parser.one(html) }
}

impl ParsedHtml {
    pub fn build_dom(&self) -> Document {
        self.build_dom_checked().expect("parser produced an invalid DOM")
    }

    pub fn build_dom_checked(&self) -> Result<Document, DocumentBuildError> {
        build_dom_document(self)
    }

    pub fn head_metadata(&self) -> HeadMetadata {
        let mut metadata = HeadMetadata::default();
        let Ok(selector) = scraper::Selector::parse("head title, link, style") else {
            return metadata;
        };
        for element in self.document.select(&selector) {
            match element.value().name() {
                "title" if metadata.title.is_none() => {
                    metadata.title = nonempty(element.text().collect::<String>());
                }
                "style" => {
                    if let Some(css) = nonempty(element.text().collect::<String>()) {
                        metadata.stylesheets.push(StylesheetReference::Inline(css));
                    }
                }
                "link" if link_is_stylesheet(element.value().attr("rel")) && link_type_is_css(element.value().attr("type")) => {
                    if let Some(href) = element.value().attr("href").and_then(|href| nonempty(href.to_owned())) {
                        metadata.stylesheets.push(StylesheetReference::External(href));
                    }
                }
                _ => {}
            }
        }
        metadata
    }

    pub fn errors(&self) -> impl ExactSizeIterator<Item = &str> {
        self.document.errors.iter().map(AsRef::as_ref)
    }

    pub fn quirks_mode(&self) -> HtmlQuirksMode {
        map_quirks_mode(self.document.quirks_mode)
    }

    pub fn syntax_tree(&self) -> HtmlSyntaxTree {
        syntax_tree(&self.document)
    }
}

impl ParsedHtmlFragment {
    pub fn errors(&self) -> impl ExactSizeIterator<Item = &str> {
        self.fragment.errors.iter().map(AsRef::as_ref)
    }

    pub fn syntax_tree(&self) -> HtmlSyntaxTree {
        let mut tree = syntax_tree(&self.fragment);
        if tree.nodes.len() == 1
            && let HtmlSyntaxNode::Element(root) = &mut tree.nodes[0]
            && root.local_name == "html"
            && root.namespace.as_deref() == Some("http://www.w3.org/1999/xhtml")
        {
            return HtmlSyntaxTree { nodes: std::mem::take(&mut root.children) };
        }
        tree
    }
}

fn html5ever_options(options: HtmlParserOptions) -> ParseOpts {
    ParseOpts {
        tokenizer: html5ever::tokenizer::TokenizerOpts { exact_errors: options.exact_errors, ..Default::default() },
        tree_builder: TreeBuilderOpts { exact_errors: options.exact_errors, scripting_enabled: options.scripting == HtmlScriptingMode::Enabled, ..Default::default() },
    }
}

fn context_namespace(namespace: HtmlNamespace) -> Namespace {
    match namespace {
        HtmlNamespace::Html => Namespace::from("http://www.w3.org/1999/xhtml"),
        HtmlNamespace::MathMl => Namespace::from("http://www.w3.org/1998/Math/MathML"),
        HtmlNamespace::Svg => Namespace::from("http://www.w3.org/2000/svg"),
    }
}

fn map_quirks_mode(mode: Html5everQuirksMode) -> HtmlQuirksMode {
    match mode {
        Html5everQuirksMode::NoQuirks => HtmlQuirksMode::NoQuirks,
        Html5everQuirksMode::LimitedQuirks => HtmlQuirksMode::LimitedQuirks,
        Html5everQuirksMode::Quirks => HtmlQuirksMode::Quirks,
    }
}

fn syntax_tree(html: &Html) -> HtmlSyntaxTree {
    HtmlSyntaxTree { nodes: syntax_children(html.tree.root()) }
}

fn syntax_children(parent: ego_tree::NodeRef<'_, Node>) -> Vec<HtmlSyntaxNode> {
    parent.children().filter_map(syntax_node).collect()
}

fn syntax_node(node: ego_tree::NodeRef<'_, Node>) -> Option<HtmlSyntaxNode> {
    match node.value() {
        Node::Document => None,
        Node::Fragment => Some(HtmlSyntaxNode::TemplateContents(syntax_children(node))),
        Node::Doctype(doctype) => Some(HtmlSyntaxNode::Doctype { name: doctype.name().to_owned(), public_id: doctype.public_id().to_owned(), system_id: doctype.system_id().to_owned() }),
        Node::Comment(comment) => Some(HtmlSyntaxNode::Comment(comment.to_string())),
        Node::Text(text) => Some(HtmlSyntaxNode::Text(text.to_string())),
        Node::ProcessingInstruction(pi) => Some(HtmlSyntaxNode::ProcessingInstruction { target: pi.target.to_string(), data: pi.data.to_string() }),
        Node::Element(element) => {
            let attributes = element
                .attrs
                .iter()
                .map(|(name, value)| HtmlSyntaxAttribute {
                    name: name.prefix.as_ref().map(|prefix| format!("{}:{}", prefix.as_ref(), name.local.as_ref())).unwrap_or_else(|| name.local.to_string()),
                    local_name: name.local.to_string(),
                    namespace: (!name.ns.is_empty()).then(|| name.ns.to_string()),
                    value: value.to_string(),
                })
                .collect();
            Some(HtmlSyntaxNode::Element(HtmlSyntaxElement { local_name: element.name.local.to_string(), namespace: (!element.name.ns.is_empty()).then(|| element.name.ns.to_string()), attributes, children: syntax_children(node) }))
        }
    }
}

/// Parse HTML directly into the renderer's framework-neutral DOM.
pub fn parse_dom_document(html: &str) -> Result<Document, DocumentBuildError> {
    build_dom_document(&parse_html_document(html))
}

/// Convert an already parsed scraper tree into the renderer's DOM.
/// Scraper types do not escape beyond this crate boundary.
pub fn build_dom_document(parsed: &ParsedHtml) -> Result<Document, DocumentBuildError> {
    let mut document = DocumentBuilder::new();
    document.set_mode(DocumentMode::Html);
    if let Some(root) = select_document_root(&parsed.document) {
        let root_data = build_dom_element_data(&mut document, &root);
        let (root_node, mut document) = document.append_dom_element(None, root_data)?;
        register_heading_toc(&mut document, root_node);
        build_dom_children(&mut document, root, root_node)?;
        return document.set_dom_root(root_node)?.finish();
    }
    document.finish_without_root()
}

fn nonempty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn link_is_stylesheet(rel: Option<&str>) -> bool {
    rel.is_some_and(|rel| rel.split_ascii_whitespace().any(|token| token.eq_ignore_ascii_case("stylesheet")))
}

fn link_type_is_css(media_type: Option<&str>) -> bool {
    media_type.map(|media_type| media_type.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("text/css")).unwrap_or(true)
}

fn select_document_root<'a>(document: &'a Html) -> Option<ElementRef<'a>> {
    for selector in ["html", "body", "svg"] {
        let selector = scraper::Selector::parse(selector).ok()?;
        if let Some(root) = document.select(&selector).next() {
            return Some(root);
        }
    }
    None
}

fn build_dom_element(document: &mut DocumentBuilder<Rootless>, element: ElementRef<'_>, parent: Option<DomNodeId>) -> Result<DomNodeId, DocumentBuildError> {
    let data = build_dom_element_data(document, &element);
    let node = document.append_dom_element(parent, data)?;
    for child in element.children() {
        match child.value() {
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    build_dom_element(document, child, Some(node))?;
                }
            }
            Node::Text(text) if !text.is_empty() => {
                document.append_dom_text(node, text.to_string())?;
            }
            _ => {}
        }
    }
    register_heading_toc(document, node);
    Ok(node)
}

fn build_dom_children(document: &mut DocumentBuilder<Rootless>, element: ElementRef<'_>, parent: DomNodeId) -> Result<(), DocumentBuildError> {
    for child in element.children() {
        match child.value() {
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    build_dom_element(document, child, Some(parent))?;
                }
            }
            Node::Text(text) if !text.is_empty() => {
                document.append_dom_text(parent, text.to_string())?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn build_dom_element_data<State>(document: &mut DocumentBuilder<State>, element: &ElementRef<'_>) -> DomElementData {
    let tag_name = element.value().name();
    let tag = document.intern_string(tag_name);
    let namespace = (!element.value().name.ns.is_empty()).then(|| document.intern_string(element.value().name.ns.as_ref()));
    let id = element.value().attr("id").map(|value| document.intern_string(value));
    let classes = document.intern_classes(element.value().attr("class"));
    let source = element.value().attr("src").or_else(|| element.value().attr("href")).or_else(|| element.value().attr("xlink:href"));
    let width = element.value().attr("width").and_then(|value| value.parse().ok());
    let height = element.value().attr("height").and_then(|value| value.parse().ok());
    let image_idx = if matches!(tag_name, "img" | "image") {
        source.filter(|source| !source.trim().is_empty()).map(|source| document.push_image(ImageResource { source: ImageSource::Uri(source.trim().to_owned()), width: 0, height: 0, width_attr: width, height_attr: height }))
    } else {
        None
    };
    let attributes = element
        .value()
        .attrs
        .iter()
        .map(|(key, value)| {
            let name = key.prefix.as_ref().map(|prefix| format!("{}:{}", prefix.as_ref(), key.local.as_ref())).unwrap_or_else(|| key.local.as_ref().to_owned());
            DomAttribute {
                name: document.intern_string(&name),
                local_name: document.intern_string(key.local.as_ref()),
                namespace: (!key.ns.is_empty()).then(|| document.intern_string(key.ns.as_ref())),
                value: document.intern_string(value.as_ref()),
            }
        })
        .collect();
    DomElementData { tag, namespace, attributes, attribute_cache: DomElementAttributeCache::new(id, classes), image_idx }
}

fn register_heading_toc(document: &mut DocumentBuilder<Rootless>, node: DomNodeId) {
    let Some(element) = document.document().element_ref(node) else { return };
    let Some(level) = (match element.tag() {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }) else {
        return;
    };
    let existing_id = element.id().map(str::to_owned);
    let children: Vec<_> = element.children().collect();
    let mut title = String::new();
    for child in children {
        collect_text(document.document(), child, &mut title);
    }
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return;
    }
    let id = existing_id.unwrap_or_else(|| assign_generated_id(document, node));
    let title = document.intern_string(&title);
    let href = document.intern_string(&format!("#{id}"));
    document.push_toc_entry(DocumentTocEntry { level, title, href, node_idx: node.raw() });
}

fn collect_text(document: &Document, node: DomNodeId, output: &mut String) {
    match document.node_ref(node) {
        Some(NodeRef::Text(text)) => {
            if !output.is_empty() {
                output.push(' ');
            }
            output.push_str(text.text());
        }
        Some(NodeRef::Element(element)) => {
            for child in element.children() {
                collect_text(document, child, output);
            }
        }
        None => {}
    }
}

fn assign_generated_id<State>(document: &mut DocumentBuilder<State>, node: DomNodeId) -> String {
    let base = format!("heading-{}", node.raw());
    let mut candidate = base.clone();
    let mut suffix = 2;
    loop {
        let value = document.intern_string(&candidate);
        if !document.document().dom_id_in_use(value) {
            let name = document.intern_string("id");
            document.set_dom_id_attribute(node, value, name).expect("generated heading IDs should be valid");
            return candidate;
        }
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
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

        let after_a = start + 2;
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
        let trimmed = input[after_a..end].trim_end();
        if let Some(without_slash) = trimmed.strip_suffix('/') {
            out.push_str("<a");
            out.push_str(without_slash.trim_end());
            out.push_str("></a>");
        } else {
            out.push_str(&input[start..=end]);
        }
        i = end + 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{StylesheetReference, parse_html_document};
    use html_dom::{Document, DomNodeId, ElementRef, NodeRef};
    use scraper::Selector;

    fn child_element<'a>(document: &'a Document, parent: ElementRef<'a>, tag: &str) -> ElementRef<'a> {
        parent.children().find_map(|child| document.element_ref(child).filter(|element| element.tag() == tag)).unwrap_or_else(|| panic!("expected <{tag}> child of <{}>", parent.tag()))
    }

    fn body(document: &Document) -> ElementRef<'_> {
        let html = document.element_ref(document.dom_root().expect("parsed document root")).expect("root element");
        child_element(document, html, "body")
    }

    fn text_content(document: &Document, node: DomNodeId) -> String {
        match document.node_ref(node).expect("valid parsed node") {
            NodeRef::Text(text) => text.text().to_owned(),
            NodeRef::Element(element) => element.children().map(|child| text_content(document, child)).collect(),
        }
    }

    #[test]
    fn normalizes_self_closing_anchors() {
        let document = parse_html_document("<html><body><a id=\"target\"/><p>x</p></body></html>");
        let selector = Selector::parse("a#target").unwrap();
        assert_eq!(document.document.select(&selector).count(), 1);
    }

    #[test]
    fn extracts_head_metadata_without_exposing_scraper() {
        let document = parse_html_document(
            r#"
            <html><head>
                <title> Example </title>
                <link rel="preload StyleSheet" type="Text/CSS; charset=utf-8" href="base.css?v=1">
                <style>p { color: blue; }</style>
            </head><body></body></html>
        "#,
        );
        let metadata = document.head_metadata();
        assert_eq!(metadata.title.as_deref(), Some("Example"));
        assert_eq!(metadata.stylesheets, vec![StylesheetReference::External("base.css?v=1".to_owned()), StylesheetReference::Inline("p { color: blue; }".to_owned()),]);
    }

    #[test]
    fn wpt_block_start_tag_implicitly_closes_open_paragraph() {
        // Adapted from WPT html/syntax/parsing/resources/blocks.dat:
        // https://github.com/web-platform-tests/wpt/blob/master/html/syntax/parsing/resources/blocks.dat
        let document = parse_html_document("<!doctype html><p>foo<div>bar<p>baz").build_dom();
        let body = body(&document);
        let children: Vec<_> = body.children().collect();

        assert_eq!(children.len(), 2);
        assert_eq!(document.get_dom_tag(children[0]), Some("p"));
        assert_eq!(text_content(&document, children[0]), "foo");
        assert_eq!(document.get_dom_tag(children[1]), Some("div"));

        let div = document.element_ref(children[1]).expect("div element");
        let div_children: Vec<_> = div.children().collect();
        assert_eq!(div_children.len(), 2);
        assert!(matches!(document.node_ref(div_children[0]), Some(NodeRef::Text(text)) if text.text() == "bar"));
        assert_eq!(document.get_dom_tag(div_children[1]), Some("p"));
        assert_eq!(text_content(&document, div_children[1]), "baz");
    }

    #[test]
    fn wpt_adoption_agency_reparents_misnested_anchor_content() {
        // Adapted from WPT html/syntax/parsing/resources/adoption01.dat:
        // https://github.com/web-platform-tests/wpt/blob/master/html/syntax/parsing/resources/adoption01.dat
        let document = parse_html_document("<a>1<p>2</a>3</p>").build_dom();
        let body = body(&document);
        let children: Vec<_> = body.children().collect();

        assert_eq!(children.len(), 2);
        assert_eq!(document.get_dom_tag(children[0]), Some("a"));
        assert_eq!(text_content(&document, children[0]), "1");
        assert_eq!(document.get_dom_tag(children[1]), Some("p"));

        let paragraph = document.element_ref(children[1]).expect("paragraph element");
        let paragraph_children: Vec<_> = paragraph.children().collect();
        assert_eq!(paragraph_children.len(), 2);
        assert_eq!(document.get_dom_tag(paragraph_children[0]), Some("a"));
        assert_eq!(text_content(&document, paragraph_children[0]), "2");
        assert!(matches!(document.node_ref(paragraph_children[1]), Some(NodeRef::Text(text)) if text.text() == "3"));
    }

    #[test]
    fn wpt_character_references_follow_html_tokenization_rules() {
        // Representative cases adapted from WPT
        // html/syntax/parsing/resources/entities01.dat:
        // https://github.com/web-platform-tests/wpt/blob/master/html/syntax/parsing/resources/entities01.dat
        let cases = [("FOO&gt;BAR", "FOO>BAR"), ("I'm &notin; I tell you", "I'm ∉ I tell you"), ("FOO&#x41;BAR", "FOOABAR"), ("FOO&#x0080;ZOO", "FOO€ZOO"), ("FOO&#xD800;ZOO", "FOO�ZOO"), ("&ammmp;", "&ammmp;")];

        for (input, expected) in cases {
            let document = parse_html_document(input).build_dom();
            assert_eq!(text_content(&document, body(&document).node_id()), expected, "input {input:?}");
        }
    }
}
