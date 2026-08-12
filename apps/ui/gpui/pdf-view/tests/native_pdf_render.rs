use gpui::prelude::*;
use gpui::{Entity, HeadlessAppContext, Render, Window, px, size};
use gpui_wgpu::CosmicTextSystem;
use lopdf::{Document, Object, Stream, dictionary};
use pdf_view_gpui::PdfView;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;

const VIEWPORT_WIDTH: u32 = 800;
const VIEWPORT_HEIGHT: u32 = 600;

struct NativePdfShell {
    pdf: Entity<PdfView>,
}

impl Render for NativePdfShell {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = ui_components::browser_theme(cx);
        let body = ui_components::reader_document_body().child(ui_components::reader_content().child(self.pdf.clone()));
        ui_components::app_page("IBM Plex Sans", 16, theme).child(ui_components::reader_toolbar(theme)).child(ui_components::reader_content().child(body))
    }
}

fn fixture_pdf() -> Vec<u8> {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let content = b"0.1 0.35 0.8 rg 40 80 220 300 re f BT /F1 30 Tf 72 480 Td (Native PDF) Tj ET".to_vec();
    let contents_id = document.add_object(Stream::new(dictionary! {}, content));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 300.into(), 500.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        "Contents" => contents_id,
    });
    document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("serialize PDF fixture");
    bytes
}

#[test]
fn native_pdf_view_paints_the_loaded_page() {
    let text_system = Arc::new(CosmicTextSystem::new("IBM Plex Sans"));
    let mut context = HeadlessAppContext::with_platform_and_scale_factor(text_system, Arc::new(()), gpui_platform::current_headless_renderer, 1.0);
    context.update(gpui_component::init);
    let bytes = fixture_pdf();
    let window = context
        .open_window(size(px(VIEWPORT_WIDTH as f32), px(VIEWPORT_HEIGHT as f32)), move |window, cx| {
            cx.new(|cx| {
                let pdf = cx.new(|cx| PdfView::from_reader(PathBuf::from("fixture.pdf"), Cursor::new(bytes), 0, window, cx));
                NativePdfShell { pdf }
            })
        })
        .expect("open headless PDF window");

    context.run_until_parked();
    let page_count = context.read_window(&window, |view, cx| view.read(cx).pdf.read(cx).page_count()).expect("read PDF view");
    assert_eq!(page_count, 1, "PDF document did not finish loading");
    let screenshot = context.capture_screenshot(window.into()).expect("capture native PDF screenshot");
    let colored_pixels = screenshot.pixels().filter(|pixel| pixel[0].abs_diff(pixel[1]) > 30 || pixel[1].abs_diff(pixel[2]) > 30).count();
    assert!(colored_pixels > 10_000, "native PDF page rendered blank: only {colored_pixels} fixture-colored pixels were painted");
}
