#![cfg(target_family = "wasm")]

use std::borrow::Cow;
use std::cell::RefCell;
use std::io::Cursor;

use gpui::prelude::*;
use gpui::{App, AppContext, Bounds, Entity, SharedString, Task, WindowBounds, WindowOptions, px, size};
use html_publication::{PublicationFormat, PublicationSource};
use html_view_core::{RendererInitialConfig, RendererPreparation};
use html_view_gpui::HtmlView;

const EPUB_BYTES: &[u8] = include_bytes!("../../../Robert D. Putnam - Bowling Alone_ Revised and Updated_ The Collapse and Revival of American Community (2020, Simon & Schuster) - libgen.li.epub");

thread_local! {
    static APPLICATION: RefCell<Option<gpui::ApplicationHandle>> = const { RefCell::new(None) };
}

enum SmokeState {
    Loading(SharedString),
    Ready(Entity<HtmlView>),
    Error(SharedString),
}

struct ReaderSmoke {
    state: SmokeState,
    _load_task: Task<()>,
}

impl ReaderSmoke {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        let executor = cx.background_executor().clone();
        let load_task = cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| {
                this.state = SmokeState::Loading(SharedString::from("Foreground task started; checking background executor…"));
                cx.notify();
            });
            executor.spawn(async {}).await;
            let _ = this.update(cx, |this, cx| {
                this.state = SmokeState::Loading(SharedString::from("Background executor responded; opening embedded EPUB…"));
                cx.notify();
            });
            let publication = PublicationSource::from_reader(PublicationFormat::Epub, Cursor::new(EPUB_BYTES)).map_err(|error| format!("opening embedded EPUB failed: {error}"));
            let publication = match publication {
                Ok(publication) => publication,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.state = SmokeState::Error(SharedString::from(error));
                        cx.notify();
                    });
                    return;
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.state = SmokeState::Loading(SharedString::from("EPUB opened; preparing first document on main thread…"));
                cx.notify();
            });
            let prepared = RendererPreparation::from_provider_with_nav(publication.provider, publication.document_uris, publication.start_index, None, RendererInitialConfig::default())
                .map_err(|error| format!("preparing first document failed: {error}"));
            let _ = this.update_in(cx, move |this, window, cx| {
                match prepared {
                    Ok(prepared) => {
                        let renderer = cx.new(|cx| HtmlView::from_preparation(prepared, window, cx).expect("prepared smoke document should finish shaping"));
                        this.state = SmokeState::Ready(renderer);
                    }
                    Err(error) => this.state = SmokeState::Error(SharedString::from(error)),
                }
                cx.notify();
            });
        });
        Self { state: SmokeState::Loading(SharedString::from("Opening embedded EPUB…")), _load_task: load_task }
    }
}

impl Render for ReaderSmoke {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        match &self.state {
            SmokeState::Loading(stage) => gpui::div().size_full().flex().items_center().justify_center().child(stage.clone()).into_any_element(),
            SmokeState::Ready(renderer) => renderer.clone().into_any_element(),
            SmokeState::Error(error) => gpui::div().size_full().flex().items_center().justify_center().child(error.clone()).into_any_element(),
        }
    }
}

#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    gpui_platform::web_init();
    let application = gpui_platform::application();
    let handle = application.run_embedded(open_reader);
    APPLICATION.with(|application| *application.borrow_mut() = Some(handle));
}

fn open_reader(cx: &mut App) {
    if let Err(error) = cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/NotoSans-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/NotoSans-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/LibertinusSerif-Regular.otf")),
        Cow::Borrowed(include_bytes!("../../desktop-gpui/assets/fonts/LibertinusSerif-Bold.otf")),
    ]) {
        log::warn!("failed to install smoke-test fonts: {error}");
    }
    let bounds = Bounds::centered(None, size(px(1000.0), px(760.0)), cx);
    cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), focus: true, ..Default::default() }, |_, cx| cx.new(ReaderSmoke::new)).expect("failed to open reader smoke window");
    cx.activate(true);
}
