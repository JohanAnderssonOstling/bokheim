use crate::text::glyph_cache::GlyphCache;
use floem::context::PaintCx;
use floem_renderer::Renderer;
pub(crate) use html_view_core::Painter;
use html_view_core::UsedBorderRadii;
use kurbo::{BezPath, Point, Rect, Stroke};
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
    fn push_clip(&mut self, rect: Rect) {
        self.cx.save();
        self.cx.clip(&rect);
    }

    fn pop_clip(&mut self) {
        self.cx.restore();
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        use std::ops::DerefMut;
        self.cx.deref_mut().fill(&rect, color, 0.0);
    }

    fn fill_rounded_rect(&mut self, rect: Rect, radii: UsedBorderRadii, color: Color) {
        use std::ops::DerefMut;
        self.cx.deref_mut().fill(&rounded_path(rect, radii), color, 0.0);
    }

    fn stroke_rounded_rect(&mut self, rect: Rect, radii: UsedBorderRadii, width: f32, color: Color) {
        use std::ops::DerefMut;
        let half = width as f64 / 2.0;
        if half <= 0.0 || rect.width() <= width as f64 || rect.height() <= width as f64 {
            return;
        }
        let inset = Rect::new(rect.x0 + half, rect.y0 + half, rect.x1 - half, rect.y1 - half);
        let center = radii.map(|x, y| ((x - half as f32).max(0.0), (y - half as f32).max(0.0)));
        self.cx.deref_mut().stroke(&rounded_path(inset, center), color, &Stroke::new(width as f64));
    }

    fn draw_glyph(&mut self, glyph: html_view_core::text_backend::GlyphId, origin: Point) {
        use std::ops::DerefMut;
        self.cx.deref_mut().draw_text(self.glyph_cache.get(glyph), origin);
    }

    fn draw_image(&mut self, image: &Image, hash: &[u8], rect: Rect) {
        use std::ops::DerefMut;
        self.cx.deref_mut().draw_img(floem_renderer::Img { img: image.clone(), hash }, rect);
    }
}

fn rounded_path(rect: Rect, radii: UsedBorderRadii) -> BezPath {
    const K: f64 = 0.552_284_749_830_793_6;
    let [(tlx, tly), (trx, try_), (brx, bry), (blx, bly)] = [radii.top_left, radii.top_right, radii.bottom_right, radii.bottom_left].map(|(x, y)| (x as f64, y as f64));
    let mut path = BezPath::new();
    path.move_to((rect.x0 + tlx, rect.y0));
    path.line_to((rect.x1 - trx, rect.y0));
    path.curve_to((rect.x1 - trx + trx * K, rect.y0), (rect.x1, rect.y0 + try_ - try_ * K), (rect.x1, rect.y0 + try_));
    path.line_to((rect.x1, rect.y1 - bry));
    path.curve_to((rect.x1, rect.y1 - bry + bry * K), (rect.x1 - brx + brx * K, rect.y1), (rect.x1 - brx, rect.y1));
    path.line_to((rect.x0 + blx, rect.y1));
    path.curve_to((rect.x0 + blx - blx * K, rect.y1), (rect.x0, rect.y1 - bly + bly * K), (rect.x0, rect.y1 - bly));
    path.line_to((rect.x0, rect.y0 + tly));
    path.curve_to((rect.x0, rect.y0 + tly - tly * K), (rect.x0 + tlx - tlx * K, rect.y0), (rect.x0 + tlx, rect.y0));
    path.close_path();
    path
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
        let frame = renderer.renderer.prepare_frame(Size::new(800.0, 600.0));
        let mut painter = RecordingPainter::default();
        frame.paint(&mut painter);

        let view = renderer.document().render_view();
        let visible: String = painter.glyphs.iter().map(|(glyph, _)| view.text().glyph_metric(*glyph).expect("recorded glyph is registered").ch()).collect();
        assert!(visible.contains("Recorded text"), "recorded glyphs were {visible:?}");

        fs::remove_dir_all(fixture).unwrap();
    }
}
