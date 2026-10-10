//! Common reader page assembly and action routing.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FocusHandle, Window};
use ui_components as components;

use crate::shell::input::{self, CommonReaderCommands};
use crate::{NextSidebarTab, PreviousSidebarTab, CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, PreviousLine, PreviousPage, ReturnToLibrary, ToggleSearch, ToggleSidebar, ToggleToc};

pub(crate) fn reader_page<C: CommonReaderCommands>(
    window: &mut Window, cx: &mut Context<C>, theme: components::BrowserTheme, document_ready: bool, loading_focus: &FocusHandle, chrome_visible: bool, bottom_bar: Option<AnyElement>, mobile_search: Option<AnyElement>, content: AnyElement,
) -> Div {
    let safe_area = window.insets().safe_area;
    let mobile = components::uses_mobile_navigation(window);

    let page = components::reader_app_page(window, cx, theme)
        .relative()
        .key_context("Reader")
        .when(chrome_visible && !mobile, |page| page.pt(safe_area.top))
        .when(!document_ready, |page| page.track_focus(loading_focus))
        .on_action(cx.listener(input::dispatch::<C, PreviousPage>))
        .on_action(cx.listener(input::dispatch::<C, NextPage>))
        .on_action(cx.listener(input::dispatch::<C, PreviousLine>))
        .on_action(cx.listener(input::dispatch::<C, NextLine>))
        .on_action(cx.listener(input::dispatch::<C, NextSidebarTab>))
        .on_action(cx.listener(input::dispatch::<C, PreviousSidebarTab>))
        .on_action(cx.listener(input::dispatch::<C, ToggleSidebar>))
        .on_action(cx.listener(input::dispatch::<C, ToggleToc>))
        .on_action(cx.listener(input::dispatch::<C, FocusReaderSearch>))
        .on_action(cx.listener(input::dispatch::<C, ToggleSearch>))
        .on_action(cx.listener(input::dispatch::<C, CloseSearch>))
        .on_action(cx.listener(input::dispatch::<C, IncreaseScale>))
        .on_action(cx.listener(input::dispatch::<C, DecreaseScale>))
        .on_action(cx.listener(input::dispatch::<C, ReturnToLibrary>));
    if mobile {
        // The document keeps the whole viewport; an unclaimed page tap
        // toggles its controls without blocking links or images.
        let controls = if bottom_bar.is_some() || mobile_search.is_some() {
            gpui::div()
                .id("reader-mobile-controls")
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .pb(safe_area.bottom)
                .pl(safe_area.left)
                .pr(safe_area.right)
                .bg(theme.page_bg)
                .flex()
                .flex_col()
                .occlude()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .when_some(mobile_search, |controls, search| controls.child(search))
                .when_some(bottom_bar, |controls, bar| controls.child(bar))
                .into_any_element()
        } else {
            return page.child(components::reader_content().child(content));
        };
        // The document always receives the full viewport; the bar only covers it.
        page.child(components::reader_content().child(content)).child(controls)
    } else {
        page.child(components::reader_content().child(content))
    }
}
