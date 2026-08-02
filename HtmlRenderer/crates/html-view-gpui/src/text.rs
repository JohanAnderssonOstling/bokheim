use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Hsla, TextRun, font, px, rgba};
use html_view_core::layout::ShapeError;
use html_view_core::{FontStyle, GlyphId, GlyphMetric, GlyphRegistry, GlyphShaper, StyleStringId};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct GlyphKey {
    ch: char,
    font_size: u32,
    font_weight: u16,
    font_style: u8,
    color: u32,
    family: String,
}

pub(crate) struct GpuiGlyph {
    pub(crate) layout: Arc<gpui::LineLayout>,
    pub(crate) color: Hsla,
}

pub(crate) type GlyphStore = Rc<RefCell<Vec<GpuiGlyph>>>;

pub(crate) struct GpuiGlyphCache {
    text_system: Arc<gpui::WindowTextSystem>,
    font_families: HashMap<String, String>,
    table: HashMap<GlyphKey, GlyphId>,
    glyphs: GlyphStore,
}

impl GpuiGlyphCache {
    pub(crate) fn new(text_system: Arc<gpui::WindowTextSystem>) -> Self {
        let font_families = text_system.all_font_names().into_iter().map(|family| (family.to_lowercase(), family)).collect();
        Self { text_system, font_families, table: HashMap::new(), glyphs: Rc::new(RefCell::new(Vec::new())) }
    }

    pub(crate) fn store(&self) -> GlyphStore {
        self.glyphs.clone()
    }

    fn available_family(&self, candidates: &[&str]) -> Option<String> {
        candidates.iter().find_map(|candidate| self.font_families.get(&candidate.to_lowercase()).cloned())
    }

    fn resolve_family(&self, family_list: Option<&str>) -> String {
        for family in family_list.into_iter().flat_map(|list| list.split(',')) {
            let family = family.trim().trim_matches(['\'', '"']);
            if family.is_empty() {
                continue;
            }
            let generic = match family.to_ascii_lowercase().as_str() {
                "serif" => self.available_family(&["Libertinus Serif", "Noto Serif", "DejaVu Serif", "Times New Roman"]),
                "sans-serif" => self.available_family(&["Noto Sans", "DejaVu Sans", "Arial", ".SystemUIFont"]),
                "monospace" => self.available_family(&["Noto Sans Mono", "DejaVu Sans Mono", ".ZedMono"]),
                "system-ui" | "ui-sans-serif" => Some(".SystemUIFont".to_owned()),
                "ui-serif" => self.available_family(&["Libertinus Serif", "Noto Serif", "DejaVu Serif"]),
                _ => None,
            };
            if let Some(generic) = generic {
                return generic;
            }
            if let Some(available) = self.font_families.get(&family.to_lowercase()) {
                return available.clone();
            }
        }
        ".SystemUIFont".to_owned()
    }
}

impl GlyphShaper for GpuiGlyphCache {
    fn reset(&mut self) {
        self.table.clear();
        self.glyphs.borrow_mut().clear();
    }

    fn shape_glyph<'a>(
        &mut self, glyph_metrics: &mut GlyphRegistry<'a>, ch: char, font_size: f32, font_weight: u16, font_style: FontStyle, color: u32, _font_family: Option<StyleStringId>, family: Option<&str>,
    ) -> Result<GlyphId, ShapeError> {
        let family = self.resolve_family(family);
        let font_style_key = match font_style {
            FontStyle::Normal => 0,
            FontStyle::Italic => 1,
        };
        let key = GlyphKey { ch, font_size: font_size.to_bits(), font_weight, font_style: font_style_key, color, family: family.clone() };
        if let Some(glyph) = self.table.get(&key) {
            if glyph_metrics.contains(*glyph) {
                return Ok(*glyph);
            }
        }

        let mut gpui_font = font(family);
        gpui_font.weight = gpui::FontWeight(font_weight as f32);
        gpui_font.style = match font_style {
            FontStyle::Normal => gpui::FontStyle::Normal,
            FontStyle::Italic => gpui::FontStyle::Italic,
        };
        let mut utf8 = [0; 4];
        let text = ch.encode_utf8(&mut utf8);
        let color = Hsla::from(rgba(color));
        let run = TextRun { len: text.len(), font: gpui_font, color, background_color: None, underline: None, strikethrough: None };
        let layout = self.text_system.layout_line(text, px(font_size), &[run], None);

        let mut glyphs = self.glyphs.borrow_mut();
        let glyph = glyphs.len() as GlyphId;
        let metric = GlyphMetric::try_new(ch, f32::from(layout.width), f32::from(layout.ascent), f32::from(layout.descent), f32::from(layout.ascent)).map_err(ShapeError::rejected_metric)?;
        let registry_glyph = glyph_metrics.register(metric)?;
        debug_assert_eq!(glyph, registry_glyph);
        glyphs.push(GpuiGlyph { layout, color });
        drop(glyphs);

        self.table.insert(key, registry_glyph);
        Ok(registry_glyph)
    }
}
