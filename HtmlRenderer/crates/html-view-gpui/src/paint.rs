use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Bounds, Corners, Hsla, Pixels, RenderImage, Window, fill, point, px, rgba, size};
use html_view_core::{GlyphId, Painter};
use image::{Frame, RgbaImage};
use kurbo::{Point, Rect};
use peniko::{Color, Image as PenikoImage};
use smallvec::SmallVec;

use crate::text::GlyphStore;

#[derive(Clone)]
pub(crate) enum PaintCommand {
    Fill { rect: Rect, color: Hsla },
    Glyph { glyph: GlyphId, origin: Point },
    Image { rect: Rect, image: Arc<RenderImage> },
}

pub(crate) struct GpuiCommandPainter<'a> {
    images: &'a mut HashMap<Vec<u8>, Arc<RenderImage>>,
    pub(crate) commands: Vec<PaintCommand>,
}

impl<'a> GpuiCommandPainter<'a> {
    pub(crate) fn new(images: &'a mut HashMap<Vec<u8>, Arc<RenderImage>>) -> Self {
        Self { images, commands: Vec::new() }
    }
}

fn gpui_color(color: Color) -> Hsla {
    Hsla::from(rgba(u32::from_be_bytes([color.r, color.g, color.b, color.a])))
}

fn render_image(image: &PenikoImage) -> Option<Arc<RenderImage>> {
    let mut bytes = image.data.data().to_vec();
    if bytes.len() != image.width as usize * image.height as usize * 4 {
        return None;
    }
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(image.width, image.height, bytes)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_vec(vec![Frame::new(buffer)]))))
}

impl Painter for GpuiCommandPainter<'_> {
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.commands.push(PaintCommand::Fill { rect, color: gpui_color(color) });
    }

    fn draw_glyph(&mut self, glyph: GlyphId, origin: Point) {
        self.commands.push(PaintCommand::Glyph { glyph, origin });
    }

    fn draw_image(&mut self, image: &PenikoImage, hash: &[u8], rect: Rect) {
        let key = hash.to_vec();
        let rendered = self.images.get(&key).cloned().or_else(|| {
            let rendered = render_image(image)?;
            self.images.insert(key, rendered.clone());
            Some(rendered)
        });
        if let Some(image) = rendered {
            self.commands.push(PaintCommand::Image { rect, image });
        }
    }
}

pub(crate) fn paint_commands(commands: &[PaintCommand], glyph_store: &GlyphStore, bounds: Bounds<Pixels>, window: &mut Window) {
    let glyphs = glyph_store.borrow();
    for command in commands {
        match command {
            PaintCommand::Fill { rect, color } => {
                let rect = Bounds { origin: point(bounds.origin.x + px(rect.x0 as f32), bounds.origin.y + px(rect.y0 as f32)), size: size(px(rect.width() as f32), px(rect.height() as f32)) };
                window.paint_quad(fill(rect, *color));
            }
            PaintCommand::Glyph { glyph, origin } => {
                let Some(glyph) = glyphs.get(*glyph as usize) else {
                    debug_assert!(false, "paint command referenced a missing interned glyph");
                    continue;
                };
                let origin = point(bounds.origin.x + px(origin.x as f32), bounds.origin.y + px(origin.y as f32) + glyph.layout.ascent);
                for run in &glyph.layout.runs {
                    for shaped_glyph in &run.glyphs {
                        let glyph_origin = point(origin.x + shaped_glyph.position.x, origin.y + shaped_glyph.position.y);
                        if shaped_glyph.is_emoji {
                            let _ = window.paint_emoji(glyph_origin, run.font_id, shaped_glyph.id, glyph.layout.font_size);
                        } else {
                            let _ = window.paint_glyph(glyph_origin, run.font_id, shaped_glyph.id, glyph.layout.font_size, glyph.color);
                        }
                    }
                }
            }
            PaintCommand::Image { rect, image } => {
                let image_bounds = Bounds { origin: point(bounds.origin.x + px(rect.x0 as f32), bounds.origin.y + px(rect.y0 as f32)), size: size(px(rect.width() as f32), px(rect.height() as f32)) };
                let _ = window.paint_image(image_bounds, Corners::default(), image.clone(), 0, false);
            }
        }
    }
}
