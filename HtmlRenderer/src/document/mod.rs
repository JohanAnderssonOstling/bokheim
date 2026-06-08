pub mod cfi;
mod computed_style;
mod dom;
pub use computed_style::*;
pub use dom::*;
use floem::kurbo::{Point, Rect, Size};
use rustc_data_structures::fx::FxHashMap;
use std::ops::Range;

const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

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

    pub fn print_box_tree(&self) {
        if !self.box_tree.boxes.is_empty() {
            self.print_box_node(0, 0);
        }
    }

    fn print_box_node(&self, idx: usize, depth: usize) {
        let indent = "  ".repeat(depth);
        let b = &self.box_tree.boxes[idx];
        let tag = self.get_tag(idx);
        let dom_id = b.dom_element.map(|i| i.to_string()).unwrap_or("-".into());
        let display = b.style.map(|si| format!("{:?}", self.styles.box_styles[si.box_idx as usize].display)).unwrap_or("none".into());
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
                        self.print_box_node(i as usize, depth + 1);
                    }
                }
            }
            LayoutMode::Table(tb) => {
                for &i in &tb.rows {
                    self.print_box_node(i as usize, depth + 1);
                }
            }
            LayoutMode::TableRow(tr) => {
                for &i in &tr.cells {
                    self.print_box_node(i as usize, depth + 1);
                }
            }
            _ => {}
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

    /// DOM element backing a layout box, if any (None for anonymous boxes).
    pub fn box_element(&self, box_idx: usize) -> Option<&DomElement> {
        self.box_tree.boxes[box_idx].dom_element.and_then(|node_idx| self.dom_nodes[node_idx as usize].as_element())
    }

    pub fn get_tag(&self, box_idx: usize) -> &str {
        self.box_element(box_idx).map(|e| self.strings.get(e.tag)).unwrap_or("")
    }

    pub fn get_id(&self, box_idx: usize) -> Option<&str> {
        self.get_box_id(box_idx).map(|idx| self.strings.get(idx))
    }

    /// Interned id of a box's DOM element, if any.
    pub fn get_box_id(&self, box_idx: usize) -> Option<u16> {
        self.box_element(box_idx).and_then(|e| e.id)
    }

    /// Interned href of a box's DOM element, if any.
    pub fn get_box_href(&self, box_idx: usize) -> Option<u16> {
        self.box_element(box_idx).and_then(|e| e.href)
    }

    pub fn get_box_point(&self, box_idx: usize) -> Point {
        self.box_tree.boxes[box_idx].point
    }

    /// Get classes for a box as iterator
    pub fn get_classes(&self, box_idx: usize) -> impl Iterator<Item = &str> {
        let range = self.box_element(box_idx).map(|e| e.classes.clone()).unwrap_or(0..0);
        self.class_list[range.start as usize..range.end as usize].iter().map(|&idx| self.strings.get(idx))
    }

    pub fn has_class(&self, box_idx: usize, class: &str) -> bool {
        self.get_classes(box_idx).any(|c| c == class)
    }

    /// Get parent box index
    pub fn get_parent(&self, box_idx: usize) -> Option<usize> {
        self.box_tree.boxes[box_idx].parent.map(|p| p as usize)
    }

    /// Iterate ancestors (parent, grandparent, etc.)
    pub fn ancestors(&self, box_idx: usize) -> AncestorIter<'_> {
        AncestorIter { doc: self, current: self.box_tree.boxes[box_idx].parent }
    }

    // ========== DOM NODE ACCESSORS ==========

    /// Get tag name for a DOM element
    pub fn get_dom_tag(&self, node_idx: usize) -> Option<&str> {
        match &self.dom_nodes.get(node_idx)? {
            DomNode::Element(e) => Some(self.strings.get(e.tag)),
            DomNode::Text(_) => None,
        }
    }

    /// Get ID for a DOM element
    pub fn get_dom_id(&self, node_idx: usize) -> Option<&str> {
        match &self.dom_nodes.get(node_idx)? {
            DomNode::Element(e) => e.id.map(|idx| self.strings.get(idx)),
            DomNode::Text(_) => None,
        }
    }

    /// Get classes for a DOM element as iterator
    pub fn get_dom_classes(&self, node_idx: usize) -> Box<dyn Iterator<Item = &str> + '_> {
        match &self.dom_nodes.get(node_idx) {
            Some(DomNode::Element(e)) => {
                let range = e.classes.clone();
                Box::new(self.class_list[range.start as usize..range.end as usize].iter().map(|&idx| self.strings.get(idx)))
            }
            _ => Box::new(std::iter::empty()),
        }
    }

    /// Check if DOM element has a specific class
    pub fn dom_has_class(&self, node_idx: usize, class: &str) -> bool {
        self.get_dom_classes(node_idx).any(|c| c == class)
    }

    /// Get parent DOM node index
    pub fn get_dom_parent(&self, node_idx: usize) -> Option<usize> {
        self.dom_nodes.get(node_idx)?.parent().map(|p| p as usize)
    }

    pub fn get_dom_namespace(&self, node_idx: usize) -> Option<&str> {
        match &self.dom_nodes.get(node_idx)? {
            DomNode::Element(e) => e.namespace.map(|idx| self.strings.get(idx)),
            DomNode::Text(_) => None,
        }
    }

    pub fn is_html_element_in_html_document(&self, node_idx: usize) -> bool {
        self.mode == DocumentMode::Html && self.get_dom_namespace(node_idx).is_some_and(|namespace| namespace == HTML_NAMESPACE)
    }

    /// Iterate DOM ancestors (parent, grandparent, etc.)
    pub fn dom_ancestors(&self, node_idx: usize) -> DomAncestorIter<'_> {
        DomAncestorIter { doc: self, current: self.dom_nodes.get(node_idx).and_then(|n| n.parent()) }
    }

    /// Get href attribute for DOM element
    pub fn get_dom_href(&self, node_idx: usize) -> Option<&str> {
        match &self.dom_nodes.get(node_idx)? {
            DomNode::Element(e) => e.href.map(|idx| self.strings.get(idx)),
            DomNode::Text(_) => None,
        }
    }

    pub fn get_dom_attr(&self, node_idx: usize, attr: &str) -> Option<&str> {
        match self.dom_nodes.get(node_idx)? {
            DomNode::Element(e) => e.attributes.iter().find(|entry| entry.namespace.is_none() && self.strings.get(entry.name) == attr).map(|entry| self.strings.get(entry.value)),
            _ => None,
        }
    }

    pub fn has_dom_attr(&self, node_idx: usize, attr: &str) -> bool {
        self.get_dom_attr(node_idx, attr).is_some()
    }

    pub fn get_dom_attr_expanded(&self, node_idx: usize, namespace: Option<&str>, local_name: &str) -> Option<&str> {
        match self.dom_nodes.get(node_idx)? {
            DomNode::Element(e) => e
                .attributes
                .iter()
                .find(|entry| {
                    self.strings.get(entry.local_name) == local_name
                        && match (namespace, entry.namespace) {
                            (None, None) => true,
                            (Some(expected), Some(actual)) => self.strings.get(actual) == expected,
                            _ => false,
                        }
                })
                .map(|entry| self.strings.get(entry.value)),
            _ => None,
        }
    }

    pub fn has_dom_attr_expanded(&self, node_idx: usize, namespace: Option<&str>, local_name: &str) -> bool {
        self.get_dom_attr_expanded(node_idx, namespace, local_name).is_some()
    }

    pub fn get_dom_attr_in_any_namespace(&self, node_idx: usize, local_name: &str) -> Option<&str> {
        match self.dom_nodes.get(node_idx)? {
            DomNode::Element(e) => e.attributes.iter().find(|entry| self.strings.get(entry.local_name) == local_name).map(|entry| self.strings.get(entry.value)),
            _ => None,
        }
    }

    pub fn has_dom_attr_in_any_namespace(&self, node_idx: usize, local_name: &str) -> bool {
        self.get_dom_attr_in_any_namespace(node_idx, local_name).is_some()
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

    /// Returns (text_node_idx, char_offset) for the glyph.
    pub fn get_dom_node_for_glyph(&self, glyph_idx: u32) -> Option<(u32, usize)> {
        for run in &self.box_tree.inline_runs {
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
pub struct Document {
    // ========== DOM TREE (Phase 1: Parsing) ==========
    pub dom_nodes: Vec<DomNode>,
    pub dom_root: Option<u32>,
    pub mode: DocumentMode,

    // ========== BOX TREE (Phase 3: Box Construction) ==========
    // The box tree plus the inline text content (glyphs, runs) it references.
    pub box_tree: BoxTree,

    // ========== RESOURCES ==========
    pub images: Vec<ImageResource>,
    pub title: Option<String>,

    // ========== LAYOUT RESULTS (Phase 4: Layout) ==========
    pub layout: LayoutResult,

    // ========== INTERNING ==========
    pub strings: StringInterner,
    pub class_list: Vec<u16>, // interned class indices

    // ========== LINKS & ANCHORS ==========
    pub link_glyph_hrefs: FxHashMap<u32, u16>,
    pub anchor_glyphs: FxHashMap<u16, u32>,
    pub toc_entries: Vec<DocumentTocEntry>,
    pub document_toc_entries: Vec<DocumentTocNode>,

    // ========== STYLES (Phase 2: Style Resolution) ==========
    pub root_font_size: f32,

    // Computed styles (Servo-inspired split design), indexed by StyleIndices.
    pub styles: StyleStore,
}

/// Output of the layout phase: line boxes, glyph offsets, and paint fragments.
/// Produced by `LayoutEngine` and consumed read-only by the renderer.
#[derive(Default)]
pub struct LayoutResult {
    pub lines: Vec<Line>,
    pub line_glyph_offsets: Vec<Vec<GlyphOffsetRun>>,
    pub decorations: Vec<DecorationFragment>,
    pub image_fragments: Vec<ImageFragment>,
    pub image_fragments_by_line: Vec<Vec<usize>>,
    pub anchor_positions: FxHashMap<u16, AnchorPosition>,
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
            box_tree: BoxTree::default(),
            images: Vec::new(),
            title: None,
            layout: LayoutResult::default(),
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
    doc: &'a Document,
    current: Option<u32>,
}

impl<'a> Iterator for AncestorIter<'a> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.current?;
        self.current = self.doc.box_tree.boxes[idx as usize].parent;
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
    pub boxes: Vec<LayoutBox>,
    pub root_box: Option<u32>,
    pub glyphs: Vec<u16>,
    pub inline_runs: Vec<InlineRun>,
    /// Maps each glyph index to the index of the box that owns it. Derived from
    /// `inline_runs`; rebuilt by `rebuild_glyph_box_map` once the glyph buffer is
    /// finalized (after whitespace collapse) and shared by shaping and layout
    /// instead of being reconstructed at each site.
    pub glyph_box: Vec<u32>,
}

impl BoxTree {
    /// Rebuild `glyph_box` from the inline text runs. Glyphs not covered by any
    /// text run keep box index 0. Must be called after the glyph buffer reaches
    /// its final length (post whitespace-collapse).
    pub fn rebuild_glyph_box_map(&mut self) {
        let mut map = vec![0u32; self.glyphs.len()];
        for run in &self.inline_runs {
            if let InlineKind::Text { glyphs } = &run.kind {
                for i in glyphs.clone() {
                    if let Some(slot) = map.get_mut(i as usize) {
                        *slot = run.box_idx;
                    }
                }
            }
        }
        self.glyph_box = map;
    }

    /// Return the glyph→box map, rebuilding it first if it is stale (its length
    /// no longer matches the glyph buffer). Lets layout run safely even if it is
    /// invoked without a preceding shaping pass.
    pub fn glyph_box_map(&mut self) -> &[u32] {
        if self.glyph_box.len() != self.glyphs.len() {
            self.rebuild_glyph_box_map();
        }
        &self.glyph_box
    }
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
