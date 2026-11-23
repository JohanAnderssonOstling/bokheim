//! The graph's typographic labels, for callers that draw nodes as text.
//!
//! The root, the path and the selection use the theme's display face in capitals,
//! spaced with hair spaces because gpui has no letter-spacing. Every other
//! label uses the selected interface face's small caps. Subtree counts follow
//! the name, raised.
//!
//! A label is measured by laying out the element it draws, so the layout and
//! the drawing cannot disagree.

use gpui::prelude::FluentBuilder;
use gpui::{App, AvailableSpace, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Size, Styled, Window, div, px, rems, size};
use gpui_component::Theme;

use ui_graph_layout::NodeRole;
use crate::browser_theme;

const HAIR_SPACE: &str = "\u{200A}";
/// A raised count's size and gap relative to its label.
const COUNT_SCALE: f32 = 0.75;
const COUNT_GAP: f32 = 2.0;
const LABEL_SIZE_STEP: f32 = crate::TEXT_SM - crate::TEXT_XS;
/// The widest a label's text runs before it wraps, then is cut short on its second line.
const MAX_TEXT_WIDTH_REM: f32 = 12.5;
const MAX_LINES: usize = 2;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 8.0;

/// The text and count shared by a graph node's measurement and drawing.
#[derive(Clone)]
pub struct GraphLabel {
    name: SharedString,
    count: Option<usize>,
}

impl GraphLabel {
    pub fn new(name: impl Into<SharedString>, count: Option<usize>) -> Self {
        Self { name: name.into(), count }
    }

    pub fn measure(&self, role: NodeRole, compact: bool, window: &mut Window, cx: &mut App) -> Size<Pixels> {
        let mut body = self.body(role, compact, cx).into_any_element();
        body.layout_as_root(size(AvailableSpace::MaxContent, AvailableSpace::MaxContent), window, cx)
    }

    pub fn view(&self, role: NodeRole, compact: bool) -> impl IntoElement {
        GraphLabelView { label: self.clone(), role, compact }
    }

    fn body(&self, role: NodeRole, compact: bool, cx: &mut App) -> Div {
        crate::ensure_text_fonts(&self.name, cx);
        let theme = browser_theme(cx);
        let font_family = Theme::global(cx).font_family.clone();
        let text_size = label_rems(role, compact);
        let display = matches!(role, NodeRole::Root | NodeRole::Path | NodeRole::Selection);
        let shown = if display { self.name.to_uppercase().chars().map(String::from).collect::<Vec<_>>().join(HAIR_SPACE) } else { self.name.to_string() };
        let text = div()
            .max_w(rems(MAX_TEXT_WIDTH_REM))
            .text_center()
            .line_clamp(MAX_LINES)
            .text_ellipsis()
            .when(display, |text| text.when_some(theme.display_font_family(), |text, family| text.font_family(family)).when(role == NodeRole::Selection, |text| text.font_weight(FontWeight::SEMIBOLD)))
            .when(!display, |text| text.font_family(font_family.clone()).font_features(crate::small_caps()))
            .child(shown);
        div()
            .px(px(PAD_X))
            .py(px(PAD_Y))
            .flex()
            .items_start()
            .text_size(rems(text_size))
            .line_height_auto()
            .child(text)
            .children(self.count.map(|count| div().flex_none().pl(px(COUNT_GAP)).font_family(font_family).text_size(rems(text_size * COUNT_SCALE)).line_height_auto().text_color(theme.text_muted).child(count.to_string())))
    }
}

#[derive(IntoElement)]
struct GraphLabelView {
    label: GraphLabel,
    role: NodeRole,
    compact: bool,
}

impl RenderOnce for GraphLabelView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let body = self.label.body(self.role, self.compact, cx);
        let theme = browser_theme(cx);
        let color = match self.role {
            NodeRole::Selection => theme.text_accent,
            NodeRole::Context => theme.text_muted,
            NodeRole::Root | NodeRole::Path | NodeRole::Fan => theme.text,
        };
        let accent = theme.text_accent;
        div().size_full().flex().items_center().justify_center().cursor_pointer().text_color(color).hover(move |style| style.text_color(accent)).child(body)
    }
}

/// Label sizes in rems, for phones or for larger windows.
fn label_rems(role: NodeRole, compact: bool) -> f32 {
    match (role, compact) {
        (NodeRole::Root, false) => 1.25,
        (NodeRole::Path, false) => 0.9,
        (NodeRole::Selection, false) => 1.05,
        (NodeRole::Fan, false) => 0.9375 + LABEL_SIZE_STEP,
        (NodeRole::Context, false) => 0.875 + LABEL_SIZE_STEP,
        (NodeRole::Root, true) => 1.0,
        (NodeRole::Path, true) => 0.775,
        (NodeRole::Selection, true) => 0.9,
        (NodeRole::Fan, true) => 0.9375 + LABEL_SIZE_STEP,
        (NodeRole::Context, true) => 0.84375 + LABEL_SIZE_STEP,
    }
}
