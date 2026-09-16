//! Common reader page assembly and action routing.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FocusHandle, Window};
use ui_components as components;

use crate::shell::input::{self, CommonReaderCommands};
use crate::{CloseSearch, DecreaseScale, FocusReaderSearch, IncreaseScale, NextLine, NextPage, PreviousLine, PreviousPage, ReturnToLibrary, ToggleSearch, ToggleToc};

pub(crate) fn reader_page<C: CommonReaderCommands>(
    window: &mut Window, cx: &mut Context<C>, theme: components::BrowserTheme, document_ready: bool, loading_focus: &FocusHandle, toolbar: Option<AnyElement>, search: Option<AnyElement>, content: AnyElement,
) -> Div {
    let toolbar_visible = toolbar.is_some();
    let safe_area_top = window.insets().safe_area.top;
    let mobile = components::uses_mobile_navigation(window);

    let page = components::reader_app_page(window, cx, theme)
        .relative()
        .key_context("Reader")
        .when(toolbar_visible && !mobile, |page| page.pt(safe_area_top))
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
        .on_action(cx.listener(input::dispatch::<C, ReturnToLibrary>));
    if mobile {
        let header = toolbar_visible.then(|| {
            let safe_area = window.insets().safe_area;
            gpui::div()
                .id("reader-mobile-chrome")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .flex()
                .flex_col()
                .pt(safe_area.top)
                .pl(safe_area.left)
                .pr(safe_area.right)
                .bg(theme.page_bg)
                .occlude()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .children(toolbar)
                .children(search)
        });
        // The document always receives the full viewport; bars only cover it.
        page.child(components::reader_content().child(content)).children(header)
    } else {
        page.children(toolbar).children(search).child(components::reader_content().child(content))
    }
}

#[cfg(all(test, feature = "mobile", not(feature = "kobo")))]
mod tests {
    use super::*;
    use crate::shell::input::CommandTarget;
    use gpui::{Render, px};

    struct TestReader {
        focus: FocusHandle,
        chrome: bool,
        search: bool,
    }

    macro_rules! ignore_commands {
        ($($command:ty),* $(,)?) => { $(
            impl CommandTarget<$command> for TestReader {
                fn execute(&mut self, _: &$command, _: &mut Window, _: &mut Context<Self>) {}
            }
        )* };
    }
    ignore_commands!(PreviousPage, NextPage, PreviousLine, NextLine, ToggleToc, FocusReaderSearch, ToggleSearch, CloseSearch, IncreaseScale, DecreaseScale, ReturnToLibrary);
    impl Render for TestReader {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let toolbar = self.chrome.then(|| gpui::div().debug_selector(|| "test-reader-toolbar".into()).h(px(48.)).into_any_element());
            let search = self.search.then(|| gpui::div().h(px(96.)).into_any_element());
            let document = gpui::div().id("test-book-viewport").debug_selector(|| "test-book-viewport".into()).size_full().into_any_element();
            let theme = components::browser_theme(cx);
            reader_page(window, cx, theme, false, &self.focus, toolbar, search, document)
        }
    }

    #[gpui::test]
    fn mobile_chrome_overlays_the_book_without_an_extra_navigation_row(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let reader = cx.new(|cx| TestReader { focus: cx.focus_handle(), chrome: false, search: false });
        let (_, cx) = cx.add_window_view(|window, cx| gpui_component::Root::new(reader.clone(), window, cx));
        cx.simulate_resize(gpui::size(px(390.), px(844.)));
        cx.run_until_parked();
        let full_book = cx.debug_bounds("test-book-viewport").expect("book viewport");
        assert!(cx.debug_bounds("mobile-navigation-back").is_none());
        for search in [false, true] {
            reader.update(cx, |reader, cx| {
                reader.chrome = true;
                reader.search = search;
                cx.notify();
            });
            cx.run_until_parked();
            assert_eq!(cx.debug_bounds("test-book-viewport"), Some(full_book));
            assert!(cx.debug_bounds("mobile-navigation-back").is_none());
            let toolbar = cx.debug_bounds("test-reader-toolbar").expect("reader toolbar");
            assert_eq!(toolbar.size.height, px(48.));
        }
        reader.update(cx, |reader, cx| {
            reader.chrome = false;
            reader.search = false;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(cx.debug_bounds("test-book-viewport"), Some(full_book));
        assert!(cx.debug_bounds("mobile-navigation-back").is_none());
    }
}
