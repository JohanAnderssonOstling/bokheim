//! The graph's typographic labels, for callers that draw nodes as text.
//!
//! Every role uses the same interface face, small caps and text size. Roles
//! change colour without changing geometry. A book count follows the name,
//! raised.
//!
//! A label is measured by laying out the element it draws, so the layout and
//! the drawing cannot disagree.

use gpui::prelude::FluentBuilder;
use gpui::{App, AvailableSpace, Div, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Size, Styled, Window, div, px, rems, size};
use gpui_component::Theme;

use crate::browser_theme;
use ui_graph_layout::NodeRole;

/// A raised count's size and gap relative to its label.
const COUNT_SCALE: f32 = 0.75;
const COUNT_GAP: f32 = 2.0;
/// The widest a label's text runs before it wraps, then is cut short on its second line.
const MAX_TEXT_WIDTH_REM: f32 = 12.5;
const MAX_LINES: usize = 2;
const PAD_X: f32 = 4.0;
const PAD_Y: f32 = 2.0;

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

    pub fn measure(&self, window: &mut Window, cx: &mut App) -> Size<Pixels> {
        let mut body = self.body(window, cx).into_any_element();
        body.layout_as_root(size(AvailableSpace::MaxContent, AvailableSpace::MaxContent), window, cx)
    }

    pub fn view(&self, role: NodeRole) -> impl IntoElement {
        GraphLabelView { label: self.clone(), role }
    }

    fn body(&self, window: &mut Window, cx: &mut App) -> Div {
        crate::ensure_text_fonts(&self.name, cx);
        let theme = browser_theme(cx);
        let font_family = Theme::global(cx).font_family.clone();
        let text_size = crate::TEXT_SM;
        let shown = self.name.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = || div().max_w(rems(MAX_TEXT_WIDTH_REM)).text_size(rems(text_size)).line_height_auto().whitespace_nowrap().text_ellipsis().font_family(font_family.clone()).font_features(crate::small_caps());
        // Break with the same shaped font we render, then let each row take its
        // actual width. The raised count follows the last visible line.
        let first_end = window.with_text_style(Some(text().style().text.clone()), |window| {
            let style = window.text_style();
            window
                .text_system()
                .shape_text(shown.clone().into(), window.rem_size() * text_size, &[style.to_run(shown.len())], Some(window.rem_size() * MAX_TEXT_WIDTH_REM), Some(MAX_LINES))
                .ok()
                .and_then(|lines| lines.first().map(|line| line.wrap_boundaries().first().map_or(line.len(), |boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index)))
                .unwrap_or(shown.len())
        });
        let (first, rest) = shown.split_at(first_end);
        let rest = rest.trim();
        let last = crate::row(0.0)
            .items_start()
            .child(text().child(if rest.is_empty() { first.trim() } else { rest }.to_owned()))
            .children(self.count.map(|count| div().flex_none().pl(px(COUNT_GAP)).font_family(font_family.clone()).text_size(rems(text_size * COUNT_SCALE)).line_height_auto().text_color(theme.text_muted).child(count.to_string())));
        div().px(px(PAD_X)).py(px(PAD_Y)).flex().flex_col().items_center().when(!rest.is_empty(), |body| body.child(text().child(first.trim().to_owned()))).child(last)
    }
}

#[derive(IntoElement)]
struct GraphLabelView {
    label: GraphLabel,
    role: NodeRole,
}

impl RenderOnce for GraphLabelView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let body = self.label.body(window, cx);
        let theme = browser_theme(cx);
        let color = match self.role {
            NodeRole::Selection | NodeRole::SelectedRoot => theme.text_accent,
            NodeRole::Context => theme.text_muted,
            NodeRole::Root | NodeRole::Path | NodeRole::Fan => theme.text,
        };
        let accent = theme.text_accent;
        div().size_full().flex().items_center().justify_center().cursor_pointer().text_color(color).hover(move |style| style.text_color(accent)).child(body)
    }
}
