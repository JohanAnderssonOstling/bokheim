use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use gpui::{Bounds, ContentMask, Corners, Edges, Hsla, Pixels, RenderImage, SvgRenderer, Window, fill, point, px, rgba, size};
use html_view_core::text_backend::{GlyphId, TextRunId};
use html_view_core::{Painter, UsedBorderRadii};
use image::{Frame, RgbaImage, imageops::FilterType};
use kurbo::{Point, Rect};
use peniko::{Color, Image as PenikoImage};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use smallvec::SmallVec;

use crate::text::{GlyphStore, GpuiGlyph, GpuiTextRun, RunStore};

#[derive(Clone)]
pub(crate) enum PaintCommand {
    PushClip { rect: Rect },
    PopClip,
    Fill { rect: Rect, color: Hsla },
    RoundedFill { rect: Rect, radii: UsedBorderRadii, color: Hsla },
    RoundedBorder { rect: Rect, radii: UsedBorderRadii, width: f32, color: Hsla },
    Glyph { glyph: GlyphId, origin: Point, color: Option<Hsla> },
    TextRun { run: Arc<GpuiTextRun>, range: Range<u32>, origin: Point, color: Option<Hsla> },
    Image { rect: Rect, image: Arc<RenderImage> },
}

pub(crate) struct GpuiCommandPainter<'a> {
    images: Option<&'a mut HashMap<Vec<u8>, Arc<RenderImage>>>,
    text_runs: Option<RunStore>,
    pub(crate) commands: Vec<PaintCommand>,
}

impl<'a> GpuiCommandPainter<'a> {
    pub(crate) fn new(images: &'a mut HashMap<Vec<u8>, Arc<RenderImage>>, text_runs: RunStore, mut commands: Vec<PaintCommand>) -> Self {
        commands.clear();
        Self { images: Some(images), text_runs: Some(text_runs), commands }
    }

    pub(crate) fn commands_only(mut commands: Vec<PaintCommand>) -> Self {
        commands.clear();
        Self { images: None, text_runs: None, commands }
    }

    pub(crate) fn into_commands(self) -> Vec<PaintCommand> {
        self.commands
    }
}

fn gpui_color(color: Color) -> Hsla {
    Hsla::from(rgba(u32::from_be_bytes([color.r, color.g, color.b, color.a])))
}

fn render_image(image: &PenikoImage, target_width: u32, target_height: u32) -> Option<Arc<RenderImage>> {
    let mut bytes = image.data.data().to_vec();
    if bytes.len() != image.width as usize * image.height as usize * 4 {
        return None;
    }
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(image.width, image.height, bytes)?;
    let buffer = if image.width == target_width && image.height == target_height { buffer } else { image::imageops::resize(&buffer, target_width, target_height, FilterType::Triangle) };
    Some(Arc::new(RenderImage::new(SmallVec::from_vec(vec![Frame::new(buffer)]))))
}

fn scaled_image_cache_key(hash: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(hash.len() + 9);
    key.push(b'r');
    key.extend_from_slice(&width.to_le_bytes());
    key.extend_from_slice(&height.to_le_bytes());
    key.extend_from_slice(hash);
    key
}

fn render_svg(bytes: &[u8], _intrinsic_size: (u32, u32), target_width: u32, target_height: u32) -> Option<Arc<RenderImage>> {
    let bytes = svg_with_viewport(bytes, target_width, target_height)?;
    SvgRenderer::new(Arc::new(())).render_single_frame(&bytes, 1.0).ok()
}

fn svg_with_viewport(bytes: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut reader = Reader::from_reader(bytes);
    let mut writer = Writer::new(Vec::with_capacity(bytes.len() + 32));
    let mut root_rewritten = false;
    loop {
        let event = reader.read_event().ok()?;
        let event = match event {
            Event::Start(start) if !root_rewritten && start.local_name().as_ref() == b"svg" => {
                root_rewritten = true;
                Event::Start(svg_start_with_viewport(&start, width, height)?)
            }
            Event::Empty(start) if !root_rewritten && start.local_name().as_ref() == b"svg" => {
                root_rewritten = true;
                Event::Empty(svg_start_with_viewport(&start, width, height)?)
            }
            Event::Eof => break,
            event => event.into_owned(),
        };
        writer.write_event(event).ok()?;
    }
    root_rewritten.then(|| writer.into_inner())
}

fn svg_start_with_viewport(start: &BytesStart<'_>, width: u32, height: u32) -> Option<BytesStart<'static>> {
    let name = std::str::from_utf8(start.name().as_ref()).ok()?.to_owned();
    let mut rewritten = BytesStart::new(name);
    for attribute in start.attributes().with_checks(false) {
        let attribute = attribute.ok()?;
        if !matches!(attribute.key.local_name().as_ref(), b"width" | b"height") {
            rewritten.push_attribute(attribute);
        }
    }
    let width = width.to_string();
    let height = height.to_string();
    rewritten.push_attribute(("width", width.as_str()));
    rewritten.push_attribute(("height", height.as_str()));
    Some(rewritten.into_owned())
}

impl Painter for GpuiCommandPainter<'_> {
    fn push_clip(&mut self, rect: Rect) {
        self.commands.push(PaintCommand::PushClip { rect });
    }

    fn pop_clip(&mut self) {
        self.commands.push(PaintCommand::PopClip);
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.commands.push(PaintCommand::Fill { rect, color: gpui_color(color) });
    }

    fn fill_rounded_rect(&mut self, rect: Rect, radii: UsedBorderRadii, color: Color) {
        self.commands.push(PaintCommand::RoundedFill { rect, radii, color: gpui_color(color) });
    }

    fn stroke_rounded_rect(&mut self, rect: Rect, radii: UsedBorderRadii, width: f32, color: Color) {
        self.commands.push(PaintCommand::RoundedBorder { rect, radii, width, color: gpui_color(color) });
    }

    fn draw_glyph(&mut self, glyph: GlyphId, origin: Point) {
        self.commands.push(PaintCommand::Glyph { glyph, origin, color: None });
    }

    fn draw_glyph_with_color(&mut self, glyph: GlyphId, origin: Point, color: u32) {
        self.commands.push(PaintCommand::Glyph { glyph, origin, color: Some(Hsla::from(rgba(color))) });
    }

    fn supports_text_runs(&self) -> bool {
        true
    }

    fn draw_text_run(&mut self, run: TextRunId, origin: Point, color: Option<u32>) {
        self.draw_text_run_fragment(run, 0..u32::MAX, origin, color);
    }

    fn draw_text_run_fragment(&mut self, run: TextRunId, range: Range<u32>, origin: Point, color: Option<u32>) {
        let Some(run) = self.text_runs.as_ref().and_then(|runs| runs.borrow().get(run as usize).cloned()) else {
            debug_assert!(false, "renderer referenced a missing shaped text run");
            return;
        };
        self.commands.push(PaintCommand::TextRun { run, range, origin, color: color.map(|color| Hsla::from(rgba(color))) });
    }

    fn draw_image(&mut self, image: &PenikoImage, hash: &[u8], rect: Rect) {
        let Some(images) = self.images.as_deref_mut() else {
            debug_assert!(false, "an image was emitted into a command-only paint layer");
            return;
        };
        let target_width = rect.width().round().clamp(1.0, u32::MAX as f64) as u32;
        let target_height = rect.height().round().clamp(1.0, u32::MAX as f64) as u32;
        let cache_key = scaled_image_cache_key(hash, target_width, target_height);
        let rendered = images.get(&cache_key).cloned().or_else(|| {
            let rendered = render_image(image, target_width, target_height)?;
            images.insert(cache_key, rendered.clone());
            Some(rendered)
        });
        if let Some(image) = rendered {
            self.commands.push(PaintCommand::Image { rect, image });
        }
    }

    fn draw_svg(&mut self, bytes: &[u8], hash: &[u8], intrinsic_size: (u32, u32), rect: Rect) {
        let target_width = rect.width().round().clamp(1.0, u32::MAX as f64) as u32;
        let target_height = rect.height().round().clamp(1.0, u32::MAX as f64) as u32;
        let mut cache_key = scaled_image_cache_key(hash, target_width, target_height);
        cache_key[0] = b's';
        let rendered = match self.images.as_deref_mut() {
            Some(images) => images.get(&cache_key).cloned().or_else(|| {
                let rendered = render_svg(bytes, intrinsic_size, target_width, target_height)?;
                images.insert(cache_key, rendered.clone());
                Some(rendered)
            }),
            None => render_svg(bytes, intrinsic_size, target_width, target_height),
        };
        if let Some(image) = rendered {
            self.commands.push(PaintCommand::Image { rect, image });
        }
    }
}

pub(crate) fn paint_commands(commands: &[PaintCommand], glyph_store: &GlyphStore, bounds: Bounds<Pixels>, window: &mut Window) {
    let started = Instant::now();
    let glyphs = glyph_store.borrow();
    paint_commands_inner(commands, &glyphs, bounds, window);
    let duration_ms = started.elapsed().as_millis();
    if duration_ms >= 10 {
        println!("HTML_GPUI_SCENE commands={} duration_ms={duration_ms}", commands.len());
    }
}

fn paint_commands_inner(commands: &[PaintCommand], glyphs: &[GpuiGlyph], bounds: Bounds<Pixels>, window: &mut Window) {
    let mut index = 0usize;
    while index < commands.len() {
        let command = &commands[index];
        match command {
            PaintCommand::PushClip { rect } => {
                let mut depth = 1usize;
                let mut end = index + 1;
                while end < commands.len() && depth > 0 {
                    match &commands[end] {
                        PaintCommand::PushClip { .. } => depth += 1,
                        PaintCommand::PopClip => depth -= 1,
                        _ => {}
                    }
                    end += 1;
                }
                debug_assert_eq!(depth, 0, "paint clip commands must be balanced");
                let inner_end = end.saturating_sub(1);
                let clip_bounds = Bounds { origin: point(bounds.origin.x + px(rect.x0 as f32), bounds.origin.y + px(rect.y0 as f32)), size: size(px(rect.width().max(0.0) as f32), px(rect.height().max(0.0) as f32)) };
                window.with_content_mask(Some(ContentMask { bounds: clip_bounds }), |window| paint_commands_inner(&commands[index + 1..inner_end], glyphs, bounds, window));
                index = end;
                continue;
            }
            PaintCommand::PopClip => {
                debug_assert!(false, "unmatched paint clip pop");
            }
            PaintCommand::Fill { rect, color } => {
                let rect = Bounds { origin: point(bounds.origin.x + px(rect.x0 as f32), bounds.origin.y + px(rect.y0 as f32)), size: size(px(rect.width() as f32), px(rect.height() as f32)) };
                window.paint_quad(fill(rect, *color));
            }
            PaintCommand::RoundedFill { rect, radii, color } => {
                let rect = Bounds { origin: point(bounds.origin.x + px(rect.x0 as f32), bounds.origin.y + px(rect.y0 as f32)), size: size(px(rect.width() as f32), px(rect.height() as f32)) };
                window.paint_quad(fill(rect, *color).corner_radii(gpui_radii(*radii)));
            }
            PaintCommand::RoundedBorder { rect, radii, width, color } => {
                let rect = Bounds { origin: point(bounds.origin.x + px(rect.x0 as f32), bounds.origin.y + px(rect.y0 as f32)), size: size(px(rect.width() as f32), px(rect.height() as f32)) };
                let widths = Edges { top: px(*width), right: px(*width), bottom: px(*width), left: px(*width) };
                window.paint_quad(fill(rect, rgba(0)).corner_radii(gpui_radii(*radii)).border_widths(widths).border_color(*color));
            }
            PaintCommand::Glyph { glyph, origin, color } => {
                let Some(glyph) = glyphs.get(*glyph as usize) else {
                    debug_assert!(false, "paint command referenced a missing interned glyph");
                    continue;
                };
                match glyph {
                    GpuiGlyph::Empty => {}
                    GpuiGlyph::Layout { layout, color: glyph_color } => {
                        let origin = point(bounds.origin.x + px(origin.x as f32), bounds.origin.y + px(origin.y as f32) + layout.ascent);
                        for run in &layout.runs {
                            for shaped_glyph in &run.glyphs {
                                let glyph_origin = point(origin.x + shaped_glyph.position.x, origin.y + shaped_glyph.position.y);
                                if shaped_glyph.is_emoji {
                                    let _ = window.paint_emoji(glyph_origin, run.font_id, shaped_glyph.id, layout.font_size);
                                } else {
                                    let _ = window.paint_glyph(glyph_origin, run.font_id, shaped_glyph.id, layout.font_size, (*color).unwrap_or(*glyph_color));
                                }
                            }
                        }
                    }
                    GpuiGlyph::RunFragment { run, range, natural_offset, color: glyph_color } => {
                        let baseline = point(bounds.origin.x + px(origin.x as f32), bounds.origin.y + px(origin.y as f32) + run.ascent);
                        let start = run.character_bytes.get(range.start as usize).copied().unwrap_or(0);
                        let end = run.character_bytes.get(range.end as usize).copied().unwrap_or(usize::MAX);
                        for shaped_glyph in run.glyphs.iter().filter(|candidate| candidate.byte_index >= start && candidate.byte_index < end) {
                            let glyph_origin = point(baseline.x + shaped_glyph.position.x - px(*natural_offset), baseline.y + shaped_glyph.position.y);
                            if shaped_glyph.is_emoji {
                                let _ = window.paint_emoji(glyph_origin, shaped_glyph.font_id, shaped_glyph.glyph_id, shaped_glyph.font_size);
                            } else {
                                let _ = window.paint_glyph(glyph_origin, shaped_glyph.font_id, shaped_glyph.glyph_id, shaped_glyph.font_size, (*color).unwrap_or(*glyph_color));
                            }
                        }
                    }
                }
            }
            PaintCommand::TextRun { run, range, origin, color } => {
                let baseline = point(bounds.origin.x + px(origin.x as f32), bounds.origin.y + px(origin.y as f32) + run.ascent);
                let start = run.character_bytes.get(range.start as usize).copied().unwrap_or(0);
                let end = run.character_bytes.get(range.end as usize).copied().unwrap_or(usize::MAX);
                for glyph in run.glyphs.iter().filter(|glyph| glyph.byte_index >= start && glyph.byte_index < end) {
                    let glyph_origin = point(baseline.x + glyph.position.x, baseline.y + glyph.position.y);
                    if glyph.is_emoji {
                        let _ = window.paint_emoji(glyph_origin, glyph.font_id, glyph.glyph_id, glyph.font_size);
                    } else {
                        let glyph_color = color.unwrap_or_else(|| run.color_at(glyph.byte_index));
                        let _ = window.paint_glyph(glyph_origin, glyph.font_id, glyph.glyph_id, glyph.font_size, glyph_color);
                    }
                }
            }
            PaintCommand::Image { rect, image } => {
                let image_bounds = Bounds { origin: point(bounds.origin.x + px(rect.x0 as f32), bounds.origin.y + px(rect.y0 as f32)), size: size(px(rect.width() as f32), px(rect.height() as f32)) };
                let _ = window.paint_image(image_bounds, Corners::default(), image.clone(), 0, false);
            }
        }
        index += 1;
    }
}

fn gpui_radii(radii: UsedBorderRadii) -> Corners<Pixels> {
    // GPUI's quad primitive has circular rather than elliptical corners. The
    // smaller axis stays inside the CSS ellipse and avoids overpainting.
    Corners {
        top_left: px(radii.top_left.0.min(radii.top_left.1)),
        top_right: px(radii.top_right.0.min(radii.top_right.1)),
        bottom_right: px(radii.bottom_right.0.min(radii.bottom_right.1)),
        bottom_left: px(radii.bottom_left.0.min(radii.bottom_left.1)),
    }
}

#[cfg(test)]
mod tests {
    use super::{render_svg, svg_with_viewport};

    const RELATIVE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" height="50%"><rect width="200" height="100" fill="blue"/></svg>"#;

    #[test]
    fn rewrites_svg_root_to_the_used_css_viewport() {
        let rewritten = svg_with_viewport(RELATIVE_SVG, 300, 96).expect("valid SVG");
        let rewritten = std::str::from_utf8(&rewritten).unwrap();
        assert!(rewritten.contains("width=\"300\""));
        assert!(rewritten.contains("height=\"96\""));
        assert!(!rewritten.contains("height=\"50%\""));
    }

    #[test]
    fn renders_full_color_svg_at_the_used_css_viewport() {
        assert!(render_svg(RELATIVE_SVG, (300, 150), 300, 96).is_some());
    }
}
