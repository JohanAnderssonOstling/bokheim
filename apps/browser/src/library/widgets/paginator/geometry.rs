//! Pure paginator geometry. This module does not render or retain GPUI
//! elements; it turns explicit groups into stable canonical rows.
//!
//! Grid groups resolve a column count and shared width. Intrinsic groups pack
//! measured child widths in reading order with fixed gaps. Both use the same
//! row-height budget for pagination.

use super::slots::Slots;
use gpui::{Pixels, px};

use super::{AssignmentDirection, PaginatorData, PaginatorGroupPolicy};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GroupGeometry {
    pub(super) group_index: usize,
    pub(super) columns: usize,
    pub(super) child_width: Pixels,
    /// The resolved gap between two children.
    pub(super) column_gap: Pixels,
    /// The margin before the first child: zero for a left-aligned group, one
    /// gap for a stretched one, whose children already fill the rest.
    pub(super) leading: Pixels,
}

#[derive(Clone)]
pub(super) struct AssignedRow {
    pub(super) group_index: usize,
    pub(super) items: Vec<PageItem>,
    pub(super) child_width: Pixels,
    pub(super) column_gap: Pixels,
}

#[derive(Clone)]
pub(super) struct PageItem {
    pub(super) child_index: usize,
    pub(super) x: Pixels,
    pub(super) width: Pixels,
}

/// Fits one group into `available_width`.
///
/// The column count always comes from the base width, never from the width a
/// child ends up drawn at: stretching must not change how many fit, or it
/// would feed back on itself. Every group resolves its grid across the full
/// available width, regardless of its child count, so short and wrapped groups
/// share the same child widths and gaps for a given policy.
///
/// The result is one width per group per viewport, which is what lets row
/// heights — including aspect-ratio ones, which follow the stretched width —
/// be known without measuring anything.
pub(super) fn group_geometry(policy: PaginatorGroupPolicy, group_index: usize, available_width: Pixels, rem_size: Pixels) -> GroupGeometry {
    // The group's own margin comes off the top, so everything below — the column
    // count, the stretch, the ceiling — divides the width the children actually
    // get. Adding it afterwards would push the last child past the edge.
    let inset = f32::from(policy.edge_inset).max(0.0);
    let available = (f32::from(available_width) - inset * 2.0).max(1.0);
    // A child never exceeds the row it sits in, so a window narrower than one
    // declared width degrades to a single shrunken column rather than overflow.
    if policy.intrinsic {
        return GroupGeometry { group_index, columns: 1, child_width: px(available), column_gap: policy.column_gap, leading: px(inset) };
    }
    let base_width = f32::from(policy.width.width(rem_size)).max(1.0).min(available);
    let column_gap = f32::from(policy.column_gap);
    let fit_width = if policy.maximum_width.is_some() && !policy.inset_edges { available + column_gap } else { available - column_gap };
    let columns_at = |width: f32| ((fit_width / (width + column_gap)).floor() as usize).max(1).min(u16::MAX as usize);
    // Respect the group's minimum column count on narrow windows by shrinking
    // children only when their preferred widths cannot fit.
    let fit = columns_at(base_width).max(policy.minimum_columns);

    let columns = fit;
    let gap_count = if policy.inset_edges { columns + 1 } else { columns - 1 };
    let fitted_width = ((available - column_gap * gap_count as f32) / columns as f32).max(1.0);
    let maximum_width = policy.maximum_width.map(|width| f32::from(width.width(rem_size)).max(base_width));
    let child_width = maximum_width.map_or(fitted_width, |maximum| maximum.min(fitted_width));
    let column_gap = if policy.maximum_width.is_some() && gap_count > 0 && !policy.pack_start { ((available - child_width * columns as f32) / gap_count as f32).max(column_gap) } else { column_gap };
    let occupied = columns as f32 * child_width + (columns.saturating_sub(1)) as f32 * column_gap;
    let leading = if policy.inset_edges { ((available - occupied) / 2.0).max(column_gap) } else { 0.0 };
    GroupGeometry { group_index, columns, child_width: px(child_width), column_gap: px(column_gap), leading: px(leading + inset) }
}

pub(super) fn group_geometries(data: &PaginatorData, available_width: Pixels, rem_size: Pixels) -> Vec<GroupGeometry> {
    data.groups.iter().enumerate().map(|(group_index, group)| group_geometry(group.policy, group_index, available_width, rem_size)).collect()
}

/// Width used when the paginator must report a size before it has been laid
/// out. The widest declared child is the only sensible guess.
pub(super) fn fallback_width(data: &PaginatorData, rem_size: Pixels) -> Pixels {
    px(data.groups.iter().map(|group| f32::from(group.policy.width.width(rem_size))).fold(1.0, f32::max))
}

pub(super) fn next_forward_row(data: &PaginatorData, widths: &Slots<Pixels>, geometries: &[GroupGeometry], next_child: &mut usize) -> Option<AssignedRow> {
    if *next_child >= data.children.len() {
        return None;
    }
    let group_index = data.group_index_for_child(*next_child)?;
    let group = data.groups[group_index];
    let geometry = geometries[group_index];
    debug_assert_eq!(geometry.group_index, group_index);
    let row_start = *next_child;
    let mut row_end = row_start;
    let mut items = Vec::new();
    let mut x = geometry.leading;
    while row_end < group.end {
        let width = if group.policy.intrinsic { widths.get(row_end).copied().unwrap_or_default().max(px(1.0)).min(geometry.child_width) } else { geometry.child_width };
        if !items.is_empty() && (if group.policy.intrinsic { x + width > geometry.leading + geometry.child_width } else { items.len() >= geometry.columns }) {
            break;
        }
        items.push(PageItem { child_index: row_end, x, width });
        x += width + geometry.column_gap;
        row_end += 1;
    }
    *next_child = row_end;
    Some(AssignedRow { group_index, items, child_width: geometry.child_width, column_gap: geometry.column_gap })
}

/// Normalizes an exact child range forward. The start remains stable and only
/// the final row of each group can be incomplete.
pub(super) fn forward_rows_in_range(data: &PaginatorData, widths: &Slots<Pixels>, geometries: &[GroupGeometry], start: usize, end: usize) -> Vec<AssignedRow> {
    let end = end.min(data.children.len());
    let mut next_child = start.min(end);
    let mut rows = Vec::new();
    while next_child < end {
        let Some(mut row) = next_forward_row(data, widths, geometries, &mut next_child) else {
            break;
        };
        if let Some(first_after_range) = row.items.iter().position(|item| item.child_index >= end) {
            row.items.truncate(first_after_range);
            next_child = end;
        }
        if !row.items.is_empty() {
            rows.push(row);
        }
    }
    rows
}

/// The vertical gap separating two rows. Rows from different groups take the
/// larger of the two policies so neither group is crowded at the boundary.
pub(super) fn gap_between(data: &PaginatorData, previous: &AssignedRow, next: &AssignedRow) -> Pixels {
    px(f32::from(data.groups[previous.group_index].policy.row_gap).max(f32::from(data.groups[next.group_index].policy.row_gap)))
}

pub(super) fn page_rows(
    data: &PaginatorData, widths: &Slots<Pixels>, geometries: &[GroupGeometry], direction: AssignmentDirection, child_offset: usize, maximum_rows: Option<usize>, available_height: Pixels, rem_size: Pixels,
) -> Vec<AssignedRow> {
    match direction {
        AssignmentDirection::Forward => forward_page_rows(data, widths, geometries, child_offset, maximum_rows, available_height, rem_size),
        AssignmentDirection::Backward => backward_page_rows(data, widths, geometries, child_offset, maximum_rows, available_height, rem_size),
    }
}

fn forward_page_rows(data: &PaginatorData, widths: &Slots<Pixels>, geometries: &[GroupGeometry], child_offset: usize, maximum_rows: Option<usize>, available_height: Pixels, rem_size: Pixels) -> Vec<AssignedRow> {
    let mut next_child = child_offset.min(data.children.len());
    let mut rows = Vec::new();
    while maximum_rows.is_none_or(|maximum_rows| rows.len() < maximum_rows) {
        let Some(row) = next_forward_row(data, widths, geometries, &mut next_child) else {
            break;
        };
        let mut candidate = rows.clone();
        candidate.push(row);
        if rows_height(data, &candidate, rem_size) > available_height.max(px(1.0)) && !rows.is_empty() {
            break;
        }
        rows = candidate;
    }
    rows
}

/// The first child of a page that ends just before `page_end`.
///
/// Walks backward one group at a time. Uniform row heights determine how
/// many rows fit; intrinsic widths determine which children those rows hold.
fn backward_page_start(data: &PaginatorData, widths: &Slots<Pixels>, geometries: &[GroupGeometry], page_end: usize, maximum_rows: Option<usize>, available_height: Pixels, rem_size: Pixels) -> usize {
    let budget = f32::from(available_height).max(1.0);
    let mut start = page_end;
    let mut used = 0.0;
    let mut rows_taken = 0usize;
    let mut adjacent_group: Option<usize> = None;

    while start > 0 {
        let Some(group_index) = data.group_index_for_child(start - 1) else { break };
        let group = data.groups[group_index];
        let geometry = geometries[group_index];
        let columns = geometry.columns.max(1);
        let row_height = f32::from(group.policy.sizing.height(geometry.child_width, rem_size)).max(1.0);
        let row_gap = f32::from(group.policy.row_gap);
        // Joining this group to the rows already taken costs one gap, and rows
        // that straddle a group boundary take the larger of the two policies.
        let boundary_gap = adjacent_group.map_or(0.0, |next| f32::from(data.groups[next].policy.row_gap).max(row_gap));

        let available = budget - used - boundary_gap;
        let mut fit = if available < 0.0 { 0 } else { ((available + row_gap) / (row_height + row_gap)).floor() as usize };
        if let Some(maximum_rows) = maximum_rows {
            fit = fit.min(maximum_rows.saturating_sub(rows_taken));
        }

        let children_here = start - group.start;
        // Intrinsic rows are counted from the requested end using the same
        // width budget. The chosen range is then drawn in reading order.
        let mut starts = Vec::new();
        if group.policy.intrinsic {
            let mut end = start;
            while end > group.start && starts.len() <= fit.max(1) {
                let mut left = end;
                let mut occupied = Pixels::ZERO;
                while left > group.start {
                    let width = widths.get(left - 1).copied().unwrap_or_default().max(px(1.0)).min(geometry.child_width);
                    let next = occupied + width + if left == end { Pixels::ZERO } else { geometry.column_gap };
                    if left < end && next > geometry.child_width {
                        break;
                    }
                    occupied = next;
                    left -= 1;
                }
                starts.push(left);
                end = left;
            }
        }
        let need = if group.policy.intrinsic { starts.len() } else { children_here.div_ceil(columns) };
        let mut take = fit.min(need);
        if take == 0 {
            // A page is never empty, even when its only row is too tall.
            if rows_taken > 0 {
                break;
            }
            take = 1;
        }

        start = if group.policy.intrinsic { starts[take - 1] } else { start - (take * columns).min(children_here) };
        used += take as f32 * row_height + take.saturating_sub(1) as f32 * row_gap + boundary_gap;
        rows_taken += take;
        adjacent_group = Some(group_index);
        if take < need {
            break;
        }
    }

    start
}

/// Rows of the page that ends just before `child_offset`.
///
/// The start is counted backward, then the rows are packed forward from it.
/// Fixed columns use division; intrinsic groups greedily pack from the end.
/// Packing the resulting contiguous range forward needs no more rows, and
/// preserves reading order even when the first or last row is partial.
fn backward_page_rows(data: &PaginatorData, widths: &Slots<Pixels>, geometries: &[GroupGeometry], child_offset: usize, maximum_rows: Option<usize>, available_height: Pixels, rem_size: Pixels) -> Vec<AssignedRow> {
    let page_end = child_offset.saturating_add(1).min(data.children.len());
    let start = backward_page_start(data, widths, geometries, page_end, maximum_rows, available_height, rem_size);
    forward_rows_in_range(data, widths, geometries, start, page_end)
}

pub(super) fn rows_height(data: &PaginatorData, rows: &[AssignedRow], rem_size: Pixels) -> Pixels {
    let mut height = Pixels::ZERO;
    for (index, row) in rows.iter().enumerate() {
        height += data.groups[row.group_index].policy.sizing.height(row.child_width, rem_size);
        if let Some(next) = rows.get(index + 1) {
            height += gap_between(data, row, next);
        }
    }
    height
}

#[cfg(test)]
mod tests {
    use super::super::{PaginatorChild, PaginatorGroup, PaginatorSizing, PaginatorWidthPolicy};
    use super::*;

    fn intrinsic_data() -> PaginatorData {
        let mut policy = PaginatorGroupPolicy::new(PaginatorWidthPolicy::pixels(px(100.0)), PaginatorSizing::pixels(px(10.0)), px(0.0));
        policy.intrinsic = true;
        policy.column_gap = px(0.0);
        PaginatorData::new([PaginatorGroup::new(policy, (0..3).map(|_| PaginatorChild::new(|_, _, _| unreachable!())))])
    }

    fn child_indices(rows: Vec<AssignedRow>) -> Vec<Vec<usize>> {
        rows.into_iter().map(|row| row.items.into_iter().map(|item| item.child_index).collect()).collect()
    }

    #[test]
    fn independent_measurement_snapshots_pack_the_same_definitions_differently() {
        let data = intrinsic_data();
        let geometry = group_geometries(&data, px(100.0), px(16.0));
        let small = vec![px(60.0), px(30.0), px(60.0)].into();
        let large = vec![px(80.0), px(70.0), px(80.0)].into();
        assert_eq!(child_indices(forward_rows_in_range(&data, &small, &geometry, 0, 3)), vec![vec![0, 1], vec![2]]);
        assert_eq!(child_indices(forward_rows_in_range(&data, &large, &geometry, 0, 3)), vec![vec![0], vec![1], vec![2]]);
        assert_eq!(child_indices(forward_rows_in_range(&data, &small, &geometry, 0, 3)), vec![vec![0, 1], vec![2]]);
    }

    #[test]
    fn backward_pagination_uses_the_supplied_measurements() {
        let data = intrinsic_data();
        let geometry = group_geometries(&data, px(100.0), px(16.0));
        let widths = vec![px(60.0), px(60.0), px(40.0)].into();
        let backward = page_rows(&data, &widths, &geometry, AssignmentDirection::Backward, 2, Some(1), px(100.0), px(16.0));
        assert_eq!(child_indices(backward), vec![vec![1, 2]]);
        let forward = page_rows(&data, &widths, &geometry, AssignmentDirection::Forward, 0, Some(1), px(100.0), px(16.0));
        assert_eq!(child_indices(forward), vec![vec![0]]);
    }
}
