use crate::document::{BorderStyle, Children, InlineKind, LayoutMode, PreferredSize, VerticalAlignValue, WhiteSpace, resolve_preffered_size};
use crate::layout::{LayoutEngine, get_style};
use floem::kurbo::{Point, Size, Vec2};
use std::ops::Range;

#[derive(Clone, Copy)]
struct TableCellPlacement {
    cell_idx: usize,
    row: usize,
    col: usize,
    colspan: usize,
    rowspan: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CollapsedBorderAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy)]
struct CollapsedBorderSegment {
    axis: CollapsedBorderAxis,
    anchor: f64,
    start: f64,
    end: f64,
    width: f64,
    color: u32,
    priority: u8,
}

fn parse_span(value: Option<&str>) -> usize {
    value.and_then(|s| s.trim().parse::<usize>().ok()).filter(|&v| v > 0).unwrap_or(1)
}

fn same_segment(a: CollapsedBorderSegment, b: CollapsedBorderSegment) -> bool {
    a.axis == b.axis && (a.anchor - b.anchor).abs() <= 0.01 && (a.start - b.start).abs() <= 0.01 && (a.end - b.end).abs() <= 0.01
}

fn segment_wins(candidate: CollapsedBorderSegment, current: CollapsedBorderSegment) -> bool {
    candidate.priority > current.priority || (candidate.priority == current.priority && candidate.width > current.width + 0.01)
}

fn register_segment(segments: &mut Vec<CollapsedBorderSegment>, candidate: CollapsedBorderSegment) {
    if candidate.width <= 0.0 || candidate.end <= candidate.start {
        return;
    }
    if let Some(existing) = segments.iter_mut().find(|segment| same_segment(**segment, candidate)) {
        if segment_wins(candidate, *existing) {
            *existing = candidate;
        }
        return;
    }
    segments.push(candidate);
}

#[cfg(test)]
fn build_cell_placements(rows: &[Vec<(usize, usize, usize)>]) -> (Vec<TableCellPlacement>, usize) {
    let mut placements = Vec::new();
    let mut carry_rowspans: Vec<usize> = Vec::new();
    let mut max_columns = 0usize;

    for (row_idx, cells) in rows.iter().enumerate() {
        if row_idx > 0 {
            for span in &mut carry_rowspans {
                if *span > 0 {
                    *span -= 1;
                }
            }
            while matches!(carry_rowspans.last(), Some(0)) {
                carry_rowspans.pop();
            }
        }

        let mut col = 0usize;
        for &(cell_idx, colspan, rowspan) in cells {
            while col < carry_rowspans.len() && carry_rowspans[col] > 0 {
                col += 1;
            }

            let colspan = colspan.max(1);
            let rowspan = rowspan.max(1);
            let end_col = col + colspan;
            if carry_rowspans.len() < end_col {
                carry_rowspans.resize(end_col, 0);
            }
            for covered_col in col..end_col {
                carry_rowspans[covered_col] = carry_rowspans[covered_col].max(rowspan);
            }
            placements.push(TableCellPlacement { cell_idx, row: row_idx, col, colspan, rowspan });
            col = end_col;
            max_columns = max_columns.max(end_col);
        }
    }

    (placements, max_columns)
}

fn resolve_column_widths(min_widths: &[f64], max_widths: &[f64], preferred_widths: Option<&[f64]>, preferred_priorities: Option<&[u8]>, target_width: f64) -> Vec<f64> {
    if min_widths.is_empty() {
        return Vec::new();
    }

    let min_widths: Vec<f64> = min_widths.iter().map(|w| w.max(0.0)).collect();
    let max_widths: Vec<f64> = max_widths.iter().enumerate().map(|(i, w)| w.max(min_widths[i])).collect();

    let target = target_width.max(0.0);
    let min_total: f64 = min_widths.iter().sum();
    if target <= min_total {
        return min_widths;
    }

    // Preferred widths sit between min and max and reduce starvation of medium columns.
    let preferred_widths: Vec<f64> = if let Some(preferred) = preferred_widths {
        min_widths.iter().zip(max_widths.iter()).enumerate().map(|(i, (min_w, max_w))| preferred.get(i).copied().unwrap_or(*min_w).clamp(*min_w, *max_w)).collect()
    } else {
        min_widths.iter().zip(max_widths.iter()).map(|(min_w, max_w)| min_w + 0.35 * (max_w - min_w)).collect()
    };
    let preferred_total: f64 = preferred_widths.iter().sum();
    let max_total: f64 = max_widths.iter().sum();

    let preferred_priorities: Vec<u8> = preferred_priorities.map(|p| p.to_vec()).unwrap_or_else(|| vec![0; min_widths.len()]);

    if target <= preferred_total {
        let mut widths = min_widths.clone();
        let mut remaining = target - min_total;
        for priority in [2u8, 1, 0] {
            if remaining <= 0.0 {
                break;
            }
            remaining = distribute_to_targets(&mut widths, &preferred_widths, Some(&preferred_priorities), priority, remaining);
        }
        if remaining > 0.0 {
            let add = remaining / widths.len() as f64;
            for w in &mut widths {
                *w += add;
            }
        }
        return widths;
    }

    if target <= max_total {
        let mut widths = preferred_widths.clone();
        let remaining = target - preferred_total;
        distribute_extra_with_caps(&mut widths, &max_widths, &preferred_widths, remaining);
        return widths;
    }

    let mut widths = max_widths.clone();
    let extra = (target - max_total) / widths.len() as f64;
    for w in &mut widths {
        *w += extra;
    }
    widths
}

fn distribute_deficit(widths: &mut [f64], start: usize, end: usize, deficit: f64) {
    if deficit <= 0.0 || start >= end {
        return;
    }
    let count = end - start;
    if count == 0 {
        return;
    }
    let mut weight_sum = 0.0f64;
    for w in &widths[start..end] {
        weight_sum += (*w).max(1.0);
    }
    if weight_sum <= 0.0 {
        let add = deficit / count as f64;
        for w in &mut widths[start..end] {
            *w += add;
        }
        return;
    }
    for w in &mut widths[start..end] {
        let share = deficit * (w.max(1.0) / weight_sum);
        *w += share;
    }
}

fn distribute_surplus(widths: &mut [f64], mins: &[f64], start: usize, end: usize, surplus: f64) {
    if surplus <= 0.0 || start >= end {
        return;
    }
    let mut shrink_capacity = 0.0f64;
    for idx in start..end {
        shrink_capacity += (widths[idx] - mins[idx]).max(0.0);
    }
    if shrink_capacity <= 0.0 {
        return;
    }
    let shrink = surplus.min(shrink_capacity);
    for idx in start..end {
        let cap = (widths[idx] - mins[idx]).max(0.0);
        if cap > 0.0 {
            widths[idx] -= shrink * (cap / shrink_capacity);
        }
    }
}

fn distribute_extra_with_caps(widths: &mut [f64], max_widths: &[f64], weights: &[f64], extra: f64) {
    if extra <= 0.0 || widths.is_empty() {
        return;
    }

    let mut remaining = extra;
    let epsilon = 0.01f64;
    while remaining > epsilon {
        let mut active_weight = 0.0f64;
        for i in 0..widths.len() {
            if max_widths[i] > widths[i] + epsilon {
                active_weight += weights[i].max(1.0);
            }
        }
        if active_weight <= 0.0 {
            break;
        }

        let mut progressed = false;
        for i in 0..widths.len() {
            let cap = (max_widths[i] - widths[i]).max(0.0);
            if cap <= epsilon {
                continue;
            }
            let share = remaining * (weights[i].max(1.0) / active_weight);
            let add = share.min(cap);
            if add > epsilon {
                widths[i] += add;
                remaining -= add;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
}

fn distribute_to_targets(widths: &mut [f64], targets: &[f64], priorities: Option<&[u8]>, required_priority: u8, extra: f64) -> f64 {
    if extra <= 0.0 || widths.is_empty() {
        return 0.0;
    }

    let mut remaining = extra;
    let epsilon = 0.01f64;
    while remaining > epsilon {
        let mut active_weight = 0.0f64;
        for i in 0..widths.len() {
            if priorities.map(|p| p.get(i).copied().unwrap_or(0) == required_priority).unwrap_or(true) && targets[i] > widths[i] + epsilon {
                active_weight += (targets[i] - widths[i]).max(1.0);
            }
        }
        if active_weight <= 0.0 {
            break;
        }

        let mut progressed = false;
        for i in 0..widths.len() {
            if !priorities.map(|p| p.get(i).copied().unwrap_or(0) == required_priority).unwrap_or(true) {
                continue;
            }
            let cap = (targets[i] - widths[i]).max(0.0);
            if cap <= epsilon {
                continue;
            }
            let share = remaining * (cap.max(1.0) / active_weight);
            let add = share.min(cap);
            if add > epsilon {
                widths[i] += add;
                remaining -= add;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    remaining
}

impl LayoutEngine {
    pub(crate) fn layout_table(&mut self, table_box_idx: usize, rows: Vec<u32>, content_pos: Point, content_width: f64, parent_content_height: Option<f64>) -> Size {
        if rows.is_empty() {
            return Size::ZERO;
        }

        let row_cells: Vec<Vec<usize>> = rows
            .iter()
            .map(|&row_idx| match self.document.box_layout_mode(row_idx as usize) {
                Some(LayoutMode::TableRow(row)) => row.cells.iter().map(|&c| c as usize).collect(),
                _ => Vec::new(),
            })
            .collect();

        let mut placements = Vec::new();
        let mut carry_rowspans: Vec<usize> = Vec::new();
        let mut column_count = 0usize;
        for (row_idx, cells) in row_cells.iter().enumerate() {
            if row_idx > 0 {
                for span in &mut carry_rowspans {
                    if *span > 0 {
                        *span -= 1;
                    }
                }
                while matches!(carry_rowspans.last(), Some(0)) {
                    carry_rowspans.pop();
                }
            }

            let mut col = 0usize;
            for &cell_idx in cells {
                while col < carry_rowspans.len() && carry_rowspans[col] > 0 {
                    col += 1;
                }

                let (colspan, rowspan) = self.cell_spans(cell_idx);
                let end_col = col + colspan;
                if carry_rowspans.len() < end_col {
                    carry_rowspans.resize(end_col, 0);
                }
                for covered_col in col..end_col {
                    carry_rowspans[covered_col] = carry_rowspans[covered_col].max(rowspan);
                }
                placements.push(TableCellPlacement { cell_idx, row: row_idx, col, colspan, rowspan });
                col = end_col;
                column_count = column_count.max(end_col);
            }
            column_count = column_count.max(carry_rowspans.len());
        }
        if column_count == 0 {
            return Size::ZERO;
        }

        let (captions_top, captions_bottom, column_width_hints) = match self.document.box_layout_mode(table_box_idx) {
            Some(LayoutMode::Table(table)) => (table.captions_top.clone(), table.captions_bottom.clone(), table.column_width_hints.clone()),
            _ => (Vec::new(), Vec::new(), Vec::new()),
        };

        let mut caption_top_height = 0.0f64;
        for &caption_idx in &captions_top {
            let caption_idx = caption_idx as usize;
            if let Some(box_) = self.document.box_at_mut(caption_idx) {
                box_.set_point(Point::new(content_pos.x, content_pos.y + caption_top_height));
            }
            let caption_size = self.layout_box(caption_idx, content_width, parent_content_height, 0.0);
            caption_top_height += caption_size.height;
        }
        let table_content_pos = Point::new(content_pos.x, content_pos.y + caption_top_height);

        let mut min_widths = vec![0.0f64; column_count];
        let mut max_widths = vec![0.0f64; column_count];
        let table_style = get_style(&self.document, table_box_idx);
        let collapsed_borders = matches!(table_style.border_collapse(), crate::document::BorderCollapseMode::Collapse);
        let h_spacing = if collapsed_borders { 0.0 } else { table_style.border_spacing_horizontal().max(0.0) as f64 };
        let v_spacing = if collapsed_borders { 0.0 } else { table_style.border_spacing_vertical().max(0.0) as f64 };
        let usable_width = (content_width - h_spacing * (column_count as f64 + 1.0)).max(0.0);

        for (col, hint) in column_width_hints.iter().copied().enumerate().take(column_count) {
            let hinted = resolve_preffered_size(hint, 0.0, usable_width).max(0.0);
            if hinted > 0.0 {
                min_widths[col] = min_widths[col].max(hinted);
                max_widths[col] = max_widths[col].max(hinted);
            }
        }

        // 1) Seed intrinsic constraints from non-spanning cells.
        for (i, placement) in placements.iter().enumerate() {
            if placement.colspan != 1 {
                continue;
            }
            let col = placement.col;
            if col >= column_count {
                continue;
            }
            let (cell_min, cell_max) = self.box_intrinsic_widths(placement.cell_idx);
            min_widths[col] = min_widths[col].max(cell_min);
            max_widths[col] = max_widths[col].max(cell_max.max(cell_min));
        }

        // 2) Propagate constraints from spanning cells.
        for placement in &placements {
            if placement.colspan <= 1 {
                continue;
            }
            let (cell_min, cell_max) = self.box_intrinsic_widths(placement.cell_idx);
            let start = placement.col.min(column_count);
            let end = (placement.col + placement.colspan).min(column_count);
            if start >= end {
                continue;
            }

            let span_min_sum: f64 = min_widths[start..end].iter().sum();
            if span_min_sum < cell_min {
                distribute_deficit(&mut min_widths, start, end, cell_min - span_min_sum);
            }

            let target_max = cell_max.max(cell_min);
            let span_max_sum: f64 = max_widths[start..end].iter().sum();
            if span_max_sum < target_max {
                distribute_deficit(&mut max_widths, start, end, target_max - span_max_sum);
            }
            for col in start..end {
                if max_widths[col] < min_widths[col] {
                    max_widths[col] = min_widths[col];
                }
            }
        }
        // 3) Apply authored min/max constraints from CSS to the column constraint space.
        for placement in &placements {
            let start = placement.col.min(column_count);
            let end = (placement.col + placement.colspan).min(column_count);
            if start >= end {
                continue;
            }

            if let Some(min_hint) = self.box_min_width_hint(placement.cell_idx, content_width) {
                let current_min: f64 = min_widths[start..end].iter().sum();
                if current_min < min_hint {
                    distribute_deficit(&mut min_widths, start, end, min_hint - current_min);
                }
            }

            if let Some(max_hint) = self.box_max_width_hint(placement.cell_idx, content_width) {
                let current_max: f64 = max_widths[start..end].iter().sum();
                if current_max > max_hint {
                    distribute_surplus(&mut max_widths, &min_widths, start, end, current_max - max_hint);
                }
            }
        }

        let mut preferred_widths: Vec<f64> = min_widths.iter().zip(max_widths.iter()).map(|(min_w, max_w)| min_w + 0.35 * (max_w - min_w)).collect();
        let mut preferred_priorities = vec![0u8; column_count];

        // 4) Apply authored width hints (e.g. table-cell widths in %) as preferred constraints.
        for placement in &placements {
            let start = placement.col.min(column_count);
            let end = (placement.col + placement.colspan).min(column_count);
            if start >= end {
                continue;
            }
            let Some(preferred_hint) = self.preferred_box_width_hint(placement.cell_idx, content_width) else {
                continue;
            };
            let current_pref: f64 = preferred_widths[start..end].iter().sum();
            if current_pref < preferred_hint {
                distribute_deficit(&mut preferred_widths, start, end, preferred_hint - current_pref);
            } else if current_pref > preferred_hint {
                distribute_surplus(&mut preferred_widths, &min_widths, start, end, current_pref - preferred_hint);
            }

            if placement.colspan == 1 {
                let col = placement.col;
                preferred_priorities[col] = preferred_priorities[col].max(self.preferred_box_width_priority(placement.cell_idx));
            }
        }

        for col in 0..column_count {
            if max_widths[col] < min_widths[col] {
                max_widths[col] = min_widths[col];
            }
            preferred_widths[col] = preferred_widths[col].clamp(min_widths[col], max_widths[col]);
        }

        let column_widths = resolve_column_widths(&min_widths, &max_widths, Some(&preferred_widths), Some(&preferred_priorities), usable_width);
        let table_width: f64 = column_widths.iter().sum::<f64>() + h_spacing * (column_count as f64 + 1.0);

        let mut column_starts = vec![0.0f64; column_widths.len()];
        let mut cursor_x = h_spacing;
        for i in 0..column_widths.len() {
            column_starts[i] = cursor_x;
            cursor_x += column_widths[i] + h_spacing;
        }
        let mut row_heights = vec![0.0f64; rows.len()];
        let mut cell_heights = vec![0.0f64; placements.len()];
        let mut cell_baseline_offsets = vec![None; placements.len()];
        let lines_checkpoint = self.layout_cache().lines.len();
        let glyph_offsets_checkpoint = self.layout_cache().line_glyph_offsets.len();
        let decorations_checkpoint = self.layout_cache().decorations.len();
        let image_fragments_checkpoint = self.layout_cache().image_fragments.len();
        let image_fragments_by_line_checkpoint = self.layout_cache().image_fragments_by_line.len();

        let mut collapsed_segments = Vec::new();

        for (i, placement) in placements.iter().enumerate() {
            let span_width = self.table_span_width(&column_starts, &column_widths, h_spacing, placement.col, placement.colspan);
            let cell_line_checkpoint = self.layout_cache().lines.len();
            if let Some(box_) = self.document.box_at_mut(placement.cell_idx) {
                box_.set_point(Point::new(table_content_pos.x + column_starts[placement.col], table_content_pos.y + v_spacing));
            }
            let cell_size = self.layout_box(placement.cell_idx, span_width, parent_content_height, 0.0);
            if let Some(box_) = self.document.box_at_mut(placement.cell_idx) {
                let mut size = box_.size();
                size.width = span_width;
                box_.set_size(size);
            }
            cell_heights[i] = cell_size.height;
            cell_baseline_offsets[i] = self.first_line_baseline_offset(placement.cell_idx, cell_line_checkpoint);
        }

        for (i, placement) in placements.iter().enumerate() {
            if placement.rowspan == 1 {
                row_heights[placement.row] = row_heights[placement.row].max(cell_heights[i]);
            }
        }
        let mut row_baselines = vec![None::<f64>; rows.len()];
        for (i, placement) in placements.iter().enumerate() {
            if placement.rowspan != 1 {
                continue;
            }
            let Some(cell_baseline) = cell_baseline_offsets[i] else {
                continue;
            };
            if !self.cell_uses_baseline_alignment(placement.cell_idx) {
                continue;
            }
            row_baselines[placement.row] = Some(row_baselines[placement.row].map(|current| current.max(cell_baseline)).unwrap_or(cell_baseline));
        }

        for (i, placement) in placements.iter().enumerate() {
            if placement.rowspan <= 1 {
                continue;
            }
            let end_row = (placement.row + placement.rowspan).min(row_heights.len());
            let current: f64 = row_heights[placement.row..end_row].iter().sum();
            let deficit = (cell_heights[i] - current).max(0.0);
            if deficit > 0.0 {
                let add = deficit / (end_row - placement.row) as f64;
                for row_h in &mut row_heights[placement.row..end_row] {
                    *row_h += add;
                }
            }
        }

        // First pass only measures spanning constraints; discard emitted visual fragments
        // so table content is painted exactly once in the final positioned pass below.
        {
            let layout = self.document.layout_mut();
            layout.lines.truncate(lines_checkpoint);
            layout.line_glyph_offsets.truncate(glyph_offsets_checkpoint);
            layout.decorations.truncate(decorations_checkpoint);
            layout.image_fragments.truncate(image_fragments_checkpoint);
            layout.image_fragments_by_line.truncate(image_fragments_by_line_checkpoint);
        }

        let mut row_starts = vec![0.0f64; rows.len()];
        let mut cursor_y = v_spacing;
        for i in 0..rows.len() {
            row_starts[i] = cursor_y;
            cursor_y += row_heights[i] + v_spacing;
        }
        let table_height = cursor_y;

        for (row_idx, &row_box_idx) in rows.iter().enumerate() {
            let row_box_idx = row_box_idx as usize;
            if let Some(box_) = self.document.box_at_mut(row_box_idx) {
                box_.set_point(table_content_pos + Vec2::new(h_spacing, row_starts[row_idx]));
                box_.set_size(Size::new(table_width - 2.0 * h_spacing, row_heights[row_idx]));
            }
        }

        for (i, placement) in placements.iter().enumerate() {
            let span_width = self.table_span_width(&column_starts, &column_widths, h_spacing, placement.col, placement.colspan);
            let span_height = self.table_row_span_height(&row_starts, &row_heights, v_spacing, placement.row, placement.rowspan);

            if let Some(box_) = self.document.box_at_mut(placement.cell_idx) {
                box_.set_point(Point::new(table_content_pos.x + column_starts[placement.col], table_content_pos.y + row_starts[placement.row]));
            }
            let lines_checkpoint = self.layout_cache().lines.len();
            let decorations_checkpoint = self.layout_cache().decorations.len();
            let image_fragments_checkpoint = self.layout_cache().image_fragments.len();
            self.layout_box(placement.cell_idx, span_width, parent_content_height, 0.0);
            let natural_height = self.document.get_box_size(placement.cell_idx).height;
            let vertical_offset = self.table_cell_vertical_offset(placement.cell_idx, natural_height, span_height, row_baselines[placement.row], cell_baseline_offsets[i]);
            let cell_has_content = self.cell_has_visible_content(placement.cell_idx, lines_checkpoint, image_fragments_checkpoint);
            if vertical_offset.abs() > 0.01 {
                self.shift_box_children(placement.cell_idx, vertical_offset);
                {
                    let layout = self.document.layout_mut();
                    for line in &mut layout.lines[lines_checkpoint..] {
                        line.point.y += vertical_offset;
                    }
                    for decoration in &mut layout.decorations[decorations_checkpoint..] {
                        decoration.rect.y0 += vertical_offset;
                        decoration.rect.y1 += vertical_offset;
                    }
                    for fragment in &mut layout.image_fragments[image_fragments_checkpoint..] {
                        fragment.offset.y += vertical_offset;
                    }
                }
            }
            if let Some(box_) = self.document.box_at_mut(placement.cell_idx) {
                let mut size = box_.size();
                size.width = span_width;
                size.height = size.height.max(span_height);
                box_.set_size(size);
            }
            let cell_style = get_style(&self.document, placement.cell_idx);
            let hide_empty_borders = matches!(cell_style.empty_cells(), crate::document::EmptyCellsMode::Hide) && !cell_has_content;
            if !hide_empty_borders {
                if collapsed_borders {
                    self.emit_block_background(placement.cell_idx);
                    self.collect_collapsed_cell_borders(&mut collapsed_segments, placement.cell_idx, 2);
                } else {
                    self.emit_block_decorations(placement.cell_idx);
                }
            }
        }

        if collapsed_borders {
            self.collect_collapsed_cell_borders(&mut collapsed_segments, table_box_idx, 1);
            self.emit_collapsed_border_segments(&collapsed_segments);
        }

        let mut caption_bottom_height = 0.0f64;
        for &caption_idx in &captions_bottom {
            let caption_idx = caption_idx as usize;
            if let Some(box_) = self.document.box_at_mut(caption_idx) {
                box_.set_point(Point::new(content_pos.x, table_content_pos.y + table_height + caption_bottom_height));
            }
            let caption_size = self.layout_box(caption_idx, content_width, parent_content_height, 0.0);
            caption_bottom_height += caption_size.height;
        }

        Size::new(table_width.max(content_width), caption_top_height + table_height + caption_bottom_height)
    }

    pub(crate) fn layout_table_row(&mut self, cells: Vec<u32>, content_pos: Point, content_width: f64, parent_content_height: Option<f64>) -> Size {
        if cells.is_empty() {
            return Size::ZERO;
        }

        let column_width = (content_width / cells.len() as f64).max(0.0);
        let mut row_height = 0.0f64;

        for (column, &cell_idx) in cells.iter().enumerate() {
            let cell_idx = cell_idx as usize;
            let cell_point = content_pos + Vec2::new(column as f64 * column_width, 0.0);
            if let Some(box_) = self.document.box_at_mut(cell_idx) {
                box_.set_point(cell_point);
            }
            let cell_size = self.layout_box(cell_idx, column_width, parent_content_height, 0.0);
            row_height = row_height.max(cell_size.height);
        }

        for &cell_idx in &cells {
            let cell_idx = cell_idx as usize;
            if let Some(box_) = self.document.box_at_mut(cell_idx) {
                let mut size = box_.size();
                size.height = size.height.max(row_height);
                box_.set_size(size);
            }
        }

        Size::new(content_width.max(0.0), row_height)
    }

    fn box_intrinsic_widths(&self, box_idx: usize) -> (f64, f64) {
        let extras = self.horizontal_noncontent_width(box_idx);
        let (content_min, content_max) = match self.document.box_layout_mode(box_idx) {
            Some(LayoutMode::Block(block)) => self.children_intrinsic_widths(&block.children),
            Some(LayoutMode::TableCell(cell)) => self.children_intrinsic_widths(&cell.children),
            Some(LayoutMode::Inline(range)) | Some(LayoutMode::Anonymous(range)) => self.runs_intrinsic_widths(range.clone()),
            Some(LayoutMode::Table(table)) => {
                let mut min_w = 0.0f64;
                let mut max_w = 0.0f64;
                for &row_idx in &table.rows {
                    let (row_min, row_max) = self.box_intrinsic_widths(row_idx as usize);
                    min_w = min_w.max(row_min);
                    max_w = max_w.max(row_max);
                }
                (min_w, max_w)
            }
            Some(LayoutMode::TableRow(row)) => {
                let mut min_w = 0.0f64;
                let mut max_w = 0.0f64;
                for &cell_idx in &row.cells {
                    let (cell_min, cell_max) = self.box_intrinsic_widths(cell_idx as usize);
                    min_w += cell_min;
                    max_w += cell_max;
                }
                (min_w, max_w)
            }
            None => (0.0, 0.0),
        };
        (content_min + extras, content_max + extras)
    }

    fn collect_collapsed_cell_borders(&self, segments: &mut Vec<CollapsedBorderSegment>, box_idx: usize, priority: u8) {
        let style = get_style(&self.document, box_idx);
        let rect = {
            let b = self.document.box_at(box_idx).expect("layout box should exist");
            floem::kurbo::Rect::new(b.point().x, b.point().y, b.point().x + b.size().width, b.point().y + b.size().height)
        };
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }

        let edges = [
            (matches!(style.border_top_style(), BorderStyle::Solid), style.border_top_width() as f64, style.border_top_color(), CollapsedBorderAxis::Horizontal, rect.y0, rect.x0, rect.x1),
            (matches!(style.border_right_style(), BorderStyle::Solid), style.border_right_width() as f64, style.border_right_color(), CollapsedBorderAxis::Vertical, rect.x1, rect.y0, rect.y1),
            (matches!(style.border_bottom_style(), BorderStyle::Solid), style.border_bottom_width() as f64, style.border_bottom_color(), CollapsedBorderAxis::Horizontal, rect.y1, rect.x0, rect.x1),
            (matches!(style.border_left_style(), BorderStyle::Solid), style.border_left_width() as f64, style.border_left_color(), CollapsedBorderAxis::Vertical, rect.x0, rect.y0, rect.y1),
        ];

        for (enabled, width, color, axis, anchor, start, end) in edges {
            if !enabled || width <= 0.0 {
                continue;
            }
            register_segment(segments, CollapsedBorderSegment { axis, anchor, start, end, width, color, priority });
        }
    }

    fn emit_collapsed_border_segments(&mut self, segments: &[CollapsedBorderSegment]) {
        for segment in segments {
            let half = segment.width / 2.0;
            let rect = match segment.axis {
                CollapsedBorderAxis::Horizontal => floem::kurbo::Rect::new(segment.start, segment.anchor - half, segment.end, segment.anchor + half),
                CollapsedBorderAxis::Vertical => floem::kurbo::Rect::new(segment.anchor - half, segment.start, segment.anchor + half, segment.end),
            };
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                continue;
            }
            self.document.layout_mut().decorations.push(crate::document::DecorationFragment { rect, color: segment.color, is_inline: false });
        }
    }

    fn children_intrinsic_widths(&self, children: &Children) -> (f64, f64) {
        match children {
            Children::Runs(range) => self.runs_intrinsic_widths(range.clone()),
            Children::Blocks(indices) => {
                let mut min_w = 0.0f64;
                let mut max_w = 0.0f64;
                for &child_idx in indices {
                    let (child_min, child_max) = self.box_intrinsic_widths(child_idx as usize);
                    min_w = min_w.max(child_min);
                    max_w = max_w.max(child_max);
                }
                (min_w, max_w)
            }
            Children::Empty => (0.0, 0.0),
        }
    }

    fn runs_intrinsic_widths(&self, run_range: Range<u32>) -> (f64, f64) {
        let mut min_content = 0.0f64;
        let mut max_content = 0.0f64;
        let mut current_line = 0.0f64;
        let mut segment = 0.0f64;

        for run in &self.document.inline_runs()[run_range.start as usize..run_range.end as usize] {
            let style = get_style(&self.document, run.box_idx as usize);
            match &run.kind {
                InlineKind::Text { glyphs } => {
                    for glyph_idx in glyphs.clone() {
                        let glyph_index = self.document.glyphs()[glyph_idx as usize];
                        let c = self.glyph_cache.get_char(glyph_index);
                        let break_kind = match (style.white_space(), c) {
                            (WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine, '\n') => 2,
                            (WhiteSpace::NoWrap, _) | (_, '\u{00A0}') => 0,
                            (WhiteSpace::Pre, _) => 0,
                            (_, ' ' | '\t' | '\n') => 1,
                            _ => 0,
                        };
                        if break_kind == 2 {
                            min_content = min_content.max(segment);
                            max_content = max_content.max(current_line);
                            segment = 0.0;
                            current_line = 0.0;
                            continue;
                        }

                        let mut advance = self.glyph_cache.get(glyph_index).size().width;
                        advance += style.letter_spacing() as f64;
                        if c == ' ' {
                            advance += style.word_spacing() as f64;
                        }
                        current_line += advance;

                        if break_kind == 1 {
                            min_content = min_content.max(segment);
                            segment = 0.0;
                        } else {
                            segment += advance;
                        }
                    }
                }
                InlineKind::Image { image_idx } => {
                    let image = &self.document.images()[*image_idx as usize];
                    let (mut width, _) = image.display_size();
                    if width <= 0.0 {
                        width = 1.0;
                    }
                    current_line += width;
                    segment += width;
                }
                InlineKind::Break => {
                    min_content = min_content.max(segment);
                    max_content = max_content.max(current_line);
                    segment = 0.0;
                    current_line = 0.0;
                }
            }
        }

        min_content = min_content.max(segment);
        max_content = max_content.max(current_line);
        (min_content, max_content.max(min_content))
    }

    fn horizontal_noncontent_width(&self, box_idx: usize) -> f64 {
        let style = get_style(&self.document, box_idx);
        style.get_horizontal_margin_padding() + style.border_left_width() as f64 + style.border_right_width() as f64
    }

    fn cell_spans(&self, cell_idx: usize) -> (usize, usize) {
        let dom_idx = self.document.box_dom_element_idx(cell_idx).map(|d| d as usize);
        let colspan = parse_span(dom_idx.and_then(|d| self.document.get_dom_attr(d, "colspan")));
        let rowspan = parse_span(dom_idx.and_then(|d| self.document.get_dom_attr(d, "rowspan")));
        (colspan, rowspan)
    }

    fn preferred_box_width_hint(&self, box_idx: usize, table_content_width: f64) -> Option<f64> {
        let style = get_style(&self.document, box_idx);
        let extras = self.horizontal_noncontent_width(box_idx);
        match style.width() {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some((px as f64 + extras).max(0.0)),
            PreferredSize::Percent(pct) => Some((table_content_width * pct as f64 + extras).max(0.0)),
        }
    }

    fn preferred_box_width_priority(&self, box_idx: usize) -> u8 {
        let style = get_style(&self.document, box_idx);
        match style.width() {
            PreferredSize::Percent(_) => 2,
            PreferredSize::Px(_) => 1,
            PreferredSize::Auto => 0,
        }
    }

    fn box_min_width_hint(&self, box_idx: usize, table_content_width: f64) -> Option<f64> {
        let style = get_style(&self.document, box_idx);
        let extras = self.horizontal_noncontent_width(box_idx);
        match style.min_width() {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some((px as f64 + extras).max(0.0)),
            PreferredSize::Percent(pct) => Some((table_content_width * pct as f64 + extras).max(0.0)),
        }
    }

    fn box_max_width_hint(&self, box_idx: usize, table_content_width: f64) -> Option<f64> {
        let style = get_style(&self.document, box_idx);
        let extras = self.horizontal_noncontent_width(box_idx);
        match style.max_width() {
            PreferredSize::Auto => None,
            PreferredSize::Px(px) => Some((px as f64 + extras).max(0.0)),
            PreferredSize::Percent(pct) => Some((table_content_width * pct as f64 + extras).max(0.0)),
        }
    }

    fn table_span_width(&self, column_starts: &[f64], column_widths: &[f64], h_spacing: f64, col: usize, colspan: usize) -> f64 {
        let start = col.min(column_widths.len());
        let end = (col + colspan).min(column_widths.len());
        if start >= end {
            return 0.0;
        }
        let last = end - 1;
        let _ = h_spacing;
        (column_starts[last] + column_widths[last] - column_starts[start]).max(0.0)
    }

    fn table_row_span_height(&self, row_starts: &[f64], row_heights: &[f64], v_spacing: f64, row: usize, rowspan: usize) -> f64 {
        let start = row.min(row_heights.len());
        let end = (row + rowspan).min(row_heights.len());
        if start >= end {
            return 0.0;
        }
        let last = end - 1;
        let _ = v_spacing;
        (row_starts[last] + row_heights[last] - row_starts[start]).max(0.0)
    }

    fn table_cell_vertical_offset(&self, cell_idx: usize, natural_height: f64, span_height: f64, row_baseline: Option<f64>, cell_baseline: Option<f64>) -> f64 {
        let extra = (span_height - natural_height).max(0.0);
        let style = get_style(&self.document, cell_idx);
        match style.vertical_align() {
            VerticalAlignValue::Baseline | VerticalAlignValue::Sub | VerticalAlignValue::Super | VerticalAlignValue::Length(_) | VerticalAlignValue::Percent(_) | VerticalAlignValue::TextTop => match (row_baseline, cell_baseline) {
                (Some(row), Some(cell)) => (row - cell).max(0.0),
                _ => 0.0,
            },
            VerticalAlignValue::Bottom | VerticalAlignValue::TextBottom => extra,
            VerticalAlignValue::Middle => extra * 0.5,
            _ => 0.0,
        }
    }

    fn first_line_baseline_offset(&self, cell_idx: usize, checkpoint: usize) -> Option<f64> {
        self.layout_cache().lines[checkpoint..].iter().find(|line| line.point.y >= self.document.get_box_point(cell_idx).y - 0.01).map(|line| line.point.y + line.baseline - self.document.get_box_point(cell_idx).y)
    }

    fn cell_uses_baseline_alignment(&self, cell_idx: usize) -> bool {
        let style = get_style(&self.document, cell_idx);
        matches!(style.vertical_align(), VerticalAlignValue::Baseline | VerticalAlignValue::Sub | VerticalAlignValue::Super | VerticalAlignValue::Length(_) | VerticalAlignValue::Percent(_) | VerticalAlignValue::TextTop)
    }

    fn cell_has_visible_content(&self, cell_idx: usize, lines_checkpoint: usize, image_fragments_checkpoint: usize) -> bool {
        if self.layout_cache().lines.len() > lines_checkpoint || self.layout_cache().image_fragments.len() > image_fragments_checkpoint {
            return true;
        }
        match self.document.box_layout_mode(cell_idx) {
            Some(LayoutMode::TableCell(cell)) => !matches!(cell.children, Children::Empty),
            _ => false,
        }
    }

    fn shift_box_children(&mut self, box_idx: usize, dy: f64) {
        let children: Vec<u32> = match self.document.box_layout_mode(box_idx) {
            Some(LayoutMode::Block(block)) => match &block.children {
                Children::Blocks(indices) => indices.clone(),
                _ => Vec::new(),
            },
            Some(LayoutMode::Table(table)) => table.rows.clone(),
            Some(LayoutMode::TableRow(row)) => row.cells.clone(),
            Some(LayoutMode::TableCell(cell)) => match &cell.children {
                Children::Blocks(indices) => indices.clone(),
                _ => Vec::new(),
            },
            Some(LayoutMode::Inline(_)) | Some(LayoutMode::Anonymous(_)) | None => Vec::new(),
        };
        for child in children {
            self.shift_box_subtree(child as usize, dy);
        }
    }

    fn shift_box_subtree(&mut self, box_idx: usize, dy: f64) {
        if let Some(box_) = self.document.box_at_mut(box_idx) {
            let mut point = box_.point();
            point.y += dy;
            box_.set_point(point);
        }
        self.shift_box_children(box_idx, dy);
    }
}

#[cfg(test)]
mod tests {
    use super::{TableCellPlacement, build_cell_placements, resolve_column_widths};

    #[test]
    fn resolve_columns_uses_min_when_space_tight() {
        let widths = resolve_column_widths(&[100.0, 50.0], &[220.0, 90.0], None, None, 120.0);
        assert_eq!(widths, vec![100.0, 50.0]);
    }

    #[test]
    fn resolve_columns_interpolates_between_min_and_max() {
        let widths = resolve_column_widths(&[50.0, 50.0], &[200.0, 100.0], None, None, 200.0);
        assert!(widths[0] > widths[1]);
        assert!((widths.iter().sum::<f64>() - 200.0).abs() < 0.01);
        assert!(widths[0] >= 50.0 && widths[0] <= 200.0);
        assert!(widths[1] >= 50.0 && widths[1] <= 100.0);
    }

    #[test]
    fn resolve_columns_distributes_extra_after_max() {
        let widths = resolve_column_widths(&[40.0, 60.0], &[50.0, 70.0], None, None, 200.0);
        assert!((widths[0] - 90.0).abs() < 0.01);
        assert!((widths[1] - 110.0).abs() < 0.01);
    }

    #[test]
    fn placements_assign_columns_in_row_order() {
        let rows = vec![vec![(10usize, 2usize, 1usize), (11usize, 1usize, 2usize)], vec![(20usize, 1usize, 1usize), (21usize, 1usize, 1usize)]];
        let (placements, cols) = build_cell_placements(&rows);
        assert_eq!(cols, 3);
        assert_eq!(placements.len(), 4);
        let first: TableCellPlacement = placements[0];
        let second: TableCellPlacement = placements[1];
        let third: TableCellPlacement = placements[2];
        let fourth: TableCellPlacement = placements[3];
        assert_eq!((first.row, first.col, first.colspan, first.cell_idx), (0, 0, 2, 10));
        assert_eq!((second.row, second.col, second.rowspan, second.cell_idx), (0, 2, 2, 11));
        assert_eq!((third.row, third.col, third.cell_idx), (1, 0, 20));
        assert_eq!((fourth.row, fourth.col, fourth.cell_idx), (1, 1, 21));
    }

    #[test]
    fn placements_respect_rowspan_reserved_slots() {
        let rows = vec![vec![(10usize, 1usize, 2usize), (11usize, 1usize, 1usize)], vec![(20usize, 1usize, 1usize)]];
        let (placements, cols) = build_cell_placements(&rows);
        assert_eq!(cols, 2);
        assert_eq!(placements.len(), 3);
        assert_eq!((placements[0].row, placements[0].col), (0, 0));
        assert_eq!((placements[1].row, placements[1].col), (0, 1));
        assert_eq!((placements[2].row, placements[2].col), (1, 1));
    }
}
