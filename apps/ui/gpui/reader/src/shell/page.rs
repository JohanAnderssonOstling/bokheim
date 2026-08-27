//! Common reader page assembly and action routing.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FocusHandle, Window};
use ui_components as components;

use crate::shell::input::{self, CommonReaderCommands};
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, PreviousLine, PreviousPage, ToggleSearch, ToggleToc};

pub(crate) fn reader_page<C: CommonReaderCommands>(window: &mut Window, cx: &mut Context<C>, theme: components::BrowserTheme, document_ready: bool, loading_focus: &FocusHandle, toolbar: Option<AnyElement>, content: AnyElement) -> Div {
    components::reader_app_page(window, cx, theme)
        .key_context("Reader")
        .when(!document_ready, |page| page.track_focus(loading_focus))
        .on_action(cx.listener(input::dispatch::<C, PreviousPage>))
        .on_action(cx.listener(input::dispatch::<C, NextPage>))
        .on_action(cx.listener(input::dispatch::<C, PreviousLine>))
        .on_action(cx.listener(input::dispatch::<C, NextLine>))
        .on_action(cx.listener(input::dispatch::<C, ToggleToc>))
        .on_action(cx.listener(input::dispatch::<C, FocusReaderSearch>))
        .on_action(cx.listener(input::dispatch::<C, ToggleSearch>))
        .on_action(cx.listener(input::dispatch::<C, CloseSearch>))
        .on_action(cx.listener(input::dispatch::<C, IncreaseScale>))
        .on_action(cx.listener(input::dispatch::<C, DecreaseScale>))
        .children(toolbar)
        .child(components::reader_content().child(content))
}
