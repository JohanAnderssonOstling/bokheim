use std::path::PathBuf;

use gpui::{App, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use gpui_platform::application;
use html_view_core::RendererCommand;
use html_view_gpui::HtmlView;

fn smoke_test_html() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../smoke/test.html")
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1_000.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                titlebar: Some(gpui::TitlebarOptions { title: Some("GPUI HTML renderer smoke test — arrows/wheel navigate, drag selects".into()), ..Default::default() }),
                focus: true,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| {
                    let mut view = HtmlView::from_source_path(smoke_test_html(), Some("0:0"), window, cx);
                    view.apply(RendererCommand::SetLineNumbers(false), cx);
                    view
                });
                window.focus(&view.read(cx).focus_handle(), cx);
                view
            },
        )
        .expect("failed to open GPUI smoke-test window");
        cx.on_window_closed(|cx, _window_id| cx.quit()).detach();
        cx.activate(true);
    });
}
