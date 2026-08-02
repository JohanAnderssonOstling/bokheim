use html_dom::{Document, ElementRef, MemoryUsageReport};
use html_style_model::{ComputedStyles, ListStylePosition, PreferredSize, StyleIndices};
use kurbo::{Point, Size};
use rustc_data_structures::fx::FxHashMap;
use std::ops::Range;

/// A list item's generated marker (`::marker`). `marker_box` is a real layout
/// box holding the marker text run; it is laid out and positioned by the engine
/// relative to the list item's first line rather than flowing as a child, so it
/// is kept out of the item's `Children`.
#[derive(Clone, Copy)]
pub struct ListItemMarker {
    pub marker_box: u32,
    pub position: ListStylePosition,
}

impl ListItemMarker {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        MemoryUsageReport::new()
    }
}

/// Output of the box-construction phase: the box tree plus the inline text
/// content it references (glyph codepoints + inline runs). Produced by
/// `build_layout_inputs`, consumed read-only by shaping and layout.
#[derive(Default, Clone)]
pub struct LayoutTree {
    pub(super) boxes: Vec<LayoutBox>,
    pub(super) root_box: Option<u32>,
    /// Generated markers keyed by their list-item box index.
    pub(super) list_markers: FxHashMap<u32, ListItemMarker>,
}

impl LayoutTree {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<LayoutBox>("LayoutTree.boxes.storage", self.boxes.capacity(), self.boxes.len());
        for layout_box in &self.boxes {
            report.extend_prefixed("LayoutTree.boxes", layout_box.memory_usage_report());
        }
        report.add_slice_storage::<(u32, ListItemMarker)>("LayoutTree.list_markers.storage", self.list_markers.capacity(), self.list_markers.len());
        report
    }
}

impl LayoutTree {
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

    pub fn print_box_tree(&self, doc: &Document, styles: &ComputedStyles) {
        if !self.boxes.is_empty() {
            self.print_box_node(doc, styles, 0, 0);
        }
    }

    fn print_box_node(&self, doc: &Document, styles: &ComputedStyles, idx: usize, depth: usize) {
        let indent = "  ".repeat(depth);
        let b = &self.boxes[idx];
        let tag = b.get_element(doc).map(|element| element.tag()).unwrap_or("");
        let dom_id = b.dom_element().map(|i| i.to_string()).unwrap_or("-".into());
        let display = b.style().and_then(|indices| styles.box_model_style(indices)).map(|style| format!("{:?}", style.display)).unwrap_or("none".into());
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
                        self.print_box_node(doc, styles, i as usize, depth + 1);
                    }
                }
            }
            LayoutMode::Table(tb) => {
                for &i in &tb.rows {
                    self.print_box_node(doc, styles, i as usize, depth + 1);
                }
            }
            LayoutMode::TableRow(tr) => {
                for &i in &tr.cells {
                    self.print_box_node(doc, styles, i as usize, depth + 1);
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

    pub fn set_list_marker(&mut self, list_item_box: u32, marker: ListItemMarker) {
        self.list_markers.insert(list_item_box, marker);
    }

    pub fn list_marker(&self, list_item_box: usize) -> Option<ListItemMarker> {
        self.list_markers.get(&(list_item_box as u32)).copied()
    }
}

#[derive(Clone)]
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

impl LayoutBox {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.extend_prefixed("LayoutBox.layout_mode", self.layout_mode.memory_usage_report());
        report
    }
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
    pub fn get_element<'a>(&self, doc: &'a Document) -> Option<ElementRef<'a>> {
        self.dom_element.and_then(|idx| doc.node_id_from_raw(idx)).and_then(|node_id| doc.element_ref(node_id))
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

#[derive(Clone)]
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

impl LayoutMode {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        match self {
            LayoutMode::Block(block) => report.extend_prefixed("LayoutMode::Block", block.memory_usage_report()),
            LayoutMode::Table(table) => report.extend_prefixed("LayoutMode::Table", table.memory_usage_report()),
            LayoutMode::TableRow(row) => report.extend_prefixed("LayoutMode::TableRow", row.memory_usage_report()),
            LayoutMode::TableCell(cell) => report.extend_prefixed("LayoutMode::TableCell", cell.memory_usage_report()),
            LayoutMode::Inline(_) => {}
            LayoutMode::Anonymous(_) => {}
        }
        report
    }
}

#[derive(Clone)]
pub struct BlockBox {
    pub children: Children,
}

impl BlockBox {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        self.children.memory_usage_report()
    }
}

#[derive(Clone)]
pub struct TableBox {
    pub rows: Vec<u32>,
    pub captions_top: Vec<u32>,
    pub captions_bottom: Vec<u32>,
    pub column_width_hints: Vec<PreferredSize>,
}

impl TableBox {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<u32>("TableBox.rows.storage", self.rows.capacity(), self.rows.len());
        report.add_slice_storage::<u32>("TableBox.captions_top.storage", self.captions_top.capacity(), self.captions_top.len());
        report.add_slice_storage::<u32>("TableBox.captions_bottom.storage", self.captions_bottom.capacity(), self.captions_bottom.len());
        report.add_slice_storage::<PreferredSize>("TableBox.column_width_hints.storage", self.column_width_hints.capacity(), self.column_width_hints.len());
        report
    }
}

#[derive(Clone)]
pub struct TableRowBox {
    pub cells: Vec<u32>,
}

impl TableRowBox {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<u32>("TableRowBox.cells.storage", self.cells.capacity(), self.cells.len());
        report
    }
}

#[derive(Clone)]
pub struct TableCellBox {
    pub children: Children,
}

impl TableCellBox {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        self.children.memory_usage_report()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Children {
    Blocks(Vec<u32>),
    Runs(Range<u32>),
    Empty,
}

impl Children {
    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        if let Children::Blocks(children) = self {
            report.add_slice_storage::<u32>("Children::Blocks.storage", children.capacity(), children.len());
        }
        report
    }
}
