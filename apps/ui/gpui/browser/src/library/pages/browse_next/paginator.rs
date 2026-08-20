use gpui::prelude::*;
use gpui::{AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Rems, Style, Window, div, point, px, relative, size};

type RenderChild = dyn Fn(&mut Window, &mut App) -> AnyElement;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum PaginatorSizing {
    /// Width divided by height, matching GPUI's `aspect_ratio` convention.
    AspectRatio(f32),
}

impl PaginatorSizing {
    fn height(self, width: Pixels) -> Pixels {
        match self {
            Self::AspectRatio(ratio) => px(f32::from(width) / ratio.max(f32::EPSILON)),
        }
    }
}

pub(super) struct PaginatorChild {
    sizing: PaginatorSizing,
    render: Box<RenderChild>,
}

impl PaginatorChild {
    pub(super) fn new(sizing: PaginatorSizing, render: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        Self { sizing, render: Box::new(render) }
    }
}

pub(super) struct Paginator {
    id: ElementId,
    children: Vec<PaginatorChild>,
    minimum_width: Rems,
    gap: Pixels,
    child_offset: usize,
}

impl Paginator {
    pub(super) fn new(id: impl Into<ElementId>, minimum_width: Rems, gap: Pixels, children: impl IntoIterator<Item = PaginatorChild>) -> Self {
        Self { id: id.into(), children: children.into_iter().collect(), minimum_width, gap, child_offset: 0 }
    }

    pub(super) fn child_offset(&self) -> usize {
        self.child_offset
    }

    pub(super) fn set_child_offset(&mut self, child_offset: usize) {
        self.child_offset = child_offset.min(self.children.len().saturating_sub(1));
    }

    fn columns(&self, available_width: Pixels, rem_size: Pixels) -> (usize, Pixels) {
        let available_width = f32::from(available_width).max(1.0);
        let minimum_width = f32::from(self.minimum_width.to_pixels(rem_size)).max(1.0);
        let gap = f32::from(self.gap).max(0.0);
        let columns = (((available_width + gap) / (minimum_width + gap)).floor() as usize).max(1).min(u16::MAX as usize);
        let child_width = px(((available_width - gap * columns.saturating_sub(1) as f32) / columns as f32).max(1.0));
        (columns, child_width)
    }

    fn build_assignment(&self, direction: AssignmentDirection, columns: usize, child_width: Pixels, available_height: Pixels) -> Vec<AssignedRow> {
        match direction {
            AssignmentDirection::Forward => self.build_forward_assignment(columns, child_width, available_height),
            AssignmentDirection::Backward => self.build_backward_assignment(columns, child_width, available_height),
        }
    }

    fn build_forward_assignment(&self, columns: usize, child_width: Pixels, available_height: Pixels) -> Vec<AssignedRow> {
        let mut next_child = self.child_offset.min(self.children.len());
        let mut raw_rows = Vec::new();
        let mut accepted = Vec::new();

        while next_child < self.children.len() {
            raw_rows.push(self.forward_source_row(&mut next_child, columns, child_width));
            let candidate = self.normalize(&raw_rows, columns, child_width);
            if !accepted.is_empty() && !self.assignment_fits(&candidate, child_width, available_height) {
                break;
            }
            accepted = candidate;
        }

        accepted
    }

    fn build_backward_assignment(&self, columns: usize, child_width: Pixels, available_height: Pixels) -> Vec<AssignedRow> {
        let mut previous_child = self.child_offset.saturating_add(1).min(self.children.len());
        let mut raw_rows = Vec::new();
        let mut accepted = Vec::new();

        while previous_child > 0 {
            raw_rows.insert(0, self.backward_source_row(&mut previous_child, columns, child_width));
            let candidate = self.normalize(&raw_rows, columns, child_width);
            if !accepted.is_empty() && !self.assignment_fits(&candidate, child_width, available_height) {
                break;
            }
            accepted = candidate;
        }

        accepted
    }

    fn forward_source_row(&self, next_child: &mut usize, columns: usize, child_width: Pixels) -> Vec<Option<usize>> {
        let mut row = vec![None; columns];
        let row_height = self.children[*next_child].sizing.height(child_width);

        for slot in &mut row {
            if *next_child >= self.children.len() || self.children[*next_child].sizing.height(child_width) != row_height {
                break;
            }
            *slot = Some(*next_child);
            *next_child += 1;
        }

        row
    }

    fn backward_source_row(&self, previous_child: &mut usize, columns: usize, child_width: Pixels) -> Vec<Option<usize>> {
        let mut row = vec![None; columns];
        let row_height = self.children[*previous_child - 1].sizing.height(child_width);

        for slot in row.iter_mut().rev() {
            if *previous_child == 0 {
                break;
            }
            let child_index = *previous_child - 1;
            if self.children[child_index].sizing.height(child_width) != row_height {
                break;
            }
            *slot = Some(child_index);
            *previous_child -= 1;
        }

        row
    }

    fn normalize(&self, source_rows: &[Vec<Option<usize>>], columns: usize, child_width: Pixels) -> Vec<AssignedRow> {
        let source_slots = source_rows.iter().flatten().copied().collect::<Vec<_>>();
        let mut output_slots = vec![None; source_slots.len()];
        let mut free_cursor = 0;
        let mut candidate_cursor = 0;
        let mut row_height = None;

        while let Some((candidate_slot, child_index)) = next_occupied(&source_slots, candidate_cursor) {
            let candidate_height = self.children[child_index].sizing.height(child_width);
            let column = free_cursor % columns;

            if column == 0 {
                row_height = Some(candidate_height);
            } else if row_height != Some(candidate_height) {
                free_cursor += columns - column;
                row_height = None;
                continue;
            }

            if free_cursor >= output_slots.len() {
                output_slots.resize(free_cursor + columns, None);
            }
            output_slots[free_cursor] = Some(child_index);
            free_cursor += 1;
            candidate_cursor = candidate_slot + 1;
        }

        output_slots
            .chunks(columns)
            .filter_map(|slots| {
                let items = slots
                    .iter()
                    .enumerate()
                    .filter_map(|(column, child_index)| child_index.map(|child_index| PageItem { child_index, column }))
                    .collect::<Vec<_>>();
                (!items.is_empty()).then_some(AssignedRow { items })
            })
            .collect()
    }

    fn assignment_fits(&self, rows: &[AssignedRow], child_width: Pixels, available_height: Pixels) -> bool {
        f32::from(self.assignment_height(rows, child_width)) <= f32::from(available_height).max(1.0)
    }

    fn assignment_height(&self, rows: &[AssignedRow], child_width: Pixels) -> Pixels {
        let row_heights = rows.iter().map(|row| self.children[row.items[0].child_index].sizing.height(child_width));
        let height = row_heights.map(f32::from).sum::<f32>() + f32::from(self.gap).max(0.0) * rows.len().saturating_sub(1) as f32;
        px(height)
    }

    fn coordinate_layout(&self, assignment: &[AssignedRow], child_width: Pixels, available_height: Pixels) -> PageLayout {
        let content_height = self.assignment_height(assignment, child_width);
        let top_inset = px(((f32::from(available_height) - f32::from(content_height)).max(0.0)) / 2.0);
        let rows = assignment
            .iter()
            .map(|row| LayoutRow {
                items: row.items.clone(),
                height: self.children[row.items[0].child_index].sizing.height(child_width),
            })
            .collect();
        PageLayout { child_width, top_inset, rows }
    }
}

fn next_occupied(slots: &[Option<usize>], cursor: usize) -> Option<(usize, usize)> {
    slots.iter().enumerate().skip(cursor).find_map(|(slot, child)| child.map(|child| (slot, child)))
}

impl IntoElement for Paginator {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Paginator {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, id: Option<&GlobalElementId>, _inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _layout: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        let (columns, child_width) = self.columns(bounds.size.width, window.rem_size());
        let layout = window.with_element_state(id.expect("paginator requires an element id"), |state: Option<AssignmentState>, _| {
            let direction = state
                .as_ref()
                .filter(|state| self.child_offset < state.child_offset)
                .map_or(AssignmentDirection::Forward, |_| AssignmentDirection::Backward);
            let candidate = self.build_assignment(direction, columns, child_width, bounds.size.height);
            let candidate_rows = candidate.len();
            let mut state = state.unwrap_or_else(|| AssignmentState {
                child_offset: self.child_offset,
                columns,
                rows: candidate_rows,
                assignment: candidate.clone(),
            });

            if state.child_offset != self.child_offset || state.columns != columns || state.rows != candidate_rows {
                state.child_offset = self.child_offset;
                state.columns = columns;
                state.rows = candidate_rows;
                state.assignment = candidate;
            }

            let layout = self.coordinate_layout(&state.assignment, child_width, bounds.size.height);
            (layout, state)
        });
        let gap = self.gap;
        let mut row_top = bounds.top() + layout.top_inset;
        let mut elements = Vec::new();

        for row in layout.rows {
            for item in row.items {
                let child = &self.children[item.child_index];
                let origin = point(bounds.left() + (layout.child_width + gap) * item.column as f32, row_top);
                let mut element = div()
                    .w(layout.child_width)
                    .h(row.height)
                    .child((child.render)(window, cx))
                    .into_any_element();
                element.prepaint_as_root(origin, size(AvailableSpace::Definite(layout.child_width), AvailableSpace::Definite(row.height)), window, cx);
                elements.push(element);
            }
            row_top += row.height + gap;
        }

        elements
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector_id: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _layout: &mut Self::RequestLayoutState, elements: &mut Self::PrepaintState, window: &mut Window, cx: &mut App) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for element in elements {
                element.paint(window, cx);
            }
        });
    }
}

#[derive(Clone, Copy)]
enum AssignmentDirection {
    Forward,
    Backward,
}

struct AssignmentState {
    child_offset: usize,
    columns: usize,
    rows: usize,
    assignment: Vec<AssignedRow>,
}

#[derive(Clone)]
struct AssignedRow {
    items: Vec<PageItem>,
}

#[derive(Clone)]
struct PageItem {
    child_index: usize,
    column: usize,
}

struct LayoutRow {
    items: Vec<PageItem>,
    height: Pixels,
}

struct PageLayout {
    child_width: Pixels,
    top_inset: Pixels,
    rows: Vec<LayoutRow>,
}
