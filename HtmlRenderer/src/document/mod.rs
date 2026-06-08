pub mod cfi;
mod computed_style;
mod dom;
pub use computed_style::*;
pub use dom::*;
use floem::kurbo::{Point, Rect, Size};
use rustc_data_structures::fx::FxHashMap;
use std::ops::{Deref, DerefMut, Range};

const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

#[derive(Clone, Copy)]
pub struct ElementRef<'a> {
    doc: &'a Document,
    node_idx: usize,
}

#[derive(Clone, Copy)]
pub struct TextRef<'a> {
    doc: &'a Document,
    node_idx: usize,
}

#[derive(Clone, Copy)]
pub enum NodeRef<'a> {
    Element(ElementRef<'a>),
    Text(TextRef<'a>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentMode {
    Html,
    Xml,
}

impl Document {
    pub fn new() -> Self {
        let mut doc = Self::default();
        doc.ensure_default_styles();
        doc
    }

    pub fn mode(&self) -> DocumentMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: DocumentMode) {
        self.mode = mode;
    }

    pub fn root_font_size(&self) -> f32 {
        self.root_font_size
    }

    pub fn set_root_font_size(&mut self, root_font_size: f32) {
        self.root_font_size = root_font_size;
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    pub fn intern_string(&mut self, value: &str) -> u16 {
        self.strings.intern(value)
    }

    pub fn string(&self, idx: u16) -> &str {
        self.strings.get(idx)
    }

    pub fn lookup_string(&self, value: &str) -> Option<u16> {
        self.strings.lookup(value)
    }

    pub fn style_view(&self, indices: StyleIndices) -> StyleView<'_> {
        self.styles.view(indices)
    }

    pub fn push_style(&mut self, font: Font, text: InheritedText, box_model: BoxModel, border: Border, background: Background) -> StyleIndices {
        self.styles.push(font, text, box_model, border, background)
    }

    pub fn box_model_style(&self, idx: u32) -> &BoxModel {
        &self.styles.box_styles[idx as usize]
    }

    pub fn text_style(&self, idx: u32) -> &InheritedText {
        &self.styles.text_styles[idx as usize]
    }

    pub fn font_style(&self, idx: u32) -> &Font {
        &self.styles.font_styles[idx as usize]
    }

    pub fn images(&self) -> &[ImageResource] {
        self.images.as_slice()
    }

    pub fn images_mut(&mut self) -> &mut [ImageResource] {
        self.images.as_mut_slice()
    }

    pub fn image(&self, image_idx: u32) -> Option<&ImageResource> {
        self.images.get(image_idx as usize)
    }

    pub fn push_image(&mut self, image: ImageResource) -> u32 {
        let image_idx = self.images.len() as u32;
        self.images.push(image);
        image_idx
    }

    pub fn toc_entries(&self) -> &[DocumentTocEntry] {
        self.toc_entries.as_slice()
    }

    pub fn document_toc_entries(&self) -> &[DocumentTocNode] {
        self.document_toc_entries.as_slice()
    }

    pub fn push_toc_entry(&mut self, entry: DocumentTocEntry) {
        self.toc_entries.push(entry);
    }

    pub fn anchor_glyph(&self, id_idx: u16) -> Option<u32> {
        self.anchor_glyphs.get(&id_idx).copied()
    }

    pub fn anchor_glyphs(&self) -> &FxHashMap<u16, u32> {
        &self.anchor_glyphs
    }

    pub fn clear_anchor_glyphs(&mut self) {
        self.anchor_glyphs.clear();
    }

    pub fn insert_anchor_glyph(&mut self, id_idx: u16, glyph_idx: u32) {
        self.anchor_glyphs.insert(id_idx, glyph_idx);
    }

    pub fn link_href_for_glyph(&self, glyph_idx: u32) -> Option<u16> {
        self.link_glyph_hrefs.get(&glyph_idx).copied()
    }

    pub fn clear_link_glyph_hrefs(&mut self) {
        self.link_glyph_hrefs.clear();
    }

    pub fn insert_link_glyph_href(&mut self, glyph_idx: u32, href_idx: u16) {
        self.link_glyph_hrefs.insert(glyph_idx, href_idx);
    }

    pub fn rebuild_document_toc_entries(&mut self) {
        self.document_toc_entries = build_document_toc_tree(self.toc_entries.as_slice());
    }

    /// Ensure default styles exist at index 0 for fallback
    pub fn ensure_default_styles(&mut self) {
        self.styles.ensure_defaults();
    }

    pub fn print_dom_tree(&self) {
        if let Some(root) = self.dom_root {
            self.print_dom_node(root as usize, 0);
        }
    }

    fn print_dom_node(&self, idx: usize, depth: usize) {
        let indent = "  ".repeat(depth);
        match &self.dom_nodes[idx] {
            DomNode::Element(elem) => {
                let tag = self.strings.get(elem.tag);
                println!("{}<{}>", indent, tag);
                for &child in &elem.children {
                    self.print_dom_node(child as usize, depth + 1);
                }
            }
            DomNode::Text(text) => {
                let truncated: String = text.text.chars().take(80).collect();
                println!("{}\"{}\"", indent, truncated);
            }
        }
    }

    // ========== DOM NODE ACCESSORS ==========

    pub fn element_ref(&self, node_idx: usize) -> Option<ElementRef<'_>> {
        self.dom_nodes.get(node_idx)?.as_element()?;
        Some(ElementRef { doc: self, node_idx })
    }

    pub fn text_ref(&self, node_idx: usize) -> Option<TextRef<'_>> {
        self.dom_nodes.get(node_idx)?.as_text()?;
        Some(TextRef { doc: self, node_idx })
    }

    pub fn node_ref(&self, node_idx: usize) -> Option<NodeRef<'_>> {
        match self.dom_nodes.get(node_idx)? {
            DomNode::Element(_) => Some(NodeRef::Element(ElementRef { doc: self, node_idx })),
            DomNode::Text(_) => Some(NodeRef::Text(TextRef { doc: self, node_idx })),
        }
    }

    pub fn node_count(&self) -> usize {
        self.dom_nodes.len()
    }

    pub fn dom_root(&self) -> Option<usize> {
        self.dom_root.map(|idx| idx as usize)
    }

    pub(crate) fn set_dom_root(&mut self, node_idx: u32) {
        self.dom_root = Some(node_idx);
    }

    pub(crate) fn push_dom_element(&mut self, element: DomElement) -> u32 {
        let node_idx = self.dom_nodes.len() as u32;
        self.dom_nodes.push(DomNode::Element(element));
        node_idx
    }

    pub(crate) fn push_dom_text(&mut self, text: impl Into<String>, parent: Option<u32>) -> u32 {
        let node_idx = self.dom_nodes.len() as u32;
        self.dom_nodes.push(DomNode::Text(DomText { text: text.into(), parent }));
        node_idx
    }

    pub(crate) fn set_dom_children(&mut self, node_idx: u32, children: Vec<u32>) -> bool {
        let Some(DomNode::Element(element)) = self.dom_nodes.get_mut(node_idx as usize) else {
            return false;
        };
        element.children = children;
        true
    }

    pub(crate) fn set_dom_id_attribute(&mut self, node_idx: u32, id_idx: u16, id_name_idx: u16) -> bool {
        let Some(DomNode::Element(element)) = self.dom_nodes.get_mut(node_idx as usize) else {
            return false;
        };
        element.id = Some(id_idx);
        element.attributes.push(DomAttribute { name: id_name_idx, local_name: id_name_idx, namespace: None, value: id_idx });
        true
    }

    pub(crate) fn dom_id_in_use(&self, id_idx: u16) -> bool {
        self.dom_nodes.iter().any(|node| matches!(node, DomNode::Element(element) if element.id == Some(id_idx)))
    }

    pub fn is_text_node(&self, node_idx: usize) -> bool {
        matches!(self.node_ref(node_idx), Some(NodeRef::Text(_)))
    }

    pub fn is_element_node(&self, node_idx: usize) -> bool {
        matches!(self.node_ref(node_idx), Some(NodeRef::Element(_)))
    }

    pub fn dom_style_indices(&self, node_idx: usize) -> Option<StyleIndices> {
        self.dom_nodes.get(node_idx)?.as_element()?.style
    }

    pub fn set_dom_style_indices(&mut self, node_idx: usize, style: Option<StyleIndices>) -> bool {
        let Some(DomNode::Element(element)) = self.dom_nodes.get_mut(node_idx) else {
            return false;
        };
        element.style = style;
        true
    }

    /// Get tag name for a DOM element
    pub fn get_dom_tag(&self, node_idx: usize) -> Option<&str> {
        self.element_ref(node_idx).map(|element| element.tag())
    }

    /// Get ID for a DOM element
    pub fn get_dom_id(&self, node_idx: usize) -> Option<&str> {
        self.element_ref(node_idx).and_then(|element| element.id())
    }

    /// Get classes for a DOM element as iterator
    pub fn get_dom_classes(&self, node_idx: usize) -> Box<dyn Iterator<Item = &str> + '_> {
        if let Some(element) = self.element_ref(node_idx) { Box::new(element.classes()) } else { Box::new(std::iter::empty()) }
    }

    /// Check if DOM element has a specific class
    pub fn dom_has_class(&self, node_idx: usize, class: &str) -> bool {
        self.element_ref(node_idx).is_some_and(|element| element.has_class(class))
    }

    /// Get parent DOM node index
    pub fn get_dom_parent(&self, node_idx: usize) -> Option<usize> {
        self.node_ref(node_idx)?.parent()
    }

    pub fn get_dom_namespace(&self, node_idx: usize) -> Option<&str> {
        self.element_ref(node_idx).and_then(|element| element.namespace())
    }

    pub fn is_html_element_in_html_document(&self, node_idx: usize) -> bool {
        self.element_ref(node_idx).is_some_and(|element| element.is_html_element_in_html_document())
    }

    /// Iterate DOM ancestors (parent, grandparent, etc.)
    pub fn dom_ancestors(&self, node_idx: usize) -> DomAncestorIter<'_> {
        DomAncestorIter { doc: self, current: self.dom_nodes.get(node_idx).and_then(|n| n.parent()) }
    }

    /// Get href attribute for DOM element
    pub fn get_dom_href(&self, node_idx: usize) -> Option<&str> {
        self.element_ref(node_idx).and_then(|element| element.href())
    }

    pub fn get_dom_attr(&self, node_idx: usize, attr: &str) -> Option<&str> {
        self.element_ref(node_idx).and_then(|element| element.attr(attr))
    }

    pub fn has_dom_attr(&self, node_idx: usize, attr: &str) -> bool {
        self.element_ref(node_idx).is_some_and(|element| element.has_attr(attr))
    }

    pub fn get_dom_attr_expanded(&self, node_idx: usize, namespace: Option<&str>, local_name: &str) -> Option<&str> {
        self.element_ref(node_idx).and_then(|element| element.attr_expanded(namespace, local_name))
    }

    pub fn has_dom_attr_expanded(&self, node_idx: usize, namespace: Option<&str>, local_name: &str) -> bool {
        self.element_ref(node_idx).is_some_and(|element| element.has_attr_expanded(namespace, local_name))
    }

    pub fn get_dom_attr_in_any_namespace(&self, node_idx: usize, local_name: &str) -> Option<&str> {
        self.element_ref(node_idx).and_then(|element| element.attr_in_any_namespace(local_name))
    }

    pub fn has_dom_attr_in_any_namespace(&self, node_idx: usize, local_name: &str) -> bool {
        self.element_ref(node_idx).is_some_and(|element| element.has_attr_in_any_namespace(local_name))
    }

    // ========== CFI HELPER FUNCTIONS ==========

    /// Find which child index this node is in its parent's children list
    pub fn get_child_index(&self, node_idx: u32) -> Option<usize> {
        let parent_idx = self.dom_nodes.get(node_idx as usize)?.parent()?;
        let parent = self.dom_nodes.get(parent_idx as usize)?.as_element()?;
        parent.children.iter().position(|&child| child == node_idx)
    }

    /// Get the CFI step number for this node (counting only element siblings before it)
    /// CFI uses step indirection: step = (element_index + 1) * 2
    pub fn get_element_step(&self, node_idx: u32) -> Option<usize> {
        let parent_idx = self.dom_nodes.get(node_idx as usize)?.parent()?;
        let parent = self.dom_nodes.get(parent_idx as usize)?.as_element()?;

        let mut element_count = 0;
        for &child_idx in &parent.children {
            if child_idx == node_idx {
                // CFI step indirection: (element_index + 1) * 2
                return Some((element_count + 1) * 2);
            }
            // Only count element nodes, not text nodes
            if self.dom_nodes.get(child_idx as usize)?.as_element().is_some() {
                element_count += 1;
            }
        }
        None
    }

    /// Returns the CFI odd step for a text node within its parent element.
    /// Step = 2 * (number of preceding element siblings) + 1.
    pub fn get_text_node_step(&self, text_node_idx: u32) -> Option<usize> {
        let parent_idx = self.dom_nodes.get(text_node_idx as usize)?.parent()?;
        let parent = self.dom_nodes.get(parent_idx as usize)?.as_element()?;
        let mut elem_count = 0;
        for &child_idx in &parent.children {
            if child_idx == text_node_idx {
                return Some(elem_count * 2 + 1);
            }
            if self.dom_nodes.get(child_idx as usize)?.as_element().is_some() {
                elem_count += 1;
            }
        }
        None
    }

    /// Finds the text node at the given CFI odd step within a parent element.
    pub fn find_text_node_by_step(&self, parent_idx: u32, text_step: usize) -> Option<u32> {
        let target_gap = (text_step - 1) / 2;
        let parent = self.dom_nodes.get(parent_idx as usize)?.as_element()?;
        let mut elem_count = 0;
        for &child_idx in &parent.children {
            match self.dom_nodes.get(child_idx as usize)? {
                DomNode::Element(_) => elem_count += 1,
                DomNode::Text(_) => {
                    if elem_count == target_gap {
                        return Some(child_idx);
                    }
                }
            }
        }
        None
    }

    /// Get all DOM nodes in the path from root to the given node
    /// Returns nodes in order from root to target
    pub fn get_dom_path_to_node(&self, node_idx: u32) -> Vec<u32> {
        let mut path = Vec::new();
        let mut current = Some(node_idx);

        // Build path from node to root
        while let Some(idx) = current {
            path.push(idx);
            current = self.dom_nodes.get(idx as usize).and_then(|n| n.parent());
        }

        // Reverse to get root-to-node order
        path.reverse();
        path
    }

    /// Find a DOM node given a CFI path of steps
    /// Steps are in CFI format (step indirection: step/2 - 1 = element_index)
    pub fn find_dom_node_by_steps(&self, steps: &[usize]) -> Option<u32> {
        let mut current = self.dom_root?;

        for &step in steps {
            let element = self.dom_nodes.get(current as usize)?.as_element()?;

            // Convert CFI step to element index: (step / 2) - 1
            if step % 2 != 0 {
                // Odd steps point to text nodes, which we don't navigate through
                return None;
            }
            let target_element_index = (step / 2) - 1;

            // Count element children to find the target
            let mut element_count = 0;
            let mut found = None;
            for &child_idx in &element.children {
                if self.dom_nodes.get(child_idx as usize)?.as_element().is_some() {
                    if element_count == target_element_index {
                        found = Some(child_idx);
                        break;
                    }
                    element_count += 1;
                }
            }

            current = found?;
        }

        Some(current)
    }

    /// Add element with classes, returns (class_range_start, class_range_end)
    pub fn intern_classes(&mut self, classes: Option<&str>) -> Range<u16> {
        let start = self.class_list.len() as u16;
        if let Some(classes) = classes {
            for class in classes.split_whitespace() {
                let idx = self.strings.intern(class);
                self.class_list.push(idx);
            }
        }
        let end = self.class_list.len() as u16;
        start..end
    }
}

impl<'a> ElementRef<'a> {
    fn element(self) -> &'a DomElement {
        self.doc.dom_nodes[self.node_idx].as_element().expect("ElementRef must point to a DOM element")
    }

    pub fn node_idx(self) -> usize {
        self.node_idx
    }

    pub fn tag(self) -> &'a str {
        self.doc.strings.get(self.element().tag)
    }

    pub fn id(self) -> Option<&'a str> {
        self.element().id.map(|idx| self.doc.strings.get(idx))
    }

    pub fn classes(self) -> impl Iterator<Item = &'a str> + 'a {
        let range = self.element().classes.clone();
        self.doc.class_list[range.start as usize..range.end as usize].iter().map(|&idx| self.doc.strings.get(idx))
    }

    pub fn has_class(self, class: &str) -> bool {
        self.classes().any(|candidate| candidate == class)
    }

    pub fn parent(self) -> Option<usize> {
        self.element().parent.map(|idx| idx as usize)
    }

    pub fn children(self) -> impl Iterator<Item = usize> + 'a {
        self.element().children.iter().map(|&idx| idx as usize)
    }

    pub fn namespace(self) -> Option<&'a str> {
        self.element().namespace.map(|idx| self.doc.strings.get(idx))
    }

    pub fn is_html_element_in_html_document(self) -> bool {
        self.doc.mode == DocumentMode::Html && self.namespace().is_some_and(|namespace| namespace == HTML_NAMESPACE)
    }

    pub fn href(self) -> Option<&'a str> {
        self.element().href.map(|idx| self.doc.strings.get(idx))
    }

    pub fn attr(self, attr: &str) -> Option<&'a str> {
        self.element().attributes.iter().find(|entry| entry.namespace.is_none() && self.doc.strings.get(entry.name) == attr).map(|entry| self.doc.strings.get(entry.value))
    }

    pub fn has_attr(self, attr: &str) -> bool {
        self.attr(attr).is_some()
    }

    pub fn attr_expanded(self, namespace: Option<&str>, local_name: &str) -> Option<&'a str> {
        self.element()
            .attributes
            .iter()
            .find(|entry| {
                self.doc.strings.get(entry.local_name) == local_name
                    && match (namespace, entry.namespace) {
                        (None, None) => true,
                        (Some(expected), Some(actual)) => self.doc.strings.get(actual) == expected,
                        _ => false,
                    }
            })
            .map(|entry| self.doc.strings.get(entry.value))
    }

    pub fn has_attr_expanded(self, namespace: Option<&str>, local_name: &str) -> bool {
        self.attr_expanded(namespace, local_name).is_some()
    }

    pub fn attr_in_any_namespace(self, local_name: &str) -> Option<&'a str> {
        self.element().attributes.iter().find(|entry| self.doc.strings.get(entry.local_name) == local_name).map(|entry| self.doc.strings.get(entry.value))
    }

    pub fn has_attr_in_any_namespace(self, local_name: &str) -> bool {
        self.attr_in_any_namespace(local_name).is_some()
    }

    pub fn style(self) -> Option<StyleIndices> {
        self.doc.dom_style_indices(self.node_idx)
    }

    pub fn image_idx(self) -> Option<u32> {
        self.element().image_idx
    }

    pub fn previous_element_sibling(self) -> Option<ElementRef<'a>> {
        let parent = self.parent().and_then(|parent_idx| self.doc.element_ref(parent_idx))?;
        let mut previous = None;
        for child_idx in parent.children() {
            if child_idx == self.node_idx {
                return previous;
            }
            if let Some(child) = self.doc.element_ref(child_idx) {
                previous = Some(child);
            }
        }
        None
    }
}

impl<'a> NodeRef<'a> {
    pub fn node_idx(self) -> usize {
        match self {
            NodeRef::Element(element) => element.node_idx(),
            NodeRef::Text(text) => text.node_idx(),
        }
    }

    pub fn parent(self) -> Option<usize> {
        match self {
            NodeRef::Element(element) => element.parent(),
            NodeRef::Text(text) => text.parent(),
        }
    }
}

impl<'a> TextRef<'a> {
    fn text_node(self) -> &'a DomText {
        self.doc.dom_nodes[self.node_idx].as_text().expect("TextRef must point to a DOM text node")
    }

    pub fn node_idx(self) -> usize {
        self.node_idx
    }

    pub fn text(self) -> &'a str {
        &self.text_node().text
    }

    pub fn parent(self) -> Option<usize> {
        self.text_node().parent.map(|idx| idx as usize)
    }
}

pub struct Document {
    // ========== DOM TREE (Phase 1: Parsing) ==========
    dom_nodes: Vec<DomNode>,
    dom_root: Option<u32>,
    mode: DocumentMode,

    // ========== RESOURCES ==========
    images: Vec<ImageResource>,
    title: Option<String>,

    // ========== INTERNING ==========
    strings: StringInterner,
    class_list: Vec<u16>, // interned class indices

    // ========== LINKS & ANCHORS ==========
    link_glyph_hrefs: FxHashMap<u32, u16>,
    anchor_glyphs: FxHashMap<u16, u32>,
    toc_entries: Vec<DocumentTocEntry>,
    document_toc_entries: Vec<DocumentTocNode>,

    // ========== STYLES (Phase 2: Style Resolution) ==========
    root_font_size: f32,

    // Computed styles (Servo-inspired split design), indexed by StyleIndices.
    styles: StyleStore,
}

/// Output of the layout phase: line boxes, glyph offsets, and paint fragments.
/// Produced by `LayoutEngine` and consumed read-only by the renderer.
#[derive(Default)]
pub struct LayoutCache {
    pub lines: Vec<Line>,
    pub line_glyph_offsets: Vec<Vec<GlyphOffsetRun>>,
    pub decorations: Vec<DecorationFragment>,
    pub image_fragments: Vec<ImageFragment>,
    pub image_fragments_by_line: Vec<Vec<usize>>,
    pub anchor_positions: FxHashMap<u16, AnchorPosition>,
}

#[derive(Default)]
pub struct DocumentState {
    document: Document,
    box_tree: BoxTree,
    layout: LayoutCache,
}

impl Deref for DocumentState {
    type Target = Document;

    fn deref(&self) -> &Self::Target {
        &self.document
    }
}

impl DerefMut for DocumentState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.document
    }
}

impl DocumentState {
    pub fn new(document: Document, box_tree: BoxTree) -> Self {
        Self { document, box_tree, layout: Default::default() }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    pub fn box_tree(&self) -> &BoxTree {
        &self.box_tree
    }

    pub fn box_tree_mut(&mut self) -> &mut BoxTree {
        &mut self.box_tree
    }

    pub fn set_box_tree(&mut self, box_tree: BoxTree) {
        self.box_tree = box_tree;
    }

    pub fn box_at(&self, box_idx: usize) -> Option<&LayoutBox> {
        self.box_tree.box_at(box_idx)
    }

    pub fn box_at_mut(&mut self, box_idx: usize) -> Option<&mut LayoutBox> {
        self.box_tree.box_at_mut(box_idx)
    }

    pub fn glyphs(&self) -> &[u16] {
        self.box_tree.glyphs()
    }

    pub fn glyphs_mut(&mut self) -> &mut [u16] {
        self.box_tree.glyphs_mut()
    }

    pub fn glyph_at(&self, glyph_idx: usize) -> Option<u16> {
        self.box_tree.glyph_at(glyph_idx)
    }

    pub fn glyph_slice(&self, range: Range<u32>) -> &[u16] {
        self.box_tree.glyph_slice(range)
    }

    /// Returns (text_node_idx, char_offset) for the glyph.
    pub fn get_dom_node_for_glyph(&self, glyph_idx: u32) -> Option<(u32, usize)> {
        for run in self.box_tree.inline_runs() {
            if let InlineKind::Text { glyphs } = &run.kind {
                if glyphs.contains(&glyph_idx) {
                    let node_idx = run.dom_text_node?;
                    let char_offset = (glyph_idx - glyphs.start) as usize;
                    return Some((node_idx, char_offset));
                }
            }
        }
        None
    }

    pub fn box_element(&self, box_idx: usize) -> Option<&DomElement> {
        self.box_tree.box_at(box_idx)?.get_element(&self.document)
    }

    pub fn box_dom_element_idx(&self, box_idx: usize) -> Option<u32> {
        self.box_tree.box_at(box_idx)?.dom_element()
    }

    pub fn get_tag(&self, box_idx: usize) -> &str {
        self.box_element(box_idx).map(|e| self.document.strings.get(e.tag)).unwrap_or("")
    }

    pub fn get_id(&self, box_idx: usize) -> Option<&str> {
        self.get_box_id(box_idx).map(|idx| self.document.strings.get(idx))
    }

    pub fn get_box_id(&self, box_idx: usize) -> Option<u16> {
        self.box_element(box_idx).and_then(|e| e.id)
    }

    pub fn get_box_href(&self, box_idx: usize) -> Option<u16> {
        self.box_element(box_idx).and_then(|e| e.href)
    }

    pub fn get_box_image_idx(&self, box_idx: usize) -> Option<u32> {
        self.box_tree.box_at(box_idx)?.dom_element().and_then(|node_idx| self.document.element_ref(node_idx as usize)?.image_idx())
    }

    pub fn box_layout_mode(&self, box_idx: usize) -> Option<&LayoutMode> {
        self.box_tree.box_at(box_idx).map(|layout_box| layout_box.layout_mode())
    }

    pub fn box_style_indices(&self, box_idx: usize) -> Option<StyleIndices> {
        self.box_tree.box_at(box_idx)?.style()
    }

    pub fn root_box(&self) -> Option<usize> {
        self.box_tree.root_box()
    }

    pub fn box_count(&self) -> usize {
        self.box_tree.box_count()
    }

    pub fn is_table_cell_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::TableCell(_)))
    }

    pub fn is_block_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Block(_)))
    }

    pub fn is_anonymous_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Anonymous(_)))
    }

    pub fn is_table_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Table(_)))
    }

    pub fn is_block_container_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Block(_) | LayoutMode::Table(_) | LayoutMode::TableCell(_)))
    }

    pub fn is_inline_box(&self, box_idx: usize) -> bool {
        matches!(self.box_layout_mode(box_idx), Some(LayoutMode::Inline(_)))
    }

    pub fn box_inline_run_range(&self, box_idx: usize) -> Option<Range<u32>> {
        match self.box_layout_mode(box_idx)? {
            LayoutMode::Inline(range) => Some(range.clone()),
            _ => None,
        }
    }

    pub fn box_anchor_y_candidate(&self, box_idx: usize) -> Option<f64> {
        let layout_box = self.box_tree.box_at(box_idx)?;
        (layout_box.size().height > 0.0 || layout_box.point().y != 0.0).then_some(layout_box.point().y)
    }

    pub fn get_box_point(&self, box_idx: usize) -> Point {
        self.box_tree.get_box_point(box_idx)
    }

    pub fn get_box_size(&self, box_idx: usize) -> Size {
        self.box_tree.get_box_size(box_idx)
    }

    pub fn set_box_point(&mut self, box_idx: usize, point: Point) {
        self.box_tree.set_box_point(box_idx, point);
    }

    pub fn set_box_size(&mut self, box_idx: usize, size: Size) {
        self.box_tree.set_box_size(box_idx, size);
    }

    pub fn inline_runs_empty(&self) -> bool {
        self.box_tree.inline_runs_empty()
    }

    pub fn inline_runs(&self) -> &[InlineRun] {
        self.box_tree.inline_runs()
    }

    pub fn inline_runs_mut(&mut self) -> &mut [InlineRun] {
        self.box_tree.inline_runs_mut()
    }

    pub fn inline_run(&self, run_idx: usize) -> Option<&InlineRun> {
        self.box_tree.inline_run(run_idx)
    }

    pub fn glyph(&self, glyph_idx: u32) -> Option<u16> {
        self.box_tree.glyph_at(glyph_idx as usize)
    }

    pub fn text_runs(&self) -> Vec<(usize, Range<u32>)> {
        self.box_tree
            .inline_runs
            .iter()
            .filter_map(|run| match &run.kind {
                InlineKind::Text { glyphs } if glyphs.start < glyphs.end => Some((run.box_idx as usize, glyphs.clone())),
                _ => None,
            })
            .collect()
    }

    pub fn get_classes(&self, box_idx: usize) -> impl Iterator<Item = &str> {
        let range = self.box_element(box_idx).map(|e| e.classes.clone()).unwrap_or(0..0);
        self.document.class_list[range.start as usize..range.end as usize].iter().map(|&idx| self.document.strings.get(idx))
    }

    pub fn has_class(&self, box_idx: usize, class: &str) -> bool {
        self.get_classes(box_idx).any(|c| c == class)
    }

    pub fn get_parent(&self, box_idx: usize) -> Option<usize> {
        self.box_tree.box_at(box_idx)?.parent().map(|p| p as usize)
    }

    pub fn ancestors(&self, box_idx: usize) -> AncestorIter<'_> {
        AncestorIter { doc: self, current: self.box_tree.box_at(box_idx).and_then(|b| b.parent()) }
    }

    pub fn print_box_tree(&self) {
        self.box_tree.print_box_tree(&self.document);
    }

    pub fn lines(&self) -> &[Line] {
        self.layout.lines.as_slice()
    }

    pub fn line(&self, idx: usize) -> Option<&Line> {
        self.layout.lines.get(idx)
    }

    pub fn line_count(&self) -> usize {
        self.layout.lines.len()
    }

    pub fn line_glyph_offsets(&self) -> &[Vec<GlyphOffsetRun>] {
        self.layout.line_glyph_offsets.as_slice()
    }

    pub fn decorations(&self) -> &[DecorationFragment] {
        self.layout.decorations.as_slice()
    }

    pub fn decorations_mut(&mut self) -> &mut [DecorationFragment] {
        self.layout.decorations.as_mut_slice()
    }

    pub fn image_fragments(&self) -> &[ImageFragment] {
        self.layout.image_fragments.as_slice()
    }

    pub fn image_fragments_by_line(&self) -> &[Vec<usize>] {
        self.layout.image_fragments_by_line.as_slice()
    }

    pub fn anchor_positions(&self) -> &FxHashMap<u16, AnchorPosition> {
        &self.layout.anchor_positions
    }

    pub fn anchor_positions_mut(&mut self) -> &mut FxHashMap<u16, AnchorPosition> {
        &mut self.layout.anchor_positions
    }

    pub fn layout(&self) -> &LayoutCache {
        &self.layout
    }

    pub fn layout_mut(&mut self) -> &mut LayoutCache {
        &mut self.layout
    }

    pub fn set_layout_cache(&mut self, layout: LayoutCache) {
        self.layout = layout;
    }

    pub fn take_layout_cache(&mut self) -> LayoutCache {
        std::mem::take(&mut self.layout)
    }
}

impl BoxTree {
    pub fn box_count(&self) -> usize {
        self.boxes.len()
    }

    pub fn box_at(&self, box_idx: usize) -> Option<&LayoutBox> {
        self.boxes.get(box_idx)
    }

    pub fn box_at_mut(&mut self, box_idx: usize) -> Option<&mut LayoutBox> {
        self.boxes.get_mut(box_idx)
    }

    pub fn root_box(&self) -> Option<usize> {
        self.root_box.map(|idx| idx as usize)
    }

    pub fn set_root_box(&mut self, root_box: Option<u32>) {
        self.root_box = root_box;
    }

    pub fn glyphs(&self) -> &[u16] {
        self.glyphs.as_slice()
    }

    pub fn glyphs_mut(&mut self) -> &mut [u16] {
        self.glyphs.as_mut_slice()
    }

    pub fn glyph_at(&self, glyph_idx: usize) -> Option<u16> {
        self.glyphs.get(glyph_idx).copied()
    }

    pub fn glyph_slice(&self, range: Range<u32>) -> &[u16] {
        &self.glyphs[range.start as usize..range.end as usize]
    }

    pub fn inline_runs(&self) -> &[InlineRun] {
        self.inline_runs.as_slice()
    }

    pub fn inline_runs_mut(&mut self) -> &mut [InlineRun] {
        self.inline_runs.as_mut_slice()
    }

    pub fn inline_run(&self, run_idx: usize) -> Option<&InlineRun> {
        self.inline_runs.get(run_idx)
    }

    pub fn inline_runs_empty(&self) -> bool {
        self.inline_runs.is_empty()
    }

    pub fn set_box_point(&mut self, box_idx: usize, point: Point) {
        if let Some(layout_box) = self.boxes.get_mut(box_idx) {
            layout_box.set_point(point);
        }
    }

    pub fn set_box_size(&mut self, box_idx: usize, size: Size) {
        if let Some(layout_box) = self.boxes.get_mut(box_idx) {
            layout_box.set_size(size);
        }
    }

    pub fn get_box_point(&self, box_idx: usize) -> Point {
        self.boxes[box_idx].point()
    }

    pub fn get_box_size(&self, box_idx: usize) -> Size {
        self.boxes[box_idx].size()
    }

    pub fn get_box_style_indices(&self, box_idx: usize) -> Option<StyleIndices> {
        self.boxes.get(box_idx)?.style()
    }

    pub fn get_box_dom_element(&self, box_idx: usize) -> Option<u32> {
        self.boxes.get(box_idx)?.dom_element()
    }

    pub fn get_box_parent(&self, box_idx: usize) -> Option<usize> {
        self.boxes.get(box_idx)?.parent().map(|p| p as usize)
    }

    pub fn get_box_layout_mode(&self, box_idx: usize) -> Option<&LayoutMode> {
        self.boxes.get(box_idx).map(|layout_box| layout_box.layout_mode())
    }

    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    pub fn print_box_tree(&self, doc: &Document) {
        if !self.boxes.is_empty() {
            self.print_box_node(doc, 0, 0);
        }
    }

    fn print_box_node(&self, doc: &Document, idx: usize, depth: usize) {
        let indent = "  ".repeat(depth);
        let b = &self.boxes[idx];
        let tag = b.get_element(doc).map(|e| doc.strings.get(e.tag)).unwrap_or("");
        let dom_id = b.dom_element().map(|i| i.to_string()).unwrap_or("-".into());
        let display = b.style().map(|si| format!("{:?}", doc.styles.box_styles[si.box_idx as usize].display)).unwrap_or("none".into());
        let mode = match &b.layout_mode {
            LayoutMode::Block(bb) => format!("Block({:?})", bb.children),
            LayoutMode::Table(tb) => format!("Table(rows={})", tb.rows.len()),
            LayoutMode::TableRow(tr) => format!("TableRow(cells={})", tr.cells.len()),
            LayoutMode::TableCell(tc) => format!("TableCell({:?})", tc.children),
            LayoutMode::Inline(r) => format!("Inline({}..{})", r.start, r.end),
            LayoutMode::Anonymous(r) => format!("Anon({}..{})", r.start, r.end),
        };
        println!("{}[{}] <{}> dom={} display={} {}", indent, idx, tag, dom_id, display, mode);
        match &b.layout_mode {
            LayoutMode::Block(bb) => {
                if let Children::Blocks(indices) = &bb.children {
                    for &i in indices {
                        self.print_box_node(doc, i as usize, depth + 1);
                    }
                }
            }
            LayoutMode::Table(tb) => {
                for &i in &tb.rows {
                    self.print_box_node(doc, i as usize, depth + 1);
                }
            }
            LayoutMode::TableRow(tr) => {
                for &i in &tr.cells {
                    self.print_box_node(doc, i as usize, depth + 1);
                }
            }
            _ => {}
        }
    }

    pub fn push_box(&mut self, layout_box: LayoutBox) -> u32 {
        let idx = self.boxes.len() as u32;
        self.boxes.push(layout_box);
        idx
    }

    pub fn push_inline_run(&mut self, run: InlineRun) {
        self.inline_runs.push(run);
    }

    pub fn push_glyph(&mut self, glyph: u16) {
        self.glyphs.push(glyph);
    }
}

#[derive(Clone, Copy)]
pub struct AnchorPosition {
    pub y: f64,
    pub order: u32,
}

#[derive(Clone, Debug)]
pub struct DocumentTocEntry {
    pub level: u8,
    pub title: u16,
    pub href: u16,
    pub node_idx: u32,
}

#[derive(Clone, Debug)]
pub struct DocumentTocNode {
    pub level: u8,
    pub title: u16,
    pub href: u16,
    pub node_idx: u32,
    pub children: Vec<DocumentTocNode>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            dom_nodes: Vec::new(),
            dom_root: None,
            mode: DocumentMode::Html,
            images: Vec::new(),
            title: None,
            strings: StringInterner::new(),
            class_list: Vec::new(),
            link_glyph_hrefs: FxHashMap::default(),
            anchor_glyphs: FxHashMap::default(),
            toc_entries: Vec::new(),
            document_toc_entries: Vec::new(),
            root_font_size: 16.0,
            styles: StyleStore::default(),
        }
    }
}

fn build_document_toc_tree(entries: &[DocumentTocEntry]) -> Vec<DocumentTocNode> {
    fn node_mut<'a>(roots: &'a mut Vec<DocumentTocNode>, path: &[usize]) -> &'a mut DocumentTocNode {
        let mut node = &mut roots[path[0]];
        for &idx in &path[1..] {
            node = &mut node.children[idx];
        }
        node
    }

    let mut roots: Vec<DocumentTocNode> = Vec::new();
    let mut path: Vec<usize> = Vec::new();

    for entry in entries {
        let parent_depth = usize::from(entry.level.saturating_sub(1));
        while path.len() > parent_depth {
            path.pop();
        }

        let node = DocumentTocNode { level: entry.level, title: entry.title, href: entry.href, node_idx: entry.node_idx, children: Vec::new() };

        if path.is_empty() {
            roots.push(node);
            path.push(roots.len() - 1);
        } else {
            let parent = node_mut(&mut roots, &path);
            parent.children.push(node);
            let idx = parent.children.len() - 1;
            path.push(idx);
        }
    }

    roots
}

// ============================================================================
// Ancestor Iterators
// ============================================================================

pub struct AncestorIter<'a> {
    doc: &'a DocumentState,
    current: Option<u32>,
}

impl<'a> Iterator for AncestorIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.current?;
        self.current = self.doc.box_tree.box_at(idx as usize).and_then(|b| b.parent());
        Some(idx as usize)
    }
}

pub struct DomAncestorIter<'a> {
    doc: &'a Document,
    current: Option<u32>,
}

impl<'a> Iterator for DomAncestorIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.current?;
        self.current = self.doc.dom_nodes.get(idx as usize).and_then(|n| n.parent());
        Some(idx as usize)
    }
}

/// Output of the box-construction phase: the box tree plus the inline text
/// content it references (glyph codepoints + inline runs). Produced by
/// `build_box_tree`, consumed read-only by shaping and layout.
#[derive(Default)]
pub struct BoxTree {
    boxes: Vec<LayoutBox>,
    root_box: Option<u32>,
    glyphs: Vec<u16>,
    inline_runs: Vec<InlineRun>,
}

pub struct LayoutBox {
    // Layout data
    pub size: Size,
    pub point: Point,
    pub layout_mode: LayoutMode,

    // DOM node backing this box (None for anonymous boxes). Element identity
    // (tag/id/classes/href) is read through this rather than duplicated here.
    pub dom_element: Option<u32>,

    // Tree structure (layout tree)
    pub parent: Option<u32>, // index of parent box

    // Style indices into split style vecs
    pub style: Option<StyleIndices>,
}

// Type alias retained for readability at layout-mode match sites.
pub type BoxType = LayoutMode;

impl LayoutBox {
    pub fn new_anonymous_box(range: Range<u32>, parent: Option<u32>) -> Self {
        Self {
            size: Size::ZERO,
            point: Point::ZERO,
            layout_mode: LayoutMode::Anonymous(range),
            dom_element: None, // anonymous boxes have no DOM element
            parent,
            style: None,
        }
    }

    /// Get element data from DOM tree
    pub fn get_element<'a>(&self, doc: &'a Document) -> Option<&'a DomElement> {
        self.dom_element.and_then(|idx| doc.dom_nodes[idx as usize].as_element())
    }

    /// Get DOM node (element or text)
    pub fn get_dom_node<'a>(&self, doc: &'a Document) -> Option<&'a DomNode> {
        self.dom_element.map(|idx| &doc.dom_nodes[idx as usize])
    }

    pub fn point(&self) -> Point {
        self.point
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn set_point(&mut self, point: Point) {
        self.point = point;
    }

    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    pub fn layout_mode(&self) -> &LayoutMode {
        &self.layout_mode
    }

    pub fn layout_mode_mut(&mut self) -> &mut LayoutMode {
        &mut self.layout_mode
    }

    pub fn style(&self) -> Option<StyleIndices> {
        self.style
    }

    pub fn set_style(&mut self, style: Option<StyleIndices>) {
        self.style = style;
    }

    pub fn dom_element(&self) -> Option<u32> {
        self.dom_element
    }

    pub fn set_dom_element(&mut self, dom_element: Option<u32>) {
        self.dom_element = dom_element;
    }

    pub fn parent(&self) -> Option<u32> {
        self.parent
    }

    pub fn set_parent(&mut self, parent: Option<u32>) {
        self.parent = parent;
    }
}

pub enum LayoutMode {
    Block(BlockBox),
    Table(TableBox),
    TableRow(TableRowBox),
    TableCell(TableCellBox),
    Inline(Range<u32>), // range of inline runs
    Anonymous(Range<u32>), // range of inline runs (no element)
                        // Future layout modes (not yet implemented)
                        // Flex(FlexBox),
                        // Grid(GridBox),
                        // InlineBlock(BlockBox),
                        // Replaced(ReplacedBox),
}

pub struct BlockBox {
    pub children: Children,
}

pub struct TableBox {
    pub rows: Vec<u32>,
    pub captions_top: Vec<u32>,
    pub captions_bottom: Vec<u32>,
    pub column_width_hints: Vec<PreferredSize>,
}

pub struct TableRowBox {
    pub cells: Vec<u32>,
}

pub struct TableCellBox {
    pub children: Children,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Children {
    Blocks(Vec<u32>),
    Runs(Range<u32>),
    Empty,
}

#[derive(Clone)]
pub struct InlineRun {
    pub kind: InlineKind,
    pub box_idx: u32,
    pub dom_text_node: Option<u32>, // DOM text node that produced this run (None for image/break)
}

#[derive(Clone)]
pub enum InlineKind {
    Text { glyphs: Range<u32> },
    Image { image_idx: u32 },
    Break,
}

#[derive(Clone)]
pub struct Line {
    pub glyphs: Range<u32>,
    pub point: Point,
    pub height: f64,
    pub baseline: f64,
    pub word_spacing: f64,   // extra space to add after each space (for justification)
    pub letter_spacing: f64, // extra space to add after each letter (for justification)
}

pub struct GlyphOffsetRun {
    pub range: Range<u32>,
    pub offset: f32,
}

pub struct DecorationFragment {
    pub rect: Rect,
    pub color: u32,
    pub is_inline: bool,
}

#[derive(Clone)]
pub enum ImageSource {
    Uri(String),
}

#[derive(Clone)]
pub struct ImageResource {
    pub source: ImageSource,
    pub width: u32,
    pub height: u32,
    pub width_attr: Option<u32>,
    pub height_attr: Option<u32>,
}

impl ImageResource {
    pub fn display_size(&self) -> (f64, f64) {
        let width = self.width_attr.unwrap_or(self.width) as f64;
        let height = self.height_attr.unwrap_or(self.height) as f64;
        (width.max(1.0), height.max(1.0))
    }
}

pub struct ImageFragment {
    pub line_idx: usize,
    pub image_idx: u32,
    pub offset: Point,
    pub size: Size,
}

#[derive(Default)]
pub struct StringInterner {
    map: FxHashMap<String, u16>,
    strings: Vec<String>,
}

impl StringInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> u16 {
        if let Some(&idx) = self.map.get(s) {
            return idx;
        }
        let idx = self.strings.len() as u16;
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), idx);
        idx
    }

    pub fn get(&self, idx: u16) -> &str {
        &self.strings[idx as usize]
    }

    pub fn lookup(&self, s: &str) -> Option<u16> {
        self.map.get(s).copied()
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }
}
