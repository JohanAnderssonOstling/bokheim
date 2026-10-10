//! The floating note editor an annotation opens, and where it is placed.
//!
//! Both paginated readers put the same panel over the same kind of layout: a
//! document split into equal columns beneath a toolbar, with an optional
//! sidebar to its left. The panel is centred on the column that was clicked
//! and takes whichever vertical side has more room. Each reader implements
//! the operations for saving and dismissing its editor.

use gpui::prelude::*;

use gpui::{AnyElement, Context, Entity, Pixels, Point, SharedString, Window, px};
use gpui_component::input::{Input, InputState};
use ui_components as components;

use book_model::AnnotationStyle;
use library_backend::Annotation;

const WIDTH: f32 = 360.0;
const MAX_HEIGHT: f32 = 420.0;

/// Distance kept between the panel and the edges of the document area.
const INSET: f32 = 12.0;

/// Distance kept between the panel and the point it describes.
const GAP: f32 = 10.0;

pub(crate) trait AnnotationEditorTarget: Sized + 'static {
    fn save_annotation_changes(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>);
    fn dismiss_annotation_editor(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

/// Creates the note input used by every annotation editor.
///
/// Auto-grow rather than a fixed box: the field is as tall as what has been
/// written in it. Its ceiling is high enough never to be the binding one — the
/// panel reaches the room its anchor allows first, and scrolls there.
pub(crate) fn note_input(window: &mut Window, cx: &mut Context<InputState>) -> InputState {
    InputState::new(window, cx).placeholder("Add a note").auto_grow(components::READER_NOTE_MIN_ROWS, components::READER_NOTE_MAX_ROWS)
}

/// Where to place the panel, in pixels relative to the document area.
pub(crate) struct Geometry {
    left: f32,
    /// Offset from whichever edge the panel is anchored to, named by `above`.
    edge_offset: f32,
    max_height: f32,
    /// Whether the panel sits above the anchor rather than below it.
    above: bool,
}

/// The width the document occupies once the sidebar has taken its share.
pub(crate) fn document_width(viewport_width: f32, sidebar_width: f32) -> f32 {
    (viewport_width - sidebar_width).max(1.0)
}

/// The passage the panel is about, in window coordinates.
///
/// A box rather than a point: a selection is as tall as the lines it crosses,
/// and a panel placed against the pointer that made it covers everything above
/// that pointer. The panel is placed clear of the whole box, which is also what
/// lets it leave the quoted text out — the reader can see the passage.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Passage {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Passage {
    pub(crate) fn from_bounds(bounds: gpui::Bounds<Pixels>) -> Self {
        Self { left: f32::from(bounds.origin.x), right: f32::from(bounds.origin.x + bounds.size.width), top: f32::from(bounds.origin.y), bottom: f32::from(bounds.origin.y + bounds.size.height) }
    }

    /// A passage whose extent is unknown, standing on the point that opened the
    /// panel. Placement then clears that point rather than the whole passage,
    /// which is the old behaviour and the best a pointer position supports.
    pub(crate) fn from_point(point: Point<Pixels>) -> Self {
        Self { left: f32::from(point.x), right: f32::from(point.x), top: f32::from(point.y), bottom: f32::from(point.y) }
    }

    fn center_x(self) -> f32 {
        (self.left + self.right) / 2.0
    }
}

/// Places the panel clear of `passage`.
///
/// `passage` is in window coordinates, so the sidebar and toolbar are taken off
/// it to reach document coordinates. Without one — an annotation opened from
/// the sidebar rather than clicked — the panel goes in the first column, a
/// third of the way down.
///
/// The side with more room wins, and the panel is anchored to that edge of the
/// passage rather than to its middle: above means "ending `GAP` above the first
/// line", below means "starting `GAP` under the last".
pub(crate) fn geometry(passage: Option<Passage>, viewport_width: f32, viewport_height: f32, sidebar_width: f32, toolbar_height: f32, column_count: usize) -> Geometry {
    let document_width = document_width(viewport_width, sidebar_width);
    let document_height = (viewport_height - toolbar_height).max(1.0);
    let column_count = column_count.max(1) as f32;
    let column_width = document_width / column_count;

    let passage = passage.map(|passage| Passage { left: passage.left - sidebar_width, right: passage.right - sidebar_width, top: passage.top - toolbar_height, bottom: passage.bottom - toolbar_height }).unwrap_or(Passage {
        left: 0.0,
        right: column_width,
        top: document_height / 3.0,
        bottom: document_height / 3.0,
    });
    let center_x = passage.center_x().clamp(0.0, document_width - f32::EPSILON);
    let top = passage.top.clamp(0.0, document_height);
    let bottom = passage.bottom.clamp(top, document_height);

    let column = (center_x / column_width).floor().min(column_count - 1.0);
    let column_center = (column + 0.5) * column_width;
    let panel_width = WIDTH.min((document_width - INSET * 2.0).max(1.0));
    let maximum_left = (document_width - panel_width).max(0.0);
    let side_inset = INSET.min(maximum_left / 2.0);
    let left = (column_center - panel_width / 2.0).clamp(side_inset, (maximum_left - side_inset).max(side_inset));

    let room_above = (top - GAP - INSET).max(1.0);
    let room_below = (document_height - bottom - GAP - INSET).max(1.0);
    let above = room_above > room_below;
    Geometry { left, edge_offset: if above { document_height - top + GAP } else { bottom + GAP }, max_height: (if above { room_above } else { room_below }).min(MAX_HEIGHT), above }
}

/// One style choice in the editor's action rows.
struct StyleChoice {
    id: SharedString,
    label: SharedString,
    style: AnnotationStyle,
    /// Whether this is the state the annotation is already in. A style row that
    /// cannot say which style is on is a row of four questions.
    selected: bool,
}

/// One highlight colour. The swatch is the colour, so the name is only what the
/// tooltip says and what a screen reader hears.
struct ColorChoice {
    id: SharedString,
    color: u32,
    name: &'static str,
    selected: bool,
}

/// Everything the editor can do, grouped the way it is drawn.
///
/// Only the two choices. There is no Save — a note is written as it is typed
/// and committed when the panel closes — and no Close, because clicking the
/// book is how a reader puts the panel down. Deleting is the sidebar's, where
/// the annotation can be seen in the company of the others.
struct EditorChoices {
    styles: Vec<StyleChoice>,
    colors: Vec<ColorChoice>,
}

/// The common choices used by both readers. Keeping them here prevents
/// their labels, colours, or styles from drifting apart.
///
/// The colours come from [`ui_components::READER_ANNOTATION_COLORS`], which is
/// also what the swatch and the renderers read, so a colour is named in exactly
/// one place.
fn standard_choices(id_prefix: &'static str, current: &Annotation) -> EditorChoices {
    let id = |suffix: &str| format!("{id_prefix}-{suffix}");
    let styles = [(AnnotationStyle::Highlight, "Highlight", "highlight"), (AnnotationStyle::Underline, "Underline", "underline"), (AnnotationStyle::Squiggly, "Squiggly", "squiggly"), (AnnotationStyle::Strikethrough, "Strike", "strike")]
        .into_iter()
        .map(|(style, label, suffix)| {
            let selected = current.style == style;
            StyleChoice { id: id(suffix).into(), label: label.into(), style, selected }
        })
        .collect();
    let colors = components::READER_ANNOTATION_COLORS.iter().map(|&(name, color)| ColorChoice { id: id(&name.to_lowercase()).into(), color, name, selected: current.color == color }).collect();
    EditorChoices { styles, colors }
}

/// Builds the editor panel, absolutely placed by `geometry`.
///
/// Two rows, quietest last: how the passage is marked, and in which colour.
/// A click outside saves the note before dismissing the panel.
fn editor<C: AnnotationEditorTarget>(geometry: Geometry, note: &Entity<InputState>, choices: EditorChoices, target: &Entity<C>, theme: components::BrowserTheme) -> AnyElement {
    let dismiss_target = target.clone();
    let mut panel = components::reader_annotation_editor(Input::new(note).appearance(false).w_full(), theme)
        .absolute()
        .left(px(geometry.left))
        .max_h(px(geometry.max_height))
        .when(geometry.above, |panel| panel.bottom(px(geometry.edge_offset)))
        .when(!geometry.above, |panel| panel.top(px(geometry.edge_offset)))
        // The panel floats over the document, which treats a click as "put the
        // cursor here" and would clear the very selection being annotated.
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        // Anywhere else is "done": the note is committed and the panel goes.
        .on_mouse_down_out(move |_, window, cx| {
            dismiss_target.update(cx, |this, cx| {
                this.save_annotation_changes(None, None, cx);
                this.dismiss_annotation_editor(window, cx);
            });
        });
    let mut styles = components::action_row();
    for choice in choices.styles {
        let target = target.clone();
        styles = styles.child(components::reader_annotation_action_button(choice.id, choice.label, choice.selected, theme).on_click(move |_, _, cx| {
            target.update(cx, |this, cx| this.save_annotation_changes(Some(choice.style.clone()), None, cx));
        }));
    }
    panel = panel.child(styles);

    let mut colors = components::reader_annotation_color_group();
    for choice in choices.colors {
        let target = target.clone();
        colors = colors.child(components::reader_annotation_color_swatch(choice.id, choice.color, choice.name, choice.selected, theme).on_click(move |_, _, cx| {
            target.update(cx, |this, cx| this.save_annotation_changes(None, Some(choice.color), cx));
        }));
    }
    panel.child(colors).into_any_element()
}

/// Builds the standard editor and connects its format-specific effects.
///
/// Dismissal runs on a click outside the panel, and the note is written back
/// first: a reader who typed a note and turned the page has saved it, because
/// nothing in the panel ever asked them to.
pub(crate) fn standard_editor<C: AnnotationEditorTarget>(id_prefix: &'static str, target: &Entity<C>, annotation: &Annotation, geometry: Geometry, note: &Entity<InputState>, theme: components::BrowserTheme) -> AnyElement {
    editor(geometry, note, standard_choices(id_prefix, annotation), target, theme)
}
