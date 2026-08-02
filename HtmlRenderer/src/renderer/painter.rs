use crate::text::glyph_cache::GlyphCache;
use floem::context::PaintCx;
use floem_renderer::Renderer;
pub(crate) use html_view_core::Painter;
use kurbo::{Point, Rect};
use peniko::{Color, Image};

pub(crate) struct FloemPainter<'borrow, 'renderer, 'cache> {
    cx: &'borrow mut PaintCx<'renderer>,
    glyph_cache: &'cache GlyphCache,
}

impl<'borrow, 'renderer, 'cache> FloemPainter<'borrow, 'renderer, 'cache> {
    pub(crate) fn new(cx: &'borrow mut PaintCx<'renderer>, glyph_cache: &'cache GlyphCache) -> Self {
        Self { cx, glyph_cache }
    }
}

impl Painter for FloemPainter<'_, '_, '_> {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        use std::ops::DerefMut;
        self.cx.deref_mut().fill(&rect, &color, 0.0);
    }

    fn draw_glyph(&mut self, glyph: crate::document::GlyphId, origin: Point) {
        use std::ops::DerefMut;
        self.cx.deref_mut().draw_text(self.glyph_cache.get(glyph), origin);
    }

    fn draw_image(&mut self, image: &Image, hash: &[u8], rect: Rect) {
        use std::ops::DerefMut;
        self.cx.deref_mut().draw_img(floem_renderer::Img { img: image.clone(), hash }, rect);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use html_view_core::RecordingPainter;
    use kurbo::Size;

    use super::super::HTMLRenderer;

    #[test]
    fn recording_painter_captures_visible_page_text() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let fixture = std::env::temp_dir().join(format!("html-renderer-recording-{nonce}"));
        fs::create_dir_all(&fixture).unwrap();
        fs::write(fixture.join("index.html"), "<html><body><p>Recorded text</p></body></html>").unwrap();

        let mut renderer = HTMLRenderer::from_source_path(fixture.clone());
        renderer.renderer.set_line_numbers_enabled(false);
        let frame = renderer.renderer.prepare_frame(Size::new(800.0, 600.0));
        let mut painter = RecordingPainter::default();
        frame.paint(&mut painter);

        let view = renderer.document().render_view();
        let visible: String = painter.glyphs.iter().map(|(glyph, _)| view.glyph_metric(*glyph).expect("recorded glyph is registered").ch()).collect();
        assert!(visible.contains("Recorded text"), "recorded glyphs were {visible:?}");

        fs::remove_dir_all(fixture).unwrap();
    }
}
