use std::path::PathBuf;

use floem::window::WindowConfig;
use floem::{Application, View};
use html_renderer::{HTMLRenderer, RendererCommand};
use kurbo::Size;

fn smoke_test_html() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../smoke/test.html")
}

fn smoke_view() -> impl View {
    let mut renderer = HTMLRenderer::from_source_path_with_nav(smoke_test_html(), Some("0:0"));
    renderer.apply(RendererCommand::SetLineNumbers(false));
    renderer
}

fn main() {
    let window = WindowConfig::default().title("Floem HTML renderer smoke test — arrows/wheel navigate, drag selects").size(Size::new(1_000.0, 760.0));
    Application::new().window(move |_| smoke_view(), Some(window)).run();
}
