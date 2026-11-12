//! Source controls shared by reader chrome and lookup summaries.
use gpui::prelude::*;
use gpui::{Anchor, AnyElement, App, Window, px};
use gpui_component::{IconName, Theme, popover::Popover};
use ui_components as components;
use ui_text::text;
use url::Url;

use super::dictionary::lookup_popover;

pub(super) enum Placement {
    Reader,
    Lookup,
}

pub(super) fn actions(uri: Option<&str>, placement: Placement, window: &Window, cx: &App) -> Vec<AnyElement> {
    let Some(uri) = uri else { return Vec::new() };
    let Ok(url) = Url::parse(uri) else { return Vec::new() };
    if !matches!(url.scheme(), "https" | "http") || !url.host_str().is_some_and(|host| host.ends_with(".wikipedia.org")) {
        return Vec::new();
    }
    let in_reader = matches!(placement, Placement::Reader);
    if in_reader && components::uses_mobile_reader_controls(window) { return Vec::new(); }
    let anchor = match placement {
        Placement::Lookup => Anchor::TopRight,
        Placement::Reader if components::uses_mobile_reader_controls(window) => Anchor::BottomRight,
        Placement::Reader => Anchor::TopLeft,
    };
    let source = uri.to_owned();
    let attribution = components::modal_form_surface(components::browser_theme(cx))
        .font_family(Theme::global(cx).font_family.clone())
        .text_size(Theme::global(cx).font_size)
        .debug_selector(|| "wikipedia-attribution".into())
        .w(px(260.0))
        .max_w((window.viewport_size().width - px(48.0)).max(px(1.0)))
        .gap(px(8.0))
        .child(components::section_title(text!(common::source_and_license)))
        .child(text!(reader::wikipedia_contributors))
        .child(components::outlined_text_button("wikipedia-source", text!(reader::wikipedia_article)).ghost().no_fill().on_click(move |_, _, cx| cx.open_url(&source)))
        .child(components::outlined_text_button("wikipedia-license", text!(reader::cc_by_sa_4_0)).ghost().no_fill().on_click(|_, _, cx| cx.open_url("https://creativecommons.org/licenses/by-sa/4.0/")));
    let info = Popover::new("wikipedia-info")
        .anchor(anchor)
        .appearance(false)
        .trigger(
            components::outlined_icon_button("wikipedia-info-button", text!(common::source_and_license), IconName::Info)
                .no_fill()
                .tab_stop(in_reader)
                .debug_selector(|| "wikipedia-info-button".into())
                .when(in_reader, |button| button.w(px(40.0)).h(px(40.0))),
        )
        .child(lookup_popover(attribution));
    let browser_url = uri.to_owned();
    let browser = components::outlined_icon_button("wikipedia-open-browser", text!(reader::open_wikipedia_in_browser), IconName::Globe)
        .no_fill()
        .debug_selector(|| "wikipedia-open-browser".into())
        .when(in_reader, |button| button.w(px(40.0)).h(px(40.0)))
        .on_click(move |_, _, cx| cx.open_url(&browser_url));
    vec![browser.into_any_element(), info.into_any_element()]
}
