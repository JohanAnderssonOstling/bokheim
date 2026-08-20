//! The floating note editor an annotation opens, and where it is placed.
//!
//! Both paginated readers put the same panel over the same kind of layout: a
//! document split into equal columns beneath a toolbar, with an optional
//! sidebar to its left. The panel is centred on the column that was clicked
//! and takes whichever vertical side has more room. Only the actions differ
//! between the readers, so they are supplied by the caller.

use gpui::prelude::*;
use gpui::{AnyElement, App, Entity, Pixels, Point, SharedString, Window, px};
use gpui_component::input::{Input, InputState};
use ui_components as components;

const WIDTH: f32 = 360.0;
const MAX_HEIGHT: f32 = 420.0;

/// Distance kept between the panel and the edges of the document area.
const INSET: f32 = 12.0;

/// Distance kept between the panel and the point it describes.
const GAP: f32 = 10.0;

/// Where to place the panel, in pixels relative to the document area.
pub(crate) struct Geometry {
    pub(crate) left: f32,
    /// Offset from whichever edge the panel is anchored to, named by `above`.
    pub(crate) edge_offset: f32,
    pub(crate) max_height: f32,
    /// Whether the panel sits above the anchor rather than below it.
    pub(crate) above: bool,
}

/// The width the document occupies once the sidebar has taken its share.
pub(crate) fn document_width(viewport_width: f32, sidebar_width: f32) -> f32 {
    (viewport_width - sidebar_width).max(1.0)
}

/// Places the panel for an annotation anchored at `anchor`.
///
/// `anchor` is in window coordinates, so the sidebar and toolbar are taken off
/// it to reach document coordinates. Without one — an annotation opened from
/// the sidebar rather than clicked — the panel goes in the first column, a
/// third of the way down.
pub(crate) fn geometry(anchor: Option<Point<Pixels>>, viewport_width: f32, viewport_height: f32, sidebar_width: f32, toolbar_height: f32, column_count: usize) -> Geometry {
    let document_width = document_width(viewport_width, sidebar_width);
    let document_height = (viewport_height - toolbar_height).max(1.0);
    let column_count = column_count.max(1) as f32;
    let column_width = document_width / column_count;

    let (anchor_x, anchor_y) = anchor.map(|point| (f32::from(point.x) - sidebar_width, f32::from(point.y) - toolbar_height)).unwrap_or((column_width / 2.0, document_height / 3.0));
    let anchor_x = anchor_x.clamp(0.0, document_width - f32::EPSILON);
    let anchor_y = anchor_y.clamp(0.0, document_height);
    let column = (anchor_x / column_width).floor().min(column_count - 1.0);
    let column_center = (column + 0.5) * column_width;
    let panel_width = WIDTH.min((document_width - INSET * 2.0).max(1.0));
    let maximum_left = (document_width - panel_width).max(0.0);
    let side_inset = INSET.min(maximum_left / 2.0);
    let left = (column_center - panel_width / 2.0).clamp(side_inset, (maximum_left - side_inset).max(side_inset));

    let room_above = (anchor_y - GAP - INSET).max(1.0);
    let room_below = (document_height - anchor_y - GAP - INSET).max(1.0);
    let above = room_above > room_below;
    Geometry {
        left,
        edge_offset: if above { document_height - anchor_y + GAP } else { anchor_y + GAP },
        max_height: (if above { room_above } else { room_below }).min(MAX_HEIGHT),
        above,
    }
}

/// One button in the editor's action rows.
pub(crate) struct Action {
    id: SharedString,
    label: SharedString,
    on_click: Box<dyn Fn(&mut Window, &mut App) + 'static>,
}

impl Action {
    pub(crate) fn new(id: impl Into<SharedString>, label: impl Into<SharedString>, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self { id: id.into(), label: label.into(), on_click: Box::new(on_click) }
    }
}

/// Builds the editor panel, absolutely placed by `geometry`.
///
/// `rows` are rendered in order as separate wrapping action rows, so a reader
/// that offers styles or colours can group them apart from save and delete.
pub(crate) fn editor(geometry: Geometry, quote: &str, note: &Entity<InputState>, rows: Vec<Vec<Action>>, theme: components::BrowserTheme) -> AnyElement {
    let mut panel = components::reader_annotation_editor(theme)
        .absolute()
        .left(px(geometry.left))
        .max_h(px(geometry.max_height))
        .when(geometry.above, |panel| panel.bottom(px(geometry.edge_offset)))
        .when(!geometry.above, |panel| panel.top(px(geometry.edge_offset)))
        // The panel floats over the document, which treats a click as "put the
        // cursor here" and would clear the very selection being annotated.
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(components::reader_state_quote(format!("“{quote}”")))
        .child(components::reader_annotation_note_editor(theme).child(Input::new(note).appearance(false).size_full()));
    for row in rows {
        let mut actions = components::reader_state_actions();
        for action in row {
            let on_click = action.on_click;
            actions = actions.child(components::reader_control_button(action.id, action.label, theme).on_click(move |_, window, cx| on_click(window, cx)));
        }
        panel = panel.child(actions);
    }
    panel.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;

    #[test]
    fn the_panel_centers_on_the_column_that_was_clicked() {
        let left = geometry(Some(point(px(100.0), px(200.0))), 1200.0, 800.0, 0.0, 58.0, 2);
        let right = geometry(Some(point(px(900.0), px(200.0))), 1200.0, 800.0, 0.0, 58.0, 2);

        assert_eq!(left.left, 120.0);
        assert_eq!(right.left, 720.0);
    }

    #[test]
    fn the_panel_takes_the_roomier_vertical_side() {
        let near_top = geometry(Some(point(px(120.0), px(180.0))), 1200.0, 800.0, 0.0, 58.0, 3);
        let near_bottom = geometry(Some(point(px(1020.0), px(700.0))), 1200.0, 800.0, 0.0, 58.0, 3);

        assert!(!near_top.above, "there is more room below an anchor near the top");
        assert!(near_bottom.above);
        assert!(near_top.max_height <= MAX_HEIGHT && near_bottom.max_height <= MAX_HEIGHT);
    }

    #[test]
    fn a_narrow_document_still_places_the_panel_inside_itself() {
        let narrow = geometry(Some(point(px(10.0), px(200.0))), 200.0, 800.0, 0.0, 58.0, 1);

        assert!(narrow.left >= 0.0, "the panel must not start left of the document");
        assert!(narrow.left <= 200.0, "nor beyond its right edge");
    }

    #[test]
    fn the_sidebar_and_toolbar_are_taken_off_a_window_anchor() {
        let with_chrome = geometry(Some(point(px(400.0), px(258.0))), 1200.0, 800.0, 200.0, 58.0, 1);
        let without_chrome = geometry(Some(point(px(200.0), px(200.0))), 1000.0, 742.0, 0.0, 0.0, 1);

        assert_eq!(with_chrome.left, without_chrome.left);
        assert_eq!(with_chrome.edge_offset, without_chrome.edge_offset);
    }

    #[test]
    fn an_unanchored_panel_lands_in_the_first_column() {
        let unanchored = geometry(None, 1200.0, 800.0, 0.0, 58.0, 3);
        let first_column = geometry(Some(point(px(200.0), px(58.0 + (800.0 - 58.0) / 3.0))), 1200.0, 800.0, 0.0, 58.0, 3);

        assert_eq!(unanchored.left, first_column.left);
    }
}
