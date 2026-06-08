use crate::document::{BlockBox, BoxTree, Children, Display, Document, ElementRef, InlineKind, InlineRun, LayoutBox, LayoutMode, NodeRef, PreferredSize, StyleIndices, StyleView, TableBox, TableCellBox, TableRowBox, WhiteSpace};
use floem::kurbo::{Point, Size};
use std::ops::Range;

const DEFAULT_STYLE_INDICES: StyleIndices = StyleIndices { font_idx: 0, text_idx: 0, box_idx: 0, border_idx: 0, bg_idx: 0 };

/// Builds the box tree from a styled DOM tree
/// Phase 3: DOM + Styles → Box Tree
///
/// Reads the immutable DOM and resolved styles from `dom`, and accumulates the
/// box tree into the owned `out`. Keeping input and output as separate borrows
/// lets the builder iterate DOM children while pushing boxes without cloning.
pub struct BoxTreeBuilder<'a> {
    dom: &'a Document,
    out: BoxTree,
}

impl<'a> BoxTreeBuilder<'a> {
    pub fn new(dom: &'a Document) -> Self {
        Self { dom, out: BoxTree::default() }
    }

    /// Build the entire box tree from the DOM root
    pub fn build(mut self) -> BoxTree {
        if let Some(root_idx) = self.dom.dom_root() {
            let root_box = self.build_box_for_node(root_idx as u32, None);
            self.out.set_root_box(root_box);
        }
        self.out
    }

    /// Build a box (and its children) for a DOM node
    /// Returns the box index, or None if display:none or text node
    fn build_box_for_node(&mut self, node_idx: u32, parent_box: Option<u32>) -> Option<u32> {
        match self.dom.node_ref(node_idx as usize)? {
            NodeRef::Element(_) => self.build_box_for_element(node_idx, parent_box),
            NodeRef::Text(_) => {
                // Text nodes don't create boxes directly
                // They're processed when building inline content for their parent
                None
            }
        }
    }

    /// Build a box for an element
    fn build_box_for_element(&mut self, elem_idx: u32, parent_box: Option<u32>) -> Option<u32> {
        let element = self.dom.element_ref(elem_idx as usize)?;
        let style_indices = element.style()?;
        let display = self.display_for_style(style_indices);

        // Don't create a box for display:none elements
        if display == Display::None {
            return None;
        }

        // Determine if this element has block or inline children
        let has_block_child = self.has_block_child(element);
        let box_idx = self.push_layout_box(elem_idx, parent_box, Some(style_indices), self.layout_mode_for_display(display)?);

        match display {
            Display::Table | Display::InlineTable => self.build_table_children(elem_idx, box_idx),
            Display::TableRow => self.build_table_row_children(elem_idx, box_idx),
            Display::TableCell => self.build_cell_children(elem_idx, box_idx),
            _ => {
                // Build children based on whether we have block or inline content
                if has_block_child {
                    self.build_block_children(elem_idx, box_idx);
                } else {
                    self.build_inline_children(elem_idx, box_idx);
                }
            }
        }

        Some(box_idx)
    }

    /// Check if element has any block-level children
    fn has_block_child(&self, element: ElementRef<'_>) -> bool {
        element.children().any(|child_idx| self.dom.element_ref(child_idx).and_then(|child| child.style()).map(|style| self.is_block_level_child_display(self.display_for_style(style))).unwrap_or(false))
    }

    /// Build block children for an element
    fn build_block_children(&mut self, elem_idx: u32, box_idx: u32) {
        let dom = self.dom;
        let Some(element) = dom.element_ref(elem_idx as usize) else {
            return;
        };
        let children: Vec<u32> = element.children().map(|idx| idx as u32).collect();

        let mut direct_children: Vec<u32> = Vec::new();
        let mut inline_segment: Vec<u32> = Vec::new();

        for child_idx in children {
            match dom.node_ref(child_idx as usize) {
                Some(NodeRef::Element(child_elem)) => {
                    let display = self.display_for_element(child_elem);

                    if self.starts_new_block_child(display) {
                        if let Some(anon_idx) = self.flush_inline_segment(&mut inline_segment, box_idx) {
                            direct_children.push(anon_idx);
                        }
                        if let Some(child_box_idx) = self.build_box_for_node(child_idx, Some(box_idx)) {
                            direct_children.push(child_box_idx);
                        }
                    } else {
                        inline_segment.push(child_idx);
                    }
                }
                Some(NodeRef::Text(_)) => {
                    inline_segment.push(child_idx);
                }
                None => {}
            }
        }

        if let Some(anon_idx) = self.flush_inline_segment(&mut inline_segment, box_idx) {
            direct_children.push(anon_idx);
        }

        self.set_box_children(box_idx, Children::Blocks(direct_children));
    }

    /// Build inline children for an element
    fn build_inline_children(&mut self, elem_idx: u32, box_idx: u32) {
        let dom = self.dom;
        let Some(element) = dom.element_ref(elem_idx as usize) else {
            return;
        };

        let start = self.out.inline_runs().len() as u32;
        let image_idx = element.image_idx();
        let children: Vec<u32> = element.children().map(|idx| idx as u32).collect();

        if let Some(image_idx) = image_idx {
            self.out.push_inline_run(InlineRun { kind: InlineKind::Image { image_idx }, box_idx, dom_text_node: None });
        }

        for child_idx in children {
            self.process_inline_node(child_idx, box_idx);
        }

        let end = self.out.inline_runs().len() as u32;

        // Update parent box with runs range
        if start < end {
            self.set_box_children(box_idx, Children::Runs(start..end));
        }
    }

    fn build_cell_children(&mut self, elem_idx: u32, box_idx: u32) {
        let Some(element) = self.dom.element_ref(elem_idx as usize) else {
            return;
        };

        if self.has_block_child(element) {
            self.build_block_children(elem_idx, box_idx);
        } else {
            self.build_inline_children(elem_idx, box_idx);
        }
    }

    fn build_table_children(&mut self, elem_idx: u32, table_box_idx: u32) {
        let dom = self.dom;
        let Some(element) = dom.element_ref(elem_idx as usize) else {
            return;
        };
        let children: Vec<u32> = element.children().map(|idx| idx as u32).collect();

        let mut rows = Vec::new();
        let mut captions_top = Vec::new();
        let mut captions_bottom = Vec::new();
        let mut column_width_hints = Vec::new();

        for child_idx in children {
            let Some(child_elem) = dom.element_ref(child_idx as usize) else {
                continue;
            };
            let child_style = child_elem.style();
            let display = self.display_for_element(child_elem);
            let caption_side = child_style.map(|style| dom.box_model_style(style.box_idx).caption_side).unwrap_or(crate::document::CaptionSide::Top);
            let child_width = self.preferred_width_for_style(child_style);

            match display {
                Display::TableCaption => {
                    if let Some(caption_idx) = self.build_box_for_node(child_idx, Some(table_box_idx)) {
                        match caption_side {
                            crate::document::CaptionSide::Bottom => captions_bottom.push(caption_idx),
                            crate::document::CaptionSide::Top => captions_top.push(caption_idx),
                        }
                    }
                }
                Display::TableColumn => {
                    let span = parse_dom_span(dom, child_idx as usize);
                    for _ in 0..span {
                        column_width_hints.push(child_width);
                    }
                }
                Display::TableColumnGroup => {
                    let mut group_added = false;
                    let group_children: Vec<u32> = child_elem.children().map(|idx| idx as u32).collect();
                    for group_child_idx in group_children {
                        let Some(group_child_elem) = dom.element_ref(group_child_idx as usize) else {
                            continue;
                        };
                        let group_display = self.display_for_element(group_child_elem);
                        if group_display == Display::TableColumn {
                            group_added = true;
                            let span = parse_dom_span(dom, group_child_idx as usize);
                            let width = self.preferred_width_for_style(group_child_elem.style());
                            for _ in 0..span {
                                column_width_hints.push(width);
                            }
                        }
                    }
                    if !group_added {
                        let span = parse_dom_span(dom, child_idx as usize);
                        for _ in 0..span {
                            column_width_hints.push(child_width);
                        }
                    }
                }
                Display::TableRow => {
                    if let Some(row_idx) = self.build_box_for_node(child_idx, Some(table_box_idx)) {
                        rows.push(row_idx);
                    }
                }
                Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
                    let group_children: Vec<u32> = child_elem.children().map(|idx| idx as u32).collect();
                    for group_child_idx in group_children {
                        let Some(group_child_elem) = dom.element_ref(group_child_idx as usize) else {
                            continue;
                        };
                        let group_display = self.display_for_element(group_child_elem);
                        if group_display == Display::TableRow {
                            if let Some(row_idx) = self.build_box_for_node(group_child_idx, Some(table_box_idx)) {
                                rows.push(row_idx);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(LayoutMode::Table(table_box)) = self.out.box_at_mut(table_box_idx as usize).map(|b| b.layout_mode_mut()) {
            table_box.rows = rows;
            table_box.captions_top = captions_top;
            table_box.captions_bottom = captions_bottom;
            table_box.column_width_hints = column_width_hints;
        }
    }

    fn build_table_row_children(&mut self, elem_idx: u32, row_box_idx: u32) {
        let dom = self.dom;
        let Some(element) = dom.element_ref(elem_idx as usize) else {
            return;
        };
        let children: Vec<u32> = element.children().map(|idx| idx as u32).collect();

        let mut cells = Vec::new();

        for child_idx in children {
            let Some(child_elem) = dom.element_ref(child_idx as usize) else {
                continue;
            };
            let display = self.display_for_element(child_elem);
            if display == Display::TableCell {
                if let Some(cell_idx) = self.build_box_for_node(child_idx, Some(row_box_idx)) {
                    cells.push(cell_idx);
                }
            }
        }

        if let Some(LayoutMode::TableRow(row_box)) = self.out.box_at_mut(row_box_idx as usize).map(|b| b.layout_mode_mut()) {
            row_box.cells = cells;
        }
    }

    fn set_box_children(&mut self, box_idx: u32, children: Children) {
        if let Some(layout_mode) = self.out.box_at_mut(box_idx as usize).map(|b| b.layout_mode_mut()) {
            match layout_mode {
                LayoutMode::Block(block_box) => {
                    block_box.children = children;
                }
                LayoutMode::TableCell(cell_box) => {
                    cell_box.children = children;
                }
                _ => {}
            }
        }
    }

    /// Process a node as inline content
    fn process_inline_node(&mut self, node_idx: u32, box_idx: u32) {
        let dom = self.dom;
        match dom.node_ref(node_idx as usize) {
            Some(NodeRef::Text(_)) => {
                self.process_text_node(node_idx, box_idx);
            }
            Some(NodeRef::Element(elem)) => {
                if self.display_for_element(elem) == Display::None {
                    return;
                }
                if elem.tag().eq_ignore_ascii_case("br") {
                    self.out.push_inline_run(InlineRun { kind: InlineKind::Break, box_idx, dom_text_node: None });
                    return;
                }

                // Create inline box for element
                let run_start = self.out.inline_runs().len() as u32;
                let style = elem.style();
                let image_idx = elem.image_idx();
                let children: Vec<u32> = elem.children().map(|idx| idx as u32).collect();
                let elem_box_idx = self.push_layout_box(node_idx, Some(box_idx), style, LayoutMode::Inline(Range::default()));

                if let Some(image_idx) = image_idx {
                    self.out.push_inline_run(InlineRun { kind: InlineKind::Image { image_idx }, box_idx: elem_box_idx, dom_text_node: None });
                } else {
                    // Process children
                    for child_idx in children {
                        self.process_inline_node(child_idx, elem_box_idx);
                    }
                }

                let run_end = self.out.inline_runs().len() as u32;

                // Update box with runs range
                if let Some(box_) = self.out.box_at_mut(elem_box_idx as usize) {
                    *box_.layout_mode_mut() = LayoutMode::Inline(run_start..run_end);
                }
            }
            None => {}
        }
    }

    /// Process a text node and create inline runs with transformations
    fn process_text_node(&mut self, node_idx: u32, box_idx: u32) {
        let Some(text_node) = self.dom.text_ref(node_idx as usize) else {
            return;
        };
        let text = text_node.text();

        if text.is_empty() {
            return;
        }

        // Preserve raw text here. The shaping pass applies text-transform and
        // whitespace collapse across the full inline stream instead of per node.
        let glyph_start = self.out.glyphs().len() as u32;
        for c in text.chars() {
            self.out.push_glyph(c as u16);
        }
        let glyph_end = self.out.glyphs().len() as u32;

        self.out.push_inline_run(InlineRun { kind: InlineKind::Text { glyphs: glyph_start..glyph_end }, box_idx, dom_text_node: Some(node_idx) });
    }

    /// Create anonymous box for inline content, returns box index
    /// Anonymous boxes inherit text/font properties from parent, use defaults for reset properties
    fn create_anonymous_box(&mut self, run_start: u32, parent: Option<u32>) -> Option<u32> {
        let run_end = self.out.inline_runs().len() as u32;
        if run_start < run_end {
            let idx = self.out.box_count() as u32;

            // Get parent's style to inherit font and text indices
            let style = parent.and_then(|parent_idx| self.out.box_at(parent_idx as usize)).and_then(|parent_box| parent_box.style()).map(|parent_style| StyleIndices {
                font_idx: parent_style.font_idx,
                text_idx: parent_style.text_idx,
                box_idx: DEFAULT_STYLE_INDICES.box_idx,
                border_idx: DEFAULT_STYLE_INDICES.border_idx,
                bg_idx: DEFAULT_STYLE_INDICES.bg_idx,
            });

            let mut anon_box = LayoutBox::new_anonymous_box(run_start..run_end, parent);
            anon_box.set_style(style);
            self.out.push_box(anon_box);
            Some(idx)
        } else {
            None
        }
    }

    fn flush_inline_segment(&mut self, nodes: &mut Vec<u32>, parent_box_idx: u32) -> Option<u32> {
        if nodes.is_empty() {
            return None;
        }

        let white_space = self.style_for_box(parent_box_idx as usize).white_space();
        let segment = self.trim_inline_segment_nodes(nodes.as_slice(), white_space);
        if segment.is_empty() {
            nodes.clear();
            return None;
        }

        let run_start = self.out.inline_runs().len() as u32;
        for &node_idx in segment {
            self.process_inline_node(node_idx, parent_box_idx);
        }
        nodes.clear();
        self.create_anonymous_box(run_start, Some(parent_box_idx))
    }

    fn trim_inline_segment_nodes<'b>(&self, nodes: &'b [u32], white_space: WhiteSpace) -> &'b [u32] {
        let mut start = 0usize;
        let mut end = nodes.len();

        while start < end && self.node_is_ignorable_at_block_edge(nodes[start], white_space) {
            start += 1;
        }
        while start < end && self.node_is_ignorable_at_block_edge(nodes[end - 1], white_space) {
            end -= 1;
        }

        if start >= end || !nodes[start..end].iter().copied().any(|node_idx| self.node_has_renderable_inline_content(node_idx)) { &nodes[0..0] } else { &nodes[start..end] }
    }

    fn node_is_ignorable_at_block_edge(&self, node_idx: u32, white_space: WhiteSpace) -> bool {
        match self.dom.node_ref(node_idx as usize) {
            Some(NodeRef::Text(text)) => self.text_is_ignorable_at_block_edge(text.text(), white_space),
            Some(NodeRef::Element(_)) => !self.node_has_renderable_inline_content(node_idx),
            None => false,
        }
    }

    fn node_has_renderable_inline_content(&self, node_idx: u32) -> bool {
        match self.dom.node_ref(node_idx as usize) {
            Some(NodeRef::Text(text)) => {
                let Some(parent_idx) = text.parent() else {
                    return false;
                };
                let white_space = self.style_for_dom_node(parent_idx as u32).white_space();
                self.text_has_renderable_content(text.text(), white_space)
            }
            Some(NodeRef::Element(elem)) => {
                if matches!(elem.style(), Some(style) if self.display_for_style(style) == Display::None) {
                    return false;
                }
                if elem.tag().eq_ignore_ascii_case("br") {
                    return true;
                }
                if elem.image_idx().is_some() {
                    return true;
                }
                elem.children().any(|child_idx| self.node_has_renderable_inline_content(child_idx as u32))
            }
            None => false,
        }
    }

    fn text_has_renderable_content(&self, text: &str, white_space: WhiteSpace) -> bool {
        match white_space {
            WhiteSpace::Pre | WhiteSpace::PreWrap => !text.is_empty(),
            WhiteSpace::PreLine => text.chars().any(|c| c == '\n' || !c.is_whitespace() || c == '\u{00A0}'),
            WhiteSpace::Normal | WhiteSpace::NoWrap => text.chars().any(|c| !c.is_whitespace() || c == '\u{00A0}'),
        }
    }

    fn text_is_ignorable_at_block_edge(&self, text: &str, white_space: WhiteSpace) -> bool {
        match white_space {
            WhiteSpace::Normal | WhiteSpace::NoWrap => !text.is_empty() && text.chars().all(|c| c.is_whitespace() && c != '\u{00A0}'),
            WhiteSpace::PreLine => !text.is_empty() && text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\u{000C}')),
            WhiteSpace::Pre | WhiteSpace::PreWrap => false,
        }
    }

    fn style_for_box(&self, box_idx: usize) -> StyleView<'_> {
        let indices = self.out.box_at(box_idx).and_then(|b| b.style()).unwrap_or(DEFAULT_STYLE_INDICES);
        self.style_for_indices(indices)
    }

    fn style_for_dom_node(&self, node_idx: u32) -> StyleView<'_> {
        let indices = match self.dom.node_ref(node_idx as usize) {
            Some(NodeRef::Element(element)) => element.style().unwrap_or(DEFAULT_STYLE_INDICES),
            Some(NodeRef::Text(text)) => text.parent().and_then(|parent_idx| self.dom.element_ref(parent_idx).and_then(|element| element.style())).unwrap_or(DEFAULT_STYLE_INDICES),
            None => DEFAULT_STYLE_INDICES,
        };
        self.style_for_indices(indices)
    }

    fn style_for_indices(&self, indices: StyleIndices) -> StyleView<'_> {
        self.dom.style_view(indices)
    }

    fn display_for_element(&self, element: ElementRef<'_>) -> Display {
        element.style().map(|style| self.display_for_style(style)).unwrap_or(Display::Block)
    }

    fn display_for_style(&self, style: StyleIndices) -> Display {
        self.dom.box_model_style(style.box_idx).display
    }

    fn preferred_width_for_style(&self, style: Option<StyleIndices>) -> PreferredSize {
        style.map(|style| self.dom.box_model_style(style.box_idx).width).unwrap_or(PreferredSize::Auto)
    }

    fn layout_mode_for_display(&self, display: Display) -> Option<LayoutMode> {
        Some(match display {
            Display::Block | Display::ListItem => LayoutMode::Block(BlockBox { children: Children::Empty }),
            Display::Inline => LayoutMode::Inline(Range::default()),
            Display::Table | Display::InlineTable => LayoutMode::Table(TableBox { rows: Vec::new(), captions_top: Vec::new(), captions_bottom: Vec::new(), column_width_hints: Vec::new() }),
            Display::TableRow => LayoutMode::TableRow(TableRowBox { cells: Vec::new() }),
            Display::TableCell => LayoutMode::TableCell(TableCellBox { children: Children::Empty }),
            Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup | Display::TableColumnGroup | Display::TableColumn | Display::TableCaption => LayoutMode::Block(BlockBox { children: Children::Empty }),
            Display::None => return None,
        })
    }

    fn push_layout_box(&mut self, dom_node_idx: u32, parent: Option<u32>, style: Option<StyleIndices>, layout_mode: LayoutMode) -> u32 {
        debug_assert!(self.dom.element_ref(dom_node_idx as usize).is_some(), "layout boxes are only created for element nodes");
        let idx = self.out.push_box(LayoutBox { size: Size::ZERO, point: Point::ZERO, layout_mode, dom_element: Some(dom_node_idx), parent, style });

        idx
    }

    fn is_block_level_child_display(&self, display: Display) -> bool {
        matches!(display, Display::Block | Display::ListItem | Display::Table | Display::InlineTable | Display::TableRow | Display::TableCell)
    }

    fn starts_new_block_child(&self, display: Display) -> bool {
        self.is_block_level_child_display(display)
    }
}

fn parse_dom_span(document: &Document, node_idx: usize) -> usize {
    document.get_dom_attr(node_idx, "span").and_then(|s| s.trim().parse::<usize>().ok()).filter(|&v| v > 0).unwrap_or(1)
}

/// Helper function to build box tree from document
pub fn build_box_tree(document: &Document) -> BoxTree {
    BoxTreeBuilder::new(document).build()
}

#[cfg(test)]
mod tests {
    use crate::document::{Children, InlineKind, LayoutMode};
    use crate::parser::DocumentFactory;

    #[test]
    fn br_generates_inline_break_run() {
        let html = "<html><body><p>a<br>b</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        assert!(document.box_tree().inline_runs().iter().any(|run| matches!(run.kind, InlineKind::Break)));
    }

    #[test]
    fn table_builds_table_row_and_cell_boxes() {
        let html = "<html><body><table><tr><td>A</td><td>B</td></tr></table></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let table_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("table")).expect("table box should exist");

        let rows = match document.box_at(table_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::Table(table) => table.rows.clone(),
            _ => panic!("table should use LayoutMode::Table"),
            },
            None => panic!("table box should exist"),
        };
        assert_eq!(rows.len(), 1);

        let cells = match document.box_at(rows[0] as usize) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::TableRow(row) => row.cells.clone(),
            _ => panic!("row should use LayoutMode::TableRow"),
            },
            None => panic!("row box should exist"),
        };
        assert_eq!(cells.len(), 2);
        assert!(matches!(document.box_at(cells[0] as usize).map(|b| b.layout_mode()), Some(LayoutMode::TableCell(_))));
        assert!(matches!(document.box_at(cells[1] as usize).map(|b| b.layout_mode()), Some(LayoutMode::TableCell(_))));
    }

    #[test]
    fn table_cell_text_creates_run_children() {
        let html = "<html><body><table><tr><td>Hello</td></tr></table></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let cell_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("td")).expect("cell box should exist");

        let children = match document.box_at(cell_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::TableCell(cell) => &cell.children,
            _ => panic!("cell should use LayoutMode::TableCell"),
            },
            None => panic!("cell box should exist"),
        };
        assert!(matches!(children, Children::Runs(_)));
    }

    #[test]
    fn ignores_whitespace_only_inline_content_between_block_children() {
        let html = "<html><body><section>\n  <p>One</p>\n  <p>Two</p>\n</section></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let section_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("section")).expect("section box should exist");

        let children = match document.box_at(section_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::Block(block) => &block.children,
            _ => panic!("section should use LayoutMode::Block"),
            },
            None => panic!("section box should exist"),
        };

        let Children::Blocks(children) = children else {
            panic!("section should contain block children");
        };

        assert_eq!(children.len(), 2, "whitespace-only anonymous boxes should be ignored");
    }

    #[test]
    fn preserves_separator_whitespace_inside_inline_segment() {
        let html = "<html><body><section><span>One</span> <span>Two</span><p>Block</p></section></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let section_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("section")).expect("section box should exist");

        let children = match document.box_at(section_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::Block(block) => &block.children,
            _ => panic!("section should use LayoutMode::Block"),
            },
            None => panic!("section box should exist"),
        };

        let Children::Blocks(children) = children else {
            panic!("section should contain block children");
        };

        assert_eq!(children.len(), 2, "inline content should become one anonymous block plus the paragraph");

        let anon_idx = children[0] as usize;
        let runs = match document.box_at(anon_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::Anonymous(runs) => runs.clone(),
            _ => panic!("first child should be an anonymous inline wrapper"),
            },
            None => panic!("anonymous box should exist"),
        };

        let mut saw_space = false;
        for run in &document.box_tree().inline_runs()[runs.start as usize..runs.end as usize] {
            if let InlineKind::Text { glyphs } = &run.kind {
                if document.box_tree().glyphs()[glyphs.start as usize..glyphs.end as usize].iter().any(|&glyph| glyph == ' ' as u16) {
                    saw_space = true;
                    break;
                }
            }
        }

        assert!(saw_space, "space between inline siblings should be preserved");
    }

    #[test]
    fn display_none_inline_descendants_do_not_generate_runs() {
        let html = "<html><body><p>Hello<span style=\"display:none\">hidden</span>World</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let rendered_text: String = document
            .box_tree()
            .inline_runs()
            .iter()
            .filter_map(|run| match &run.kind {
                InlineKind::Text { glyphs } => Some(document.box_tree().glyphs()[glyphs.start as usize..glyphs.end as usize].iter().map(|&glyph| char::from_u32(glyph as u32).expect("glyph should be valid char")).collect::<String>()),
                _ => None,
            })
            .collect();

        assert_eq!(rendered_text, "HelloWorld");
    }

    #[test]
    fn inline_table_uses_table_layout_mode() {
        let html = "<html><body><div id=\"table\" style=\"display:inline-table\"><div style=\"display:table-row\"><div style=\"display:table-cell\">A</div></div></div></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let table_idx = (0..document.box_count()).find(|&i| document.get_id(i) == Some("table")).expect("inline-table box should exist");

        let display = document.box_at(table_idx).and_then(|b| b.style()).map(|style| document.box_model_style(style.box_idx).display);
        assert_eq!(display, Some(crate::document::Display::InlineTable));

        let rows = match document.box_at(table_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::Table(table) => table.rows.clone(),
            _ => panic!("inline-table should use LayoutMode::Table"),
            },
            None => panic!("table box should exist"),
        };
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn table_row_child_becomes_block_child_in_mixed_container() {
        let html = "<html><body><div><span>Lead</span><div style=\"display:table-row\"><div style=\"display:table-cell\">Cell</div></div></div></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let div_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("div")).expect("div box should exist");

        let children = match document.box_at(div_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::Block(block) => &block.children,
            _ => panic!("div should use LayoutMode::Block"),
            },
            None => panic!("div box should exist"),
        };

        let Children::Blocks(children) = children else {
            panic!("div should contain block children");
        };

        assert_eq!(children.len(), 2, "inline lead-in should flush before the table row child");
        assert!(matches!(document.box_at(children[0] as usize).map(|b| b.layout_mode()), Some(LayoutMode::Anonymous(_))));
        assert!(matches!(document.box_at(children[1] as usize).map(|b| b.layout_mode()), Some(LayoutMode::TableRow(_))));
    }

    #[test]
    fn table_cell_child_is_not_left_in_inline_segment() {
        let html = "<html><body><div><span>Lead</span><div style=\"display:table-cell\">Cell</div></div></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let div_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("div")).expect("div box should exist");

        let children = match document.box_at(div_idx) {
            Some(box_) => match box_.layout_mode() {
            LayoutMode::Block(block) => &block.children,
            _ => panic!("div should use LayoutMode::Block"),
            },
            None => panic!("div box should exist"),
        };

        let Children::Blocks(children) = children else {
            panic!("div should contain block children");
        };

        assert_eq!(children.len(), 2);
        assert!(matches!(document.box_at(children[1] as usize).map(|b| b.layout_mode()), Some(LayoutMode::TableCell(_))));
    }
}
