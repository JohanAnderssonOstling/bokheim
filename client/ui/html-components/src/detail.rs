//! HTML descriptions integrated with the application's generic controls.
use gpui::prelude::*;
use gpui::{App, Div, ElementId, SharedString, div, rgba};
use gpui_component::Theme;
#[cfg(test)]
use html_engine_model::layout_state;
use html_gpui::HtmlFragmentElement;
use html_parse::plain_text_from_fragment;
#[cfg(test)]
use ui_components::book_detail_text;
use ui_components::{BOOK_DETAIL_DESCRIPTION_LINE_HEIGHT_EM, BOOK_DETAIL_DESCRIPTION_SIZE, BrowserTheme, InteractionHighlights};

/// The blurb as a continuous HTML fragment, justified with the engine's
/// Knuth–Plass line breaking and sized to its enclosing scroll column.
///
/// `None` where the markup carries no words, so a page is not given a heading
/// over nothing.
pub fn book_detail_description(description: &str, theme: BrowserTheme, cx: &mut App) -> Option<Div> {
    if plain_text_from_fragment(description).is_empty() {
        return None;
    }
    let selection_color = rgba(u32::from_be_bytes(InteractionHighlights::from_theme(theme).selection)).into();
    Some(div().w_full().min_w_0().flex_none().child(html_fragment("book-detail-description-html", description.to_owned(), theme, cx).justify().selectable(selection_color)))
}

/// Continuous HTML using the resolved interface font and theme.
pub fn html_fragment(id: impl Into<ElementId>, source: impl Into<SharedString>, theme: BrowserTheme, cx: &mut App) -> HtmlFragmentElement {
    crate::configure_fonts(cx);
    let stylesheet = description_stylesheet(Theme::global(cx).font_family.as_ref());
    HtmlFragmentElement::new(id, source).stylesheet(stylesheet).text_color(theme.text)
}

fn description_stylesheet(family: &str) -> String {
    let family = family.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\a ").replace('\r', "\\d ");
    // HTML fragments do not inherit GPUI text styles. Use the resolved
    // interface family throughout the description while retaining emphasis.
    format!(
        "html, body {{ margin: 0; padding: 0; }}
         :where(body) {{ font-family: \"{family}\"; font-size: {BOOK_DETAIL_DESCRIPTION_SIZE}rem; line-height: {BOOK_DETAIL_DESCRIPTION_LINE_HEIGHT_EM}; }}
         p:first-child {{ margin-top: 0; }}
         p:last-child {{ margin-bottom: 0; }}
         a:any-link {{ color: inherit !important; text-decoration: none !important; }}
         * {{ font-family: \"{family}\" !important; }}"
    )
}

#[cfg(test)]
mod tests {
    use html_pipeline::engine::box_queries;

    use super::*;
    use gpui::{AppContext, AvailableSpace, Context, FocusHandle, IntoElement, Modifiers, MouseButton, Render, TestAppContext, Window, canvas, point, px};
    use gpui_component::{Root, WindowExt};
    use ui_components::ScrollableElement as _;

    const TITLE: &str = "The <Unknown> &amp; *stars* \"Åsa\" 'é'";
    const DESCRIPTION: &str = "Description & café follows the characters through a changing world, with friendships and unexpected discoveries along the way. This longer paragraph wraps across several justified lines so selection also exercises the book detail page's normal description layout.";

    struct LiteralTitle;

    struct TitleAndDescription {
        focus: FocusHandle,
    }

    #[gpui::test]
    fn description_uses_resolved_interface_font_including_authored_text(cx: &mut TestAppContext) {
        use html_gpui::backend::GpuiTextCache;
        use html_pipeline::DocumentFactory;
        use html_pipeline::engine::{GlyphResourceStore, LayoutConstraints};

        cx.update(gpui_component::init);
        let window = cx.add_window(|_, _| gpui::EmptyView);
        window
            .update(cx, |_, window, cx| {
                let mut shaper = GpuiTextCache::new(window.text_system().clone());
                let source = "<p id='plain' style='font-family:Publisher'>Description <strong id='bold'>bold</strong> <em id='italic'>italic</em></p>";
                for selected in ["Raleway", "NV Garamond", "Example Interface Font"] {
                    Theme::global_mut(cx).font_family = selected.into();
                    let mut resources = GlyphResourceStore::default();
                    let document = DocumentFactory::new()
                        .parse_with_new_pipeline(source, Some(&description_stylesheet(Theme::global(cx).font_family.as_ref())))
                        .shape(&mut shaper, &mut resources)
                        .unwrap()
                        .layout(LayoutConstraints::new(300.0, 16.0).unwrap(), &mut resources)
                        .unwrap();
                    let view = document.render_view(&resources);
                    let boxes = view.boxes();
                    for id in ["plain", "bold", "italic"] {
                        let index = (0..box_queries::len(boxes)).map(layout_state::box_id_new).find(|&index| box_queries::attribute(boxes, index, "id") == Some(id)).unwrap();
                        let format = box_queries::text_format(boxes, index);
                        let family = format.font_family.unwrap();
                        assert_eq!(family.trim_matches('"'), selected, "{id} must follow the resolved interface font");
                        if id == "bold" {
                            assert_eq!(format.font_weight, 700);
                        }
                        if id == "italic" {
                            assert_eq!(format.font_style, html_style_model::FontStyle::Italic);
                        }
                    }
                }
            })
            .unwrap();
    }

    impl Render for LiteralTitle {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(600.)).child(book_detail_text("literal-title", TITLE))
        }
    }

    impl Render for TitleAndDescription {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let focus = self.focus.clone();
            // Match the detail page: build the scrolling pane during canvas
            // prepaint, inside a focusable page and the application Root.
            canvas(
                move |bounds, window, cx| {
                    let mut body = div()
                        .size_full()
                        .w(px(300.))
                        .track_focus(&focus)
                        .flex()
                        .flex_col()
                        .child(div().h(px(40.)).child(book_detail_text("literal-title", TITLE)))
                        .child(book_detail_description(&format!("<p>{}</p>", DESCRIPTION.replace('&', "&amp;")), BrowserTheme::default(), cx).unwrap())
                        .overflow_y_scrollbar()
                        .into_any_element();
                    body.prepaint_as_root(bounds.origin, bounds.size.map(AvailableSpace::Definite), window, cx);
                    body
                },
                |_, mut body, window, cx| body.paint(window, cx),
            )
            .size_full()
        }
    }

    #[gpui::test]
    fn metadata_is_selectable_and_copies_literal_punctuation(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let content = cx.new(|_| LiteralTitle);
            Root::new(content, window, cx)
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let from = point(px(1.), px(10.));
        let to = point(px(30.), px(10.));
        cx.simulate_mouse_move(from, None, Modifiers::default());
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        cx.update(|window, cx| {
            let _ = window.draw(cx);
            assert!(!window.selected_text(cx).trim().is_empty());
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-a cmd-c" } else { "ctrl-a ctrl-c" });
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim(), TITLE);
    }

    #[gpui::test]
    fn description_and_metadata_copy_follow_focus_in_application_root(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let content = cx.new(|cx| TitleAndDescription { focus: cx.focus_handle() });
            Root::new(content, window, cx)
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let select_and_copy = if cfg!(target_os = "macos") { "cmd-a cmd-c" } else { "ctrl-a ctrl-c" };
        cx.simulate_click(point(px(10.), px(50.)), Modifiers::default());
        cx.simulate_keystrokes(select_and_copy);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim(), DESCRIPTION);
        cx.simulate_click(point(px(10.), px(10.)), Modifiers::default());
        cx.simulate_keystrokes(select_and_copy);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim(), TITLE);
    }

    #[gpui::test]
    fn description_drag_copies_text_and_hover_does_not_extend_selection(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let content = cx.new(|cx| TitleAndDescription { focus: cx.focus_handle() });
            Root::new(content, window, cx)
        });
        cx.run_until_parked();
        let from = point(px(1.), px(52.));
        let to = point(px(120.), px(52.));
        cx.simulate_mouse_move(from, None, Modifiers::default());
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        let copy = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };
        cx.simulate_keystrokes(copy);
        let selected = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(selected.starts_with("Description"), "description drag must copy its text: {selected:?}");
        cx.simulate_mouse_move(point(px(500.), px(52.)), None, Modifiers::default());
        cx.simulate_keystrokes(copy);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), selected);

        // Wrapped lines use the same geometry as the painted description.
        cx.simulate_mouse_down(point(px(1.), px(90.)), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(point(px(250.), px(150.)), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(point(px(250.), px(150.)), MouseButton::Left, Modifiers::default());
        cx.simulate_keystrokes(copy);
        let wrapped = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(!wrapped.is_empty() && DESCRIPTION.contains(&wrapped), "wrapped selection must follow description text: {wrapped:?}");

        // Recover even when the release happened outside the window.
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(point(px(500.), px(52.)), None, Modifiers::default());
        cx.simulate_keystrokes(copy);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), selected);
    }

    #[gpui::test]
    fn metadata_hover_after_missed_release_does_not_select_text(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let content = cx.new(|_| LiteralTitle);
            Root::new(content, window, cx)
        });
        cx.run_until_parked();
        let from = point(px(1.), px(10.));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(point(px(200.), px(10.)), None, Modifiers::default());
        cx.update(|window, cx| {
            let _ = window.draw(cx);
            assert!(window.selected_text(cx).is_empty(), "hover must not extend a stale drag");
        });
    }
}
