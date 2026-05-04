use floem::context::PaintCx;
use floem::kurbo::{Point, Rect};
use floem::peniko::Image as PenikoImage;
use floem_renderer::Renderer;
use floem_renderer::tiny_skia;
use floem::prelude::Color;
use floem::text::TextLayout;

pub(crate) trait Painter {
    fn fill_rect(&mut self, rect: Rect, color: Color);
    fn draw_text(&mut self, layout: &TextLayout, point: Point);
    fn draw_image(&mut self, image: &PenikoImage, hash: &[u8], rect: Rect);
}

impl Painter for PaintCx<'_> {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        use std::ops::DerefMut;
        self.deref_mut().fill(&rect, &color, 0.0);
    }

    fn draw_text(&mut self, layout: &TextLayout, point: Point) {
        use std::ops::DerefMut;
        self.deref_mut().draw_text(layout, point);
    }

    fn draw_image(&mut self, image: &PenikoImage, hash: &[u8], rect: Rect) {
        use std::ops::DerefMut;
        self.deref_mut().draw_img(floem_renderer::Img { img: image.clone(), hash }, rect);
    }
}

pub struct SoftwarePainter {
    pixmap: tiny_skia::Pixmap,
}

impl SoftwarePainter {
    pub fn new(width: u32, height: u32) -> Self {
        let mut pixmap = tiny_skia::Pixmap::new(width, height).expect("failed to create pixmap");
        pixmap.fill(tiny_skia::Color::WHITE);
        Self { pixmap }
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, String> {
        self.pixmap.encode_png().map_err(|e| e.to_string())
    }
}

impl Painter for SoftwarePainter {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let Some(skia_rect) = tiny_skia::Rect::from_xywh(rect.x0 as f32, rect.y0 as f32, rect.width() as f32, rect.height() as f32) else {
            return;
        };
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a));
        paint.blend_mode = tiny_skia::BlendMode::SourceOver;
        self.pixmap.fill_rect(skia_rect, &paint, tiny_skia::Transform::identity(), None);
    }

    fn draw_text(&mut self, _layout: &TextLayout, _point: Point) {
        // TODO: Software text rendering path needs adaptation to Floem main's text internals.
    }

    fn draw_image(&mut self, image: &PenikoImage, _hash: &[u8], rect: Rect) {
        if image.width == 0 || image.height == 0 {
            return;
        }
        let src_data = image.data.data();
        if src_data.len() != (image.width * image.height * 4) as usize {
            return;
        }
        // Premultiply alpha for tiny-skia's premultiplied RGBA format
        let premultiplied: Vec<u8> = src_data
            .chunks_exact(4)
            .flat_map(|px| {
                let a = px[3] as u32;
                [(px[0] as u32 * a / 255) as u8, (px[1] as u32 * a / 255) as u8, (px[2] as u32 * a / 255) as u8, px[3]]
            })
            .collect();
        let Some(src) = tiny_skia::PixmapRef::from_bytes(&premultiplied, image.width, image.height) else {
            return;
        };
        let sx = rect.width() as f32 / image.width as f32;
        let sy = rect.height() as f32 / image.height as f32;
        self.pixmap.draw_pixmap(rect.x0 as i32, rect.y0 as i32, src, &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::from_scale(sx, sy), None);
    }
}
