//! The floating note editor an annotation opens, and where it is placed.
//!
//! Both paginated readers put the same panel over the same kind of layout: a
//! document split into equal columns beneath a toolbar, with an optional
//! sidebar to its left. The panel is centred on the column that was clicked
//! and takes whichever vertical side has more room. Each reader implements
//! the operations for saving and dismissing its editor.

use ui_text::text;

use gpui::prelude::*;

use gpui::{AnyElement, App, Context, Edges, Entity, Pixels, Point, ScrollHandle, Window, px};
use gpui_component::Disableable;
use gpui_component::input::{Enter as InputEnter, Input, InputState};
use ui_components as components;

use library_backend::Annotation;

const WIDTH: f32 = components::ANNOTATION_EDITOR_WIDTH;
const MAX_HEIGHT: f32 = components::ANNOTATION_EDITOR_MAX_HEIGHT;
const MIN_HEIGHT: f32 = 160.0;

/// Distance kept between the panel and the edges of the document area.
const INSET: f32 = 12.0;

/// Distance kept between the panel and the point it describes.
const GAP: f32 = 10.0;

pub(crate) use components::AnnotationEditorTarget;
pub(crate) use components::{EditorBook, RecurringEditorContext};

pub(crate) trait BookmarkEditorTarget: AnnotationEditorTarget {
    fn save_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

/// Creates the note input used by every annotation editor.
///
/// Auto-grow rather than a fixed box: the field is as tall as what has been
/// written in it. Its ceiling is high enough never to be the binding one — the
/// panel reaches the room its anchor allows first, and scrolls there.
pub(crate) fn note_input(window: &mut Window, cx: &mut Context<InputState>) -> InputState {
    components::annotation_note_input(window, cx)
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
/// System UI and keyboard insets bound the available space. If neither side
/// can fit an editor, it covers part of the passage within that visible space.
pub(crate) fn geometry(passage: Option<Passage>, viewport_width: f32, viewport_height: f32, sidebar_width: f32, toolbar_height: f32, column_count: usize, insets: Edges<Pixels>) -> Geometry {
    let document_width = document_width(viewport_width, sidebar_width);
    let document_height = (viewport_height - toolbar_height).max(1.0);
    let visible_top = (f32::from(insets.top) - toolbar_height).clamp(0.0, document_height);
    let visible_bottom = (document_height - f32::from(insets.bottom)).clamp(visible_top, document_height);
    let column_count = column_count.max(1) as f32;
    let column_width = document_width / column_count;

    let passage = passage.map(|passage| Passage { left: passage.left - sidebar_width, right: passage.right - sidebar_width, top: passage.top - toolbar_height, bottom: passage.bottom - toolbar_height }).unwrap_or(Passage {
        left: 0.0,
        right: column_width,
        top: visible_top + (visible_bottom - visible_top) / 3.0,
        bottom: visible_top + (visible_bottom - visible_top) / 3.0,
    });
    let center_x = passage.center_x().clamp(0.0, document_width - f32::EPSILON);
    let top = passage.top.clamp(visible_top, visible_bottom);
    let bottom = passage.bottom.clamp(top, visible_bottom);

    let column = (center_x / column_width).floor().min(column_count - 1.0);
    let column_center = (column + 0.5) * column_width;
    let panel_width = WIDTH.min((document_width - INSET * 2.0).max(1.0));
    let maximum_left = (document_width - panel_width).max(0.0);
    let side_inset = INSET.min(maximum_left / 2.0);
    let left = (column_center - panel_width / 2.0).clamp(side_inset, (maximum_left - side_inset).max(side_inset));

    let room_above = (top - GAP - visible_top - INSET).max(1.0);
    let room_below = (visible_bottom - bottom - GAP - INSET).max(1.0);
    // A tall passage can leave no usable space on either side, especially
    // with the keyboard open. Keep the note editable even if the panel must
    // cover part of that passage. Its existing scroll surface handles overflow.
    if room_above.max(room_below) < MIN_HEIGHT {
        return Geometry { left, edge_offset: document_height - visible_bottom + INSET, max_height: (visible_bottom - visible_top - INSET * 2.0).clamp(1.0, MAX_HEIGHT), above: true };
    }
    let above = room_above > room_below;
    Geometry { left, edge_offset: if above { document_height - top + GAP } else { bottom + GAP }, max_height: (if above { room_above } else { room_below }).min(MAX_HEIGHT), above }
}

/// Builds the editor panel, absolutely placed by `geometry`.
///
/// Two rows, quietest last: how the passage is marked, and in which colour.
/// A click outside saves the note before dismissing the panel.
fn editor<C: AnnotationEditorTarget>(
    id_prefix: &'static str, annotation: &Annotation, geometry: Geometry, note: &Entity<InputState>, target: &Entity<C>, scroll: &ScrollHandle, extra: Option<AnyElement>, theme: components::BrowserTheme, window: &mut Window,
    cx: &mut Context<C>,
) -> AnyElement {
    let dismiss_target = target.clone();
    let controls = components::annotation_editor_controls(id_prefix, annotation, target, theme, window, cx);
    components::annotation_editor_panel(extra, Input::new(note).appearance(false).w_full(), controls, scroll, theme)
        .debug_selector(|| "reader-annotation-popup".to_owned())
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
            // Dismissing owns this press, so the book cannot register its
            // release as a page tap, follow a link, or reveal reader controls.
            cx.stop_propagation();
        })
        .into_any_element()
}

/// Builds the standard editor and connects its format-specific effects.
///
/// Dismissal runs on a click outside the panel, and the note is written back
/// first: a reader who typed a note and turned the page has saved it, because
/// nothing in the panel ever asked them to.
pub(crate) fn standard_editor<C: AnnotationEditorTarget>(
    id_prefix: &'static str, target: &Entity<C>, annotation: &Annotation, geometry: Geometry, note: &Entity<InputState>, theme: components::BrowserTheme, window: &mut Window, cx: &mut Context<C>,
) -> AnyElement {
    let scroll = window.use_keyed_state(id_prefix, cx, |_, _| ScrollHandle::default()).read(cx).clone();
    editor(id_prefix, annotation, geometry, note, target, &scroll, None, theme, window, cx)
}

pub(crate) fn recurring_editor<C: AnnotationEditorTarget>(
    id_prefix: &'static str, target: &Entity<C>, annotation: &Annotation, context: RecurringEditorContext, geometry: Geometry, note: &Entity<InputState>, theme: components::BrowserTheme, window: &mut Window, cx: &mut Context<C>,
) -> AnyElement {
    let extra = components::recurring_annotation_editor_controls(annotation, context, target, theme);
    let scroll = window.use_keyed_state(id_prefix, cx, |_, _| ScrollHandle::default()).read(cx).clone();
    editor(id_prefix, annotation, geometry, note, target, &scroll, Some(extra), theme, window, cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{AnnotationAnchor, AnnotationStyle};
    use content_address::BookId;
    use gpui::{Render, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, div, point};

    fn assert_panel_fits(geometry: &Geometry, document_height: f32, visible_top: f32, visible_bottom: f32) {
        let top = if geometry.above { document_height - geometry.edge_offset - geometry.max_height } else { geometry.edge_offset };
        let bottom = top + geometry.max_height;
        assert!(top >= visible_top, "panel top {top} must clear {visible_top}");
        assert!(bottom <= visible_bottom, "panel bottom {bottom} must clear {visible_bottom}");
        assert!(geometry.max_height >= MIN_HEIGHT, "the editor must remain usable");
    }

    #[test]
    fn editor_moves_above_keyboard_for_a_passage_below_it_and_restores_after_dismissal() {
        let passage = Some(Passage { left: 40.0, right: 200.0, top: 680.0, bottom: 720.0 });
        let before = geometry(passage, 400.0, 800.0, 0.0, 0.0, 1, Edges::default());
        let keyboard = Edges { top: px(24.0), bottom: px(320.0), ..Default::default() };
        let during = geometry(passage, 400.0, 800.0, 0.0, 0.0, 1, keyboard);
        assert_panel_fits(&during, 800.0, 24.0, 480.0);
        assert!(during.edge_offset > before.edge_offset);
        let after = geometry(passage, 400.0, 800.0, 0.0, 0.0, 1, Edges::default());
        assert_eq!(after.edge_offset, before.edge_offset);
        assert_eq!(after.max_height, before.max_height);
    }

    #[test]
    fn editor_below_passage_caps_long_notes_at_keyboard_edge() {
        let passage = Some(Passage { left: 40.0, right: 200.0, top: 40.0, bottom: 80.0 });
        let keyboard = Edges { top: px(24.0), bottom: px(320.0), ..Default::default() };
        let placement = geometry(passage, 400.0, 800.0, 0.0, 0.0, 1, keyboard);
        assert!(!placement.above);
        assert_panel_fits(&placement, 800.0, 24.0, 480.0);
        assert!(placement.max_height < MAX_HEIGHT);
    }

    #[test]
    fn editor_remains_usable_when_passage_spans_space_above_keyboard() {
        let passage = Some(Passage { left: 40.0, right: 200.0, top: 30.0, bottom: 720.0 });
        let keyboard = Edges { top: px(24.0), bottom: px(440.0), ..Default::default() };
        let placement = geometry(passage, 400.0, 800.0, 0.0, 0.0, 1, keyboard);
        assert_panel_fits(&placement, 800.0, 24.0, 360.0);
    }

    struct EditorTest {
        note: Entity<InputState>,
        annotation: Annotation,
        open: bool,
        above: bool,
        scroll: Option<ScrollHandle>,
        document_clicks: usize,
    }

    impl BookmarkEditorTarget for EditorTest {
        fn save_bookmark(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            let name = self.note.read(cx).value();
            if name.trim().is_empty() {
                return;
            }
            self.annotation.note = name.trim().to_owned();
            self.open = false;
            cx.notify();
        }
    }

    impl AnnotationEditorTarget for EditorTest {
        fn save_annotation_changes(&mut self, style: Option<AnnotationStyle>, color: Option<u32>, cx: &mut Context<Self>) {
            self.annotation.note = self.note.read(cx).value().to_string();
            if let Some(style) = style {
                self.annotation.style = style;
            }
            if let Some(color) = color {
                self.annotation.color = color;
            }
            cx.notify();
        }

        fn dismiss_annotation_editor(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            self.open = false;
            cx.notify();
        }

        fn delete_edited_annotation(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            self.annotation.deleted = true;
            self.open = false;
            cx.notify();
        }
    }

    impl Render for EditorTest {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let mut surface = components::reader_document_body().id("test-annotation-document").on_click(cx.listener(|this, _, _, _| this.document_clicks += 1)).child(div().id("highlight").w(px(80.0)).h(px(30.0)).on_click(cx.listener(
                |this, _, _, cx| {
                    this.open = true;
                    cx.notify();
                },
            )));
            if self.open {
                self.scroll = Some(window.use_keyed_state("test-editor", cx, |_, _| ScrollHandle::default()).read(cx).clone());
                let geometry = Geometry { left: 120.0, edge_offset: 100.0, max_height: 250.0, above: self.above };
                surface = surface.child(if self.annotation.is_bookmark() {
                    bookmark_editor(&cx.entity(), geometry, &self.note, components::browser_theme(cx), cx)
                } else {
                    standard_editor("test-editor", &cx.entity(), &self.annotation, geometry, &self.note, components::browser_theme(cx), window, cx)
                });
            }
            surface
        }
    }

    fn setup(cx: &mut TestAppContext, above: bool) -> (Entity<EditorTest>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        let (root, visual) = cx.add_window_view(|window, cx| {
            let editor = cx.new(|cx| {
                let note = cx.new(|cx| note_input(window, cx));
                crate::shell::bookmarks::watch_name_validity(&note, cx, |this: &EditorTest| this.annotation.is_bookmark());
                EditorTest {
                    note,
                    annotation: Annotation {
                        id: "test-highlight".to_owned(),
                        book_id: BookId::from_bytes([0; 32]),
                        anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2!/4/2/1:0)".to_owned()),
                        exact_text: "Highlighted text".to_owned(),
                        style: AnnotationStyle::Highlight,
                        color: components::READER_ANNOTATION_DEFAULT_COLOR,
                        note: String::new(),
                        created_at: 0,
                        toc_ordinal: None,
                        progress: None,
                        modified_at: 0,
                        deleted: false,
                    },
                    open: false,
                    above,
                    scroll: None,
                    document_clicks: 0,
                }
            });
            gpui_component::Root::new(editor, window, cx)
        });
        let editor = visual.update(|_, cx| root.read(cx).view().clone().downcast::<EditorTest>().ok().unwrap());
        (editor, visual)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn clicking_highlight_opens_visible_editor_and_outside_click_saves_note(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, false);
        draw(cx);
        cx.simulate_click(point(px(20.0), px(15.0)), Default::default());
        draw(cx);
        let bounds = cx.debug_bounds("reader-annotation-popup").unwrap();
        assert_eq!(bounds.left(), px(120.0));
        assert_eq!(bounds.top(), px(100.0));
        assert_eq!(bounds.size.width, px(WIDTH));
        assert!(bounds.size.height > px(80.0));
        assert!(bounds.size.height <= px(250.0));
        let styles = cx.debug_bounds("reader-annotation-style-row").unwrap();
        let colors = cx.debug_bounds("reader-annotation-color-row").unwrap();
        let note = cx.debug_bounds("reader-annotation-note-row").unwrap();
        assert_eq!(styles.left(), colors.left());
        assert_eq!(styles.right(), colors.right());
        assert_eq!(styles.left(), note.left());
        assert_eq!(styles.right(), note.right());
        assert_eq!(cx.debug_bounds("reader-annotation-editor-delete").unwrap().right(), styles.right());
        if cfg!(feature = "kobo") {
            assert!(cx.debug_bounds("reader-annotation-color-Yellow").is_none(), "monochrome Kobo has no color picker");
        } else {
            let yellow = cx.debug_bounds("reader-annotation-color-Yellow").unwrap();
            let orange = cx.debug_bounds("reader-annotation-color-Orange").unwrap();
            let red = cx.debug_bounds("reader-annotation-color-Red").unwrap();
            assert_eq!(yellow.left(), styles.left());
            // Evenly distributed controls can round to adjacent pixel positions.
            assert!(((orange.center().x - yellow.center().x) - (red.center().x - orange.center().x)).abs() <= px(1.0));
        }
        cx.simulate_click(bounds.origin + point(px(20.0), px(20.0)), Default::default());
        view.read_with(cx, |view, _| assert!(view.open));
        view.update_in(cx, |view, window, cx| view.note.update(cx, |input, cx| input.set_value("Saved note", window, cx)));
        draw(cx);
        let clicks_before_dismissal = view.read_with(cx, |view, _| view.document_clicks);
        cx.simulate_click(point(px(600.0), px(400.0)), Default::default());
        draw(cx);
        view.read_with(cx, |view, _| {
            assert!(!view.open);
            assert_eq!(view.annotation.note, "Saved note");
            assert_eq!(view.document_clicks, clicks_before_dismissal, "closing the editor must consume the document tap");
        });
        cx.simulate_click(point(px(600.0), px(400.0)), Default::default());
        draw(cx);
        view.read_with(cx, |view, _| assert_eq!(view.document_clicks, clicks_before_dismissal + 1, "subsequent book taps remain available"));
    }

    #[gpui::test]
    fn bookmark_popup_saves_when_save_is_pressed(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, false);
        view.update_in(cx, |view, window, cx| {
            view.annotation.id = "bookmark:test".into();
            view.annotation.exact_text.clear();
            view.open = true;
            view.note.update(cx, |input, cx| {
                input.set_placeholder("Name bookmark", window, cx);
                input.focus(window, cx);
            });
            cx.notify();
        });
        draw(cx);
        assert!(cx.debug_bounds("reader-bookmark-popup").is_some());
        assert!(cx.debug_bounds("reader-annotation-style-row").is_none());
        assert!(cx.debug_bounds("reader-annotation-color-row").is_none());
        assert!(cx.debug_bounds("reader-annotation-editor-delete").is_none());
        cx.simulate_input("My bookmark");
        draw(cx);
        view.read_with(cx, |view, _| {
            assert!(view.open);
            assert!(view.annotation.note.is_empty(), "typing does not save bookmarks");
        });
        let save = cx.debug_bounds("reader-bookmark-save").unwrap();
        cx.simulate_click(save.center(), Default::default());
        draw(cx);
        view.read_with(cx, |view, _| {
            assert!(!view.open);
            assert_eq!(view.annotation.note, "My bookmark");
        });
    }

    #[gpui::test]
    fn enter_saves_bookmark_without_inserting_a_newline(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, false);
        view.update_in(cx, |view, window, cx| {
            view.annotation.id = "bookmark:test".into();
            view.open = true;
            view.note.update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
        });
        draw(cx);
        cx.simulate_keystrokes("enter");
        view.read_with(cx, |view, cx| {
            assert!(view.open, "Enter cannot save an empty name");
            assert!(view.note.read(cx).value().is_empty());
        });
        cx.simulate_input("Saved with Enter");
        // Enter submits the original text regardless of the caret's position.
        cx.simulate_keystrokes("home enter");
        draw(cx);
        view.read_with(cx, |view, cx| {
            assert!(!view.open);
            assert_eq!(view.annotation.note, "Saved with Enter");
            assert_eq!(view.note.read(cx).value(), "Saved with Enter");
        });
    }

    #[gpui::test]
    fn dismissing_bookmark_editor_discards_unsaved_names(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, false);
        for (saved, escape) in [("", false), ("Existing bookmark", false), ("Existing bookmark", true)] {
            view.update_in(cx, |view, window, cx| {
                view.annotation.id = "bookmark:test".into();
                view.annotation.note = saved.to_owned();
                view.open = true;
                view.note.update(cx, |input, cx| {
                    input.set_placeholder("Name bookmark", window, cx);
                    input.set_value("", window, cx);
                    input.focus(window, cx);
                });
                cx.notify();
            });
            draw(cx);
            let save = cx.debug_bounds("reader-bookmark-save").unwrap();
            cx.simulate_click(save.center(), Default::default());
            view.read_with(cx, |view, _| assert!(view.open, "blank names cannot be saved"));
            cx.simulate_input("Unsaved name");
            if escape {
                cx.simulate_keystrokes("escape");
            } else {
                cx.simulate_click(point(px(600.0), px(400.0)), Default::default());
            }
            draw(cx);
            view.read_with(cx, |view, _| {
                assert!(!view.open);
                assert_eq!(view.annotation.note, saved);
            });
        }
    }

    #[gpui::test]
    fn editor_above_passage_has_height_and_keeps_bottom_anchor(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, true);
        view.update(cx, |view, cx| {
            view.open = true;
            cx.notify();
        });
        draw(cx);
        let bounds = cx.debug_bounds("reader-annotation-popup").unwrap();
        let viewport = cx.update(|window, _| window.viewport_size());
        assert_eq!(bounds.left(), px(120.0));
        assert_eq!(bounds.bottom(), viewport.height - px(100.0));
        assert!(bounds.size.height > px(80.0));
        assert!(bounds.size.height <= px(250.0));
    }

    #[gpui::test]
    fn short_note_does_not_scroll_when_editor_has_room(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, false);
        for note in ["", "A short note", "First line\nSecond line\nThird line"] {
            view.update_in(cx, |view, window, cx| {
                view.open = true;
                view.note.update(cx, |input, cx| input.set_value(note, window, cx));
                cx.notify();
            });
            draw(cx);
            let bounds = cx.debug_bounds("reader-annotation-popup").unwrap();
            assert!(bounds.size.height < px(250.0), "short notes must fit below the height limit");
            let scroll = view.read_with(cx, |view, _| view.scroll.as_ref().unwrap().clone());
            assert_eq!(scroll.max_offset().y, px(0.0), "a fitting note must not create panel overflow: {note:?}");
            let note_bounds = cx.debug_bounds("reader-annotation-note-row").unwrap();
            cx.simulate_event(ScrollWheelEvent { position: note_bounds.center(), delta: ScrollDelta::Pixels(point(px(0.0), px(-100.0))), ..Default::default() });
            draw(cx);
            assert_eq!(scroll.offset().y, px(0.0));
            view.read_with(cx, |view, cx| assert_eq!(view.note.read(cx).scroll_offset().y, px(0.0), "a fitting note must not scroll inside its field"));
        }
    }

    #[gpui::test]
    fn long_note_caps_editor_height_and_scrolls(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, false);
        view.update_in(cx, |view, window, cx| {
            view.open = true;
            view.note.update(cx, |input, cx| input.set_value("A line in a long annotation.\n".repeat(40), window, cx));
            cx.notify();
        });
        draw(cx);
        let bounds = cx.debug_bounds("reader-annotation-popup").unwrap();
        assert_eq!(bounds.size.height, px(250.0));
        let scroll = view.read_with(cx, |view, _| view.scroll.as_ref().unwrap().clone());
        assert!(scroll.max_offset().y > px(0.0));
        cx.simulate_event(ScrollWheelEvent { position: bounds.origin + point(px(5.0), px(50.0)), delta: ScrollDelta::Pixels(point(px(0.0), px(-100.0))), ..Default::default() });
        draw(cx);
        assert!(scroll.offset().y < px(0.0));
        view.read_with(cx, |view, _| assert!(view.open));
    }

    #[cfg(not(feature = "kobo"))]
    #[gpui::test]
    fn color_picker_selects_with_mouse_and_keyboard_without_losing_the_note(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx, false);
        view.update_in(cx, |view, window, cx| {
            view.open = true;
            view.note.update(cx, |input, cx| input.set_value("Keep this note", window, cx));
            cx.notify();
        });
        draw(cx);
        assert!(cx.debug_bounds("reader-annotation-color-selected-Yellow").is_some());
        let blue = cx.debug_bounds("reader-annotation-color-Blue").unwrap();
        cx.simulate_click(blue.center(), Default::default());
        draw(cx);
        assert!(cx.debug_bounds("reader-annotation-color-selected-Blue").is_some());
        assert!(cx.debug_bounds("reader-annotation-color-selected-Yellow").is_none());
        view.read_with(cx, |view, _| assert_eq!(view.annotation.color, components::READER_ANNOTATION_COLORS[5].1));
        cx.simulate_keystrokes("right");
        draw(cx);
        assert!(cx.debug_bounds("reader-annotation-color-selected-Teal").is_some());
        cx.simulate_keystrokes("end right");
        draw(cx);
        assert!(cx.debug_bounds("reader-annotation-color-selected-Yellow").is_some());
        cx.simulate_keystrokes("left");
        draw(cx);
        assert!(cx.debug_bounds("reader-annotation-color-selected-Green").is_some());
        cx.simulate_keystrokes("home enter");
        draw(cx);
        view.read_with(cx, |view, _| {
            assert!(view.open);
            assert_eq!(view.annotation.note, "Keep this note");
            assert_eq!(view.annotation.color, components::READER_ANNOTATION_DEFAULT_COLOR);
        });
        let delete = cx.debug_bounds("reader-annotation-editor-delete").unwrap();
        let colors = cx.debug_bounds("reader-annotation-color-row").unwrap();
        let green = cx.debug_bounds("reader-annotation-color-Green").unwrap();
        assert_eq!(delete.right(), colors.right());
        assert!(delete.left() >= green.right());
        assert_eq!(delete.center().y, green.center().y);
        cx.simulate_click(delete.center(), Default::default());
        draw(cx);
        view.read_with(cx, |view, _| {
            assert!(view.annotation.deleted);
            assert!(!view.open);
        });
    }
}

/// Save or Enter commits the bookmark name. Dismissal discards its edits.
pub(crate) fn bookmark_editor<C: BookmarkEditorTarget>(target: &Entity<C>, geometry: Geometry, name: &Entity<InputState>, theme: components::BrowserTheme, cx: &App) -> AnyElement {
    let dismiss = target.clone();
    let keyboard = target.clone();
    let enter = target.clone();
    let save = target.clone();
    let save_button =
        components::outlined_text_button("reader-bookmark-save", text!(reader::save)).primary().disabled(name.read(cx).value().trim().is_empty()).debug_selector(|| "reader-bookmark-save".into()).on_click(move |_, window, cx| {
            cx.stop_propagation();
            save.update(cx, |this, cx| this.save_bookmark(window, cx));
        });
    components::modal_surface(theme)
        .id("reader-bookmark-editor")
        .debug_selector(|| "reader-bookmark-popup".into())
        .absolute()
        .w(px(WIDTH))
        .max_w_full()
        .max_h(px(geometry.max_height))
        .left(px(geometry.left))
        .when(geometry.above, |panel| panel.bottom(px(geometry.edge_offset)))
        .when(!geometry.above, |panel| panel.top(px(geometry.edge_offset)))
        .occlude()
        .p(px(components::SPACE_MD))
        .rounded(px(components::RADIUS_MD))
        .border_1()
        .border_color(theme.rule)
        .bg(theme.page_bg)
        .shadow(components::overlay_shadow(theme))
        .capture_action(move |action: &InputEnter, window, cx| {
            if action.shift || action.secondary {
                return;
            }
            // Submit before the shared note input can insert a newline.
            enter.update(cx, |this, cx| this.save_bookmark(window, cx));
            cx.stop_propagation();
        })
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down_out(move |_, window, cx| {
            dismiss.update(cx, |this, cx| this.dismiss_annotation_editor(window, cx));
            cx.stop_propagation();
        })
        .on_key_down(move |event, window, cx| {
            if event.keystroke.key == "escape" {
                keyboard.update(cx, |this, cx| this.dismiss_annotation_editor(window, cx));
                cx.stop_propagation();
            }
        })
        .child(components::column(components::SPACE_SM).w_full().child(Input::new(name).appearance(false).w_full()).child(components::action_row().justify_end().child(save_button)))
        .into_any_element()
}
