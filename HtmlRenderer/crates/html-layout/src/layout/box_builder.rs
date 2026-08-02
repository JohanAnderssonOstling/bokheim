use crate::document::{
    BlockBox, Children, ComputedStyles, Display, Document, DomNodeId, ElementRef, Float, GlyphId, InlineContent, InlineKind, InlineRun, LayoutBox, LayoutMode, LayoutTree, ListItemMarker, ListStyleType, NodeRef, PreferredSize, StyleIndices,
    StyleView, TableBox, TableCellBox, TableRowBox, WhiteSpace,
};
use kurbo::{Point, Size};
use std::ops::Range;

struct LayoutTreeBuilder<'a, 'out> {
    document: &'a Document,
    styles: &'a ComputedStyles,
    layout_tree: &'out mut LayoutTree,
    inline_content: &'out mut InlineContent,
}

impl<'a, 'out> LayoutTreeBuilder<'a, 'out> {
    fn new(document: &'a Document, styles: &'a ComputedStyles, layout_tree: &'out mut LayoutTree, inline_content: &'out mut InlineContent) -> Self {
        Self { document, styles, layout_tree, inline_content }
    }

    fn build(&mut self) {
        if let Some(root_idx) = self.document.dom_root() {
            let root_box = self.build_box_for_node(root_idx, None);
            self.layout_tree.set_root_box(root_box);
        }
    }

    fn build_box_for_node(&mut self, node_id: DomNodeId, parent_box: Option<u32>) -> Option<u32> {
        match self.document.node_ref(node_id)? {
            NodeRef::Element(_) => self.build_box_for_element(node_id, parent_box),
            NodeRef::Text(_) => None,
        }
    }

    fn build_box_for_element(&mut self, elem_id: DomNodeId, parent_box: Option<u32>) -> Option<u32> {
        let element = self.document.element_ref(elem_id)?;
        let style_indices = self.styles.style_for_node(element.node_id())?;
        let display = self.display_for_style(style_indices);

        if display == Display::None {
            return None;
        }

        let has_block_child = self.has_block_child(element);
        let mut layout_mode = self.layout_mode_for_display(display)?;
        if matches!(self.styles.box_model_style(style_indices).expect("validated style handle").float, Float::Left | Float::Right) {
            layout_mode = LayoutMode::Block(BlockBox { children: Children::Empty });
        }
        let box_idx = self.push_layout_box(elem_id, parent_box, Some(style_indices), layout_mode);

        match display {
            Display::Table | Display::InlineTable => self.build_table_children(elem_id, box_idx),
            Display::TableRow => self.build_table_row_children(elem_id, box_idx),
            Display::TableCell => self.build_cell_children(elem_id, box_idx),
            _ => {
                if has_block_child {
                    self.build_block_children(elem_id, box_idx);
                } else {
                    self.build_inline_children(elem_id, box_idx);
                }
            }
        }

        if display == Display::ListItem {
            self.build_list_marker(box_idx, elem_id, style_indices);
        }

        Some(box_idx)
    }

    fn build_list_marker(&mut self, list_item_box: u32, elem_id: DomNodeId, style_indices: StyleIndices) {
        let style = self.styles.view(style_indices).expect("validated style handle");
        let list_style_type = style.list_style_type();
        let position = style.list_style_position();
        if list_style_type == ListStyleType::None {
            return;
        }

        let ordinal = if list_style_type.is_bullet() { 0 } else { self.list_item_ordinal(elem_id) };
        let Some(text) = super::list_marker::marker_text(list_style_type, ordinal) else {
            return;
        };

        let marker_box = self.layout_tree.push_box(LayoutBox { size: Size::ZERO, point: Point::ZERO, layout_mode: LayoutMode::Inline(Range::default()), dom_element: None, parent: Some(list_item_box), style: Some(style_indices) });

        let run_start = self.inline_content.inline_runs().len() as u32;
        let glyph_start = self.inline_content.glyphs().len() as u32;
        for c in text.chars() {
            self.inline_content.push_glyph(c as GlyphId);
        }
        let glyph_end = self.inline_content.glyphs().len() as u32;
        self.inline_content.push_inline_run(InlineRun { kind: InlineKind::Marker { glyphs: glyph_start..glyph_end }, box_idx: marker_box, dom_text_node: None });
        let run_end = self.inline_content.inline_runs().len() as u32;
        if let Some(b) = self.layout_tree.box_at_mut(marker_box as usize) {
            *b.layout_mode_mut() = LayoutMode::Inline(run_start..run_end);
        }

        self.layout_tree.set_list_marker(list_item_box, ListItemMarker { marker_box, position });
    }

    fn list_item_ordinal(&self, elem_id: DomNodeId) -> i64 {
        let Some(element) = self.document.element_ref(elem_id) else {
            return 1;
        };
        let Some(parent_idx) = element.parent() else {
            return 1;
        };
        let Some(parent) = self.document.element_ref(parent_idx) else {
            return 1;
        };

        let reversed = parent.has_attr("reversed");
        let list_items: Vec<DomNodeId> = parent.children().filter(|&child| self.document.element_ref(child).is_some_and(|c| self.display_for_element(c) == Display::ListItem)).collect();

        let start_attr = parent.attr("start").and_then(|s| s.trim().parse::<i64>().ok());
        let mut current = match (reversed, start_attr) {
            (_, Some(start)) => start,
            (true, None) => list_items.len() as i64,
            (false, None) => 1,
        };

        for li in list_items {
            if let Some(value) = self.document.element_ref(li).and_then(|c| c.attr("value")).and_then(|v| v.trim().parse::<i64>().ok()) {
                current = value;
            }
            if li == elem_id {
                return current;
            }
            current += if reversed { -1 } else { 1 };
        }
        current
    }

    fn has_block_child(&self, element: ElementRef<'_>) -> bool {
        element.children().any(|child_idx| self.document.element_ref(child_idx).and_then(|child| self.styles.style_for_node(child.node_id())).map(|style| self.is_block_level_child_display(self.display_for_style(style))).unwrap_or(false))
    }

    fn build_block_children(&mut self, elem_id: DomNodeId, box_idx: u32) {
        let dom = self.document;
        let Some(element) = dom.element_ref(elem_id) else {
            return;
        };
        let children: Vec<u32> = element.children().map(|idx| idx.raw()).collect();

        let mut direct_children: Vec<u32> = Vec::new();
        let mut inline_segment: Vec<u32> = Vec::new();

        for child_idx in children {
            let Some(child_id) = dom.node_id_from_raw(child_idx) else {
                continue;
            };

            match dom.node_ref(child_id) {
                Some(NodeRef::Element(child_elem)) => {
                    let display = self.display_for_element(child_elem);

                    if self.starts_new_block_child(display) {
                        if let Some(anon_idx) = self.flush_inline_segment(&mut inline_segment, box_idx) {
                            direct_children.push(anon_idx);
                        }
                        if let Some(child_box_idx) = self.build_box_for_node(child_id, Some(box_idx)) {
                            direct_children.push(child_box_idx);
                        }
                    } else {
                        inline_segment.push(child_id.raw());
                    }
                }
                Some(NodeRef::Text(_)) => {
                    inline_segment.push(child_id.raw());
                }
                None => {}
            }
        }

        if let Some(anon_idx) = self.flush_inline_segment(&mut inline_segment, box_idx) {
            direct_children.push(anon_idx);
        }

        self.set_box_children(box_idx, Children::Blocks(direct_children));
    }

    fn build_inline_children(&mut self, elem_id: DomNodeId, box_idx: u32) {
        let dom = self.document;
        let Some(element) = dom.element_ref(elem_id) else {
            return;
        };

        let start = self.inline_content.inline_runs().len() as u32;
        let image_idx = element.image_idx();
        let children: Vec<u32> = element.children().map(|idx| idx.raw()).collect();

        if let Some(image_idx) = image_idx {
            self.inline_content.push_inline_run(InlineRun { kind: InlineKind::Image { image_idx }, box_idx, dom_text_node: None });
        }

        for child_idx in children {
            let Some(child_id) = dom.node_id_from_raw(child_idx) else { continue };
            self.process_inline_node(child_id, box_idx);
        }

        let end = self.inline_content.inline_runs().len() as u32;

        if start < end {
            self.set_box_children(box_idx, Children::Runs(start..end));
        }
    }

    fn build_cell_children(&mut self, elem_id: DomNodeId, box_idx: u32) {
        let Some(element) = self.document.element_ref(elem_id) else {
            return;
        };

        if self.has_block_child(element) {
            self.build_block_children(elem_id, box_idx);
        } else {
            self.build_inline_children(elem_id, box_idx);
        }
    }

    fn build_table_children(&mut self, elem_id: DomNodeId, table_box_idx: u32) {
        let dom = self.document;
        let Some(element) = dom.element_ref(elem_id) else {
            return;
        };
        let children: Vec<u32> = element.children().map(|idx| idx.raw()).collect();

        let mut rows = Vec::new();
        let mut captions_top = Vec::new();
        let mut captions_bottom = Vec::new();
        let mut column_width_hints = Vec::new();

        for child_idx in children {
            let Some(child_id) = dom.node_id_from_raw(child_idx) else {
                continue;
            };
            let Some(child_elem) = dom.element_ref(child_id) else {
                continue;
            };
            let child_style = self.styles.style_for_node(child_elem.node_id());
            let display = self.display_for_element(child_elem);
            let caption_side = child_style.map(|style| self.styles.box_model_style(style).expect("validated style handle").caption_side).unwrap_or(crate::document::CaptionSide::Top);
            let child_width = self.preferred_width_for_style(child_style);

            match display {
                Display::TableCaption => {
                    if let Some(caption_idx) = self.build_box_for_node(child_id, Some(table_box_idx)) {
                        match caption_side {
                            crate::document::CaptionSide::Bottom => captions_bottom.push(caption_idx),
                            crate::document::CaptionSide::Top => captions_top.push(caption_idx),
                        }
                    }
                }
                Display::TableColumn => {
                    let span = parse_dom_span(dom, child_id);
                    for _ in 0..span {
                        column_width_hints.push(child_width);
                    }
                }
                Display::TableColumnGroup => {
                    let mut group_added = false;
                    let group_children: Vec<u32> = child_elem.children().map(|idx| idx.raw()).collect();
                    for group_child_idx in group_children {
                        let Some(group_child_id) = dom.node_id_from_raw(group_child_idx) else {
                            continue;
                        };
                        let Some(group_child_elem) = dom.element_ref(group_child_id) else {
                            continue;
                        };
                        let group_display = self.display_for_element(group_child_elem);
                        if group_display == Display::TableColumn {
                            group_added = true;
                            let span = parse_dom_span(dom, group_child_id);
                            let width = self.preferred_width_for_style(self.styles.style_for_node(group_child_elem.node_id()));
                            for _ in 0..span {
                                column_width_hints.push(width);
                            }
                        }
                    }
                    if !group_added {
                        let span = parse_dom_span(dom, child_id);
                        for _ in 0..span {
                            column_width_hints.push(child_width);
                        }
                    }
                }
                Display::TableRow => {
                    if let Some(row_idx) = self.build_box_for_node(child_id, Some(table_box_idx)) {
                        rows.push(row_idx);
                    }
                }
                Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
                    let group_children: Vec<u32> = child_elem.children().map(|idx| idx.raw()).collect();
                    for group_child_idx in group_children {
                        let Some(group_child_id) = dom.node_id_from_raw(group_child_idx) else {
                            continue;
                        };
                        let Some(group_child_elem) = dom.element_ref(group_child_id) else {
                            continue;
                        };
                        let group_display = self.display_for_element(group_child_elem);
                        if group_display == Display::TableRow {
                            if let Some(row_idx) = self.build_box_for_node(group_child_id, Some(table_box_idx)) {
                                rows.push(row_idx);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(LayoutMode::Table(table_box)) = self.layout_tree.box_at_mut(table_box_idx as usize).map(|b| b.layout_mode_mut()) {
            table_box.rows = rows;
            table_box.captions_top = captions_top;
            table_box.captions_bottom = captions_bottom;
            table_box.column_width_hints = column_width_hints;
        }
    }

    fn build_table_row_children(&mut self, elem_id: DomNodeId, row_box_idx: u32) {
        let dom = self.document;
        let Some(element) = dom.element_ref(elem_id) else {
            return;
        };
        let children: Vec<u32> = element.children().map(|idx| idx.raw()).collect();

        let mut cells = Vec::new();

        for child_idx in children {
            let Some(child_id) = dom.node_id_from_raw(child_idx) else {
                continue;
            };
            let Some(child_elem) = dom.element_ref(child_id) else {
                continue;
            };
            let display = self.display_for_element(child_elem);
            if display == Display::TableCell {
                if let Some(cell_idx) = self.build_box_for_node(child_id, Some(row_box_idx)) {
                    cells.push(cell_idx);
                }
            }
        }

        if let Some(LayoutMode::TableRow(row_box)) = self.layout_tree.box_at_mut(row_box_idx as usize).map(|b| b.layout_mode_mut()) {
            row_box.cells = cells;
        }
    }

    fn set_box_children(&mut self, box_idx: u32, children: Children) {
        if let Some(layout_mode) = self.layout_tree.box_at_mut(box_idx as usize).map(|b| b.layout_mode_mut()) {
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

    fn process_inline_node(&mut self, node_id: DomNodeId, box_idx: u32) {
        let dom = self.document;
        match dom.node_ref(node_id) {
            Some(NodeRef::Text(_)) => {
                self.process_text_node(node_id, box_idx);
            }
            Some(NodeRef::Element(elem)) => {
                if self.display_for_element(elem) == Display::None {
                    return;
                }
                if elem.tag().eq_ignore_ascii_case("br") {
                    self.inline_content.push_inline_run(InlineRun { kind: InlineKind::Break, box_idx, dom_text_node: None });
                    return;
                }

                let run_start = self.inline_content.inline_runs().len() as u32;
                let style = self.styles.style_for_node(elem.node_id());
                let image_idx = elem.image_idx();
                let children: Vec<u32> = elem.children().map(|idx| idx.raw()).collect();
                let float_kind = style.map(|s| self.styles.box_model_style(s).expect("validated style handle").float).unwrap_or(Float::None);
                let layout_mode = if matches!(float_kind, Float::Left | Float::Right) { LayoutMode::Block(BlockBox { children: Children::Empty }) } else { LayoutMode::Inline(Range::default()) };
                let elem_box_idx = self.push_layout_box(node_id, Some(box_idx), style, layout_mode);

                if matches!(float_kind, Float::Left | Float::Right) {
                    if let Some(image_idx) = image_idx {
                        self.inline_content.push_inline_run(InlineRun { kind: InlineKind::Image { image_idx }, box_idx: elem_box_idx, dom_text_node: None });
                    } else {
                        for child_idx in children {
                            let Some(child_id) = dom.node_id_from_raw(child_idx) else { continue };
                            self.process_inline_node(child_id, elem_box_idx);
                        }
                    }
                    let run_end = self.inline_content.inline_runs().len() as u32;
                    if run_start < run_end {
                        self.set_box_children(elem_box_idx, Children::Runs(run_start..run_end));
                    }
                    self.inline_content.push_inline_run(InlineRun { kind: InlineKind::FloatAnchor { box_idx: elem_box_idx }, box_idx, dom_text_node: None });
                } else {
                    if let Some(image_idx) = image_idx {
                        self.inline_content.push_inline_run(InlineRun { kind: InlineKind::Image { image_idx }, box_idx: elem_box_idx, dom_text_node: None });
                    } else {
                        for child_idx in children {
                            let Some(child_id) = dom.node_id_from_raw(child_idx) else { continue };
                            self.process_inline_node(child_id, elem_box_idx);
                        }
                    }

                    let run_end = self.inline_content.inline_runs().len() as u32;

                    if let Some(box_) = self.layout_tree.box_at_mut(elem_box_idx as usize) {
                        *box_.layout_mode_mut() = LayoutMode::Inline(run_start..run_end);
                    }
                }
            }
            None => {}
        }
    }

    fn process_text_node(&mut self, node_id: DomNodeId, box_idx: u32) {
        let Some(text_node) = self.document.text_ref(node_id) else {
            return;
        };
        let text = text_node.text();

        if text.is_empty() {
            return;
        }

        let glyph_start = self.inline_content.glyphs().len() as u32;
        for c in text.chars() {
            self.inline_content.push_glyph(c as GlyphId);
        }
        let glyph_end = self.inline_content.glyphs().len() as u32;

        self.inline_content.push_inline_run(InlineRun { kind: InlineKind::Text { glyphs: glyph_start..glyph_end }, box_idx, dom_text_node: Some(node_id.raw()) });
    }

    fn create_anonymous_box(&mut self, run_start: u32, parent: Option<u32>) -> Option<u32> {
        let run_end = self.inline_content.inline_runs().len() as u32;
        if run_start < run_end {
            let idx = self.layout_tree.box_count() as u32;

            let style = parent.and_then(|parent_idx| self.layout_tree.box_at(parent_idx as usize)).and_then(|parent_box| parent_box.style()).and_then(|parent_style| self.styles.anonymous_box_indices(parent_style));

            let mut anon_box = LayoutBox::new_anonymous_box(run_start..run_end, parent);
            anon_box.set_style(style);
            self.layout_tree.push_box(anon_box);
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

        let run_start = self.inline_content.inline_runs().len() as u32;
        for &node_idx in segment {
            let Some(node_id) = self.document.node_id_from_raw(node_idx) else {
                continue;
            };
            self.process_inline_node(node_id, parent_box_idx);
        }
        nodes.clear();
        self.create_anonymous_box(run_start, Some(parent_box_idx))
    }

    fn trim_inline_segment_nodes<'b>(&self, nodes: &'b [u32], white_space: WhiteSpace) -> &'b [u32] {
        let mut start = 0usize;
        let mut end = nodes.len();

        while start < end && self.document.node_id_from_raw(nodes[start]).is_some_and(|node_id| self.node_is_ignorable_at_block_edge(node_id, white_space)) {
            start += 1;
        }
        while start < end && self.document.node_id_from_raw(nodes[end - 1]).is_some_and(|node_id| self.node_is_ignorable_at_block_edge(node_id, white_space)) {
            end -= 1;
        }

        if start >= end || !nodes[start..end].iter().copied().filter_map(|node_idx| self.document.node_id_from_raw(node_idx)).any(|node_id| self.node_has_renderable_inline_content(node_id)) { &nodes[0..0] } else { &nodes[start..end] }
    }

    fn node_is_ignorable_at_block_edge(&self, node_id: DomNodeId, white_space: WhiteSpace) -> bool {
        match self.document.node_ref(node_id) {
            Some(NodeRef::Text(text)) => self.text_is_ignorable_at_block_edge(text.text(), white_space),
            Some(NodeRef::Element(_)) => !self.node_has_renderable_inline_content(node_id),
            None => false,
        }
    }

    fn node_has_renderable_inline_content(&self, node_id: DomNodeId) -> bool {
        match self.document.node_ref(node_id) {
            Some(NodeRef::Text(text)) => {
                let Some(parent_idx) = text.parent() else {
                    return false;
                };
                let white_space = self.style_for_dom_node(parent_idx).white_space();
                self.text_has_renderable_content(text.text(), white_space)
            }
            Some(NodeRef::Element(elem)) => {
                if matches!(self.styles.style_for_node(elem.node_id()), Some(style) if self.display_for_style(style) == Display::None) {
                    return false;
                }
                if elem.tag().eq_ignore_ascii_case("br") {
                    return true;
                }
                if elem.image_idx().is_some() {
                    return true;
                }
                elem.children().any(|child_idx| self.node_has_renderable_inline_content(child_idx))
            }
            None => false,
        }
    }

    fn text_has_renderable_content(&self, text: &str, white_space: WhiteSpace) -> bool {
        if white_space.preserves_spaces() {
            !text.is_empty()
        } else if white_space.preserves_newlines() {
            text.chars().any(|c| c == '\n' || !c.is_whitespace() || c == '\u{00A0}')
        } else {
            text.chars().any(|c| !c.is_whitespace() || c == '\u{00A0}')
        }
    }

    fn text_is_ignorable_at_block_edge(&self, text: &str, white_space: WhiteSpace) -> bool {
        if white_space.preserves_spaces() {
            false
        } else if white_space.preserves_newlines() {
            !text.is_empty() && text.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\u{000C}'))
        } else {
            !text.is_empty() && text.chars().all(|c| c.is_whitespace() && c != '\u{00A0}')
        }
    }

    fn style_for_box(&self, box_idx: usize) -> StyleView<'_> {
        let indices = self.layout_tree.box_at(box_idx).and_then(|b| b.style()).unwrap_or_else(|| self.styles.default_indices());
        self.style_for_indices(indices)
    }

    fn style_for_dom_node(&self, node_id: DomNodeId) -> StyleView<'_> {
        let indices = match self.document.node_ref(node_id) {
            Some(NodeRef::Element(element)) => self.styles.style_for_node(element.node_id()).unwrap_or_else(|| self.styles.default_indices()),
            Some(NodeRef::Text(text)) => text.parent().and_then(|parent_idx| self.styles.style_for_node(parent_idx)).unwrap_or_else(|| self.styles.default_indices()),
            None => self.styles.default_indices(),
        };
        self.style_for_indices(indices)
    }

    fn style_for_indices(&self, indices: StyleIndices) -> StyleView<'_> {
        self.styles.view(indices).expect("validated style handle")
    }

    fn display_for_element(&self, element: ElementRef<'_>) -> Display {
        self.styles.style_for_node(element.node_id()).map(|style| self.display_for_style(style)).unwrap_or(Display::Block)
    }

    fn display_for_style(&self, style: StyleIndices) -> Display {
        self.styles.box_model_style(style).expect("validated style handle").display
    }

    fn preferred_width_for_style(&self, style: Option<StyleIndices>) -> PreferredSize {
        style.map(|style| self.styles.box_model_style(style).expect("validated style handle").width).unwrap_or(PreferredSize::Auto)
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

    fn push_layout_box(&mut self, dom_node_id: DomNodeId, parent: Option<u32>, style: Option<StyleIndices>, layout_mode: LayoutMode) -> u32 {
        debug_assert!(self.document.element_ref(dom_node_id).is_some(), "layout boxes are only created for element nodes");
        let idx = self.layout_tree.push_box(LayoutBox { size: Size::ZERO, point: Point::ZERO, layout_mode, dom_element: Some(dom_node_id.raw()), parent, style });

        idx
    }

    fn is_block_level_child_display(&self, display: Display) -> bool {
        matches!(display, Display::Block | Display::ListItem | Display::Table | Display::InlineTable | Display::TableRow | Display::TableCell)
    }

    fn starts_new_block_child(&self, display: Display) -> bool {
        self.is_block_level_child_display(display)
    }
}

fn parse_dom_span(document: &Document, node_id: DomNodeId) -> usize {
    document.get_dom_attr(node_id, "span").and_then(|s| s.trim().parse::<usize>().ok()).filter(|&v| v > 0).unwrap_or(1)
}

pub fn build_layout_inputs(document: &Document, styles: &ComputedStyles, layout_tree: &mut LayoutTree, inline_content: &mut InlineContent) {
    LayoutTreeBuilder::new(document, styles, layout_tree, inline_content).build();
}

#[cfg(test)]
mod tests {
    use crate::document::{Children, GlyphId, InlineKind, LayoutMode};
    use crate::parser::DocumentFactory;

    #[test]
    fn list_style_resolves_and_inherits_by_nesting() {
        use crate::document::{ListStyleType, PreferredSize};
        let html = "<html><body><ul><li>a<ul><li>b<ul><li>c</li></ul></li></ul></li></ul><ol><li>d</li></ol></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        let li_type = |n: usize| -> ListStyleType {
            let mut seen = 0;
            for i in 0..document.box_count() {
                if document.get_tag(i).eq_ignore_ascii_case("li") {
                    if seen == n {
                        let si = document.box_style_indices(i).expect("li style");
                        return document.style_view(si).list_style_type();
                    }
                    seen += 1;
                }
            }
            panic!("li #{n} not found");
        };

        assert_eq!(li_type(0), ListStyleType::Disc);
        assert_eq!(li_type(1), ListStyleType::Circle);
        assert_eq!(li_type(2), ListStyleType::Square);
        assert_eq!(li_type(3), ListStyleType::Decimal);

        let ul_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("ul")).expect("ul box");
        let si = document.box_style_indices(ul_idx).expect("ul style");
        let pad_left = document.style_view(si).padding_left();
        assert!(matches!(pad_left, crate::document::LengthPct::Px(px) if (px - 40.0).abs() < 0.01), "ul padding-left should be 40px, got {pad_left:?}");
        let _ = PreferredSize::Auto;
    }

    #[test]
    fn br_generates_inline_break_run() {
        let html = "<html><body><p>a<br>b</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        assert!(document.inline_content().inline_runs().iter().any(|run| matches!(run.kind, InlineKind::Break)));
    }

    #[test]
    fn floated_inline_span_becomes_float_anchor_run() {
        let html = "<html><body><p><span style=\"float:left\">T</span>ext</p></body></html>";
        let mut factory = DocumentFactory::new();
        let document = factory.parse_with_new_pipeline(html, None);

        assert!(document.inline_content().inline_runs().iter().any(|run| matches!(run.kind, InlineKind::FloatAnchor { .. })));

        let span_idx = (0..document.box_count()).find(|&i| document.get_tag(i).eq_ignore_ascii_case("span")).expect("float span box should exist");
        let box_mode = document.box_at(span_idx).map(|box_| box_.layout_mode()).expect("float span box should exist");
        assert!(matches!(box_mode, LayoutMode::Block(_)), "floated inline span should stay a block box");
        let style = document.box_at(span_idx).and_then(|box_| box_.style()).expect("float span should have style");
        assert!(matches!(document.styles().box_model_style(style).expect("validated style handle").float, crate::document::Float::Left));
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
        for run in &document.inline_content().inline_runs()[runs.start as usize..runs.end as usize] {
            if let InlineKind::Text { glyphs } = &run.kind {
                if document.inline_content().glyphs()[glyphs.start as usize..glyphs.end as usize].iter().any(|&glyph| glyph == ' ' as GlyphId) {
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
            .inline_content()
            .inline_runs()
            .iter()
            .filter_map(|run| match &run.kind {
                InlineKind::Text { glyphs } => Some(document.inline_content().glyphs()[glyphs.start as usize..glyphs.end as usize].iter().map(|&glyph| char::from_u32(glyph as u32).expect("glyph should be valid char")).collect::<String>()),
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

        let display = document.box_at(table_idx).and_then(|b| b.style()).map(|style| document.styles().box_model_style(style).expect("validated style handle").display);
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
