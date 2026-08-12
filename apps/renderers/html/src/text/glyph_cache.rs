use crate::document::MemoryUsageReport;
use floem::text::{Attrs, AttrsList, FamilyOwned, Style as TextFontStyle, TextLayout};
use html_view_core::layout::ShapeError;
use html_view_core::text_backend::{FontSlant, GlyphId, GlyphMetric, GlyphRegistry, GlyphShaper};
use lightningcss::properties::text::TextAlign;
use peniko::Color;
use rustc_data_structures::fx::FxHashMap;
use std::mem::size_of;
use std::time::{Duration, Instant};

/// Fixed-point scale for the font-size component of the glyph cache key.
///
/// The size is quantized to quarter-pixel granularity rather than truncated to
/// a whole pixel: this preserves fractional CSS sizes (which are pervasive in
/// EPUB CSS via `em`/`rem`/`%`) instead of flooring them, and lifts the old
/// 255px ceiling to ~16383px. Glyphs are always shaped at the quantized size so
/// the key and the rasterized layout agree.
const FONT_SIZE_SCALE: f32 = 4.0;

/// Quantize a font size (px) to the cache-key representation.
fn quantize_font_size(font_size: f32) -> u16 {
    (font_size * FONT_SIZE_SCALE).round().clamp(0.0, u16::MAX as f32) as u16
}

/// Recover the px size a quantized key was shaped at.
fn quantized_font_size_px(quantized: u16) -> f32 {
    quantized as f32 / FONT_SIZE_SCALE
}

/// Tracks current style state during HTML parsing
pub struct ParseState {
    pub font_weight: u16,
    pub text_style: TextFontStyle,
    pub root_font_size: f32,
    pub text_align: TextAlign,
    pub width: f64,
}

impl Default for ParseState {
    fn default() -> Self {
        Self {
            font_weight: 400, // normal weight
            text_style: TextFontStyle::Normal,
            root_font_size: 12.,
            text_align: TextAlign::Left,
            width: 600.,
        }
    }
}

pub struct GlyphCache {
    table: FxHashMap<(char, u16, u16, u8, u32, String), GlyphId>,
    family_cache: FxHashMap<String, Vec<FamilyOwned>>,
    reverse: Vec<TextLayout>,
    pub layout_time: Duration,
    previous_document: Option<GlyphDocumentResources>,
}

struct GlyphDocumentResources {
    table: FxHashMap<(char, u16, u16, u8, u32, String), GlyphId>,
    reverse: Vec<TextLayout>,
    layout_time: Duration,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self { table: FxHashMap::default(), family_cache: FxHashMap::default(), reverse: Vec::with_capacity(100), layout_time: Duration::ZERO, previous_document: None }
    }
}

/// Extract real vertical metrics from a shaped single-glyph layout.
///
/// Falls back to the layout-box height (with a zero descent) when the layout
/// produced no run, preserving the previous behavior for degenerate glyphs.
fn metrics_from_layout(ch: char, layout: &TextLayout) -> GlyphMetric {
    let advance = layout.size().width as f32;
    match layout.layout_runs().next() {
        Some(run) => GlyphMetric::try_new(ch, advance, run.max_ascent, run.max_descent, run.line_y).expect("glyph layout metrics must be non-negative"),
        None => {
            let height = layout.size().height as f32;
            GlyphMetric::try_new(ch, advance, height, 0.0, height).expect("glyph layout metrics must be non-negative")
        }
    }
}

impl GlyphCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get_or_insert(&mut self, metrics: &mut GlyphRegistry<'_>, char: char, mut font: Attrs, parse_state: &ParseState) -> (&TextLayout, GlyphId) {
        use std::collections::hash_map::Entry;

        let font_size = quantize_font_size(font.font_size);
        font = font.font_size(quantized_font_size_px(font_size));
        font = font.raw_weight(parse_state.font_weight);
        font = font.style(parse_state.text_style);

        let style_key = match parse_state.text_style {
            TextFontStyle::Normal => 0,
            TextFontStyle::Italic => 1,
            TextFontStyle::Oblique => 2,
        };
        let key = (char, font_size, parse_state.font_weight, style_key, 0x000000FF, String::new());
        let index = match self.table.entry(key) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let start = Instant::now();

                let index = self.reverse.len() as GlyphId;
                let mut layout = TextLayout::new();
                let mut buf = [0u8; 4];
                let s = char.encode_utf8(&mut buf);

                layout.set_text(s, AttrsList::new(font));
                let metric = metrics_from_layout(char, &layout);
                let metric_index = metrics.register(metric).expect("glyph metric table capacity exceeded");
                debug_assert_eq!(index, metric_index);
                self.reverse.push(layout);

                self.layout_time += start.elapsed();
                *e.insert(metric_index)
            }
        };
        (self.reverse.get(index as usize).unwrap(), index)
    }
    pub fn get(&self, index: GlyphId) -> &TextLayout {
        self.reverse.get(index as usize).unwrap()
    }

    /// Shape a character using font properties
    pub fn shape_with_font_properties(&mut self, metrics: &mut GlyphRegistry<'_>, c: char, font_size: f32, font_weight: u16, font_slant: FontSlant, color: u32, family: Option<&str>) -> Result<GlyphId, ShapeError> {
        use std::collections::hash_map::Entry;
        // Use provided properties
        let font_size = quantize_font_size(font_size);
        let text_style = if font_slant == FontSlant::Italic { TextFontStyle::Italic } else { TextFontStyle::Normal };

        let style_key = match text_style {
            TextFontStyle::Normal => 0,
            TextFontStyle::Italic => 1,
            TextFontStyle::Oblique => 2,
        };
        let family_key = family.unwrap_or_default().to_owned();
        let key = (c, font_size, font_weight, style_key, color, family_key.clone());
        let metric_id = match self.table.entry(key) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let start = Instant::now();

                let mut layout = TextLayout::new();
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);

                let color = Color::rgba8((color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8, (color & 0xFF) as u8);
                let mut font = Attrs::new().font_size(quantized_font_size_px(font_size)).raw_weight(font_weight).style(text_style).color(color);
                if let Some(family) = family {
                    let families = self.family_cache.entry(family_key).or_insert_with(|| FamilyOwned::parse_list(family).collect());
                    if !families.is_empty() {
                        font = font.family(families);
                    }
                }

                layout.set_text(s, AttrsList::new(font));
                let metric = metrics_from_layout(c, &layout);
                let metric_index = metrics.register(metric)?;
                debug_assert_eq!(self.reverse.len() as GlyphId, metric_index);
                self.reverse.push(layout);

                self.layout_time += start.elapsed();
                *e.insert(metric_index)
            }
        };
        Ok(metric_id)
    }

    pub fn len(&self) -> usize {
        self.reverse.len()
    }

    pub fn total_memory_usage(&self) -> usize {
        self.memory_usage_report().total_bytes()
    }

    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = MemoryUsageReport::new();
        report.add_slice_storage::<((char, u16, u16, u8, u32, String), GlyphId)>("GlyphCache.table.storage", self.table.capacity(), self.table.len());
        report.add_slice_storage::<(String, Vec<FamilyOwned>)>("GlyphCache.family_cache.storage", self.family_cache.capacity(), self.family_cache.len());
        let family_values_bytes: usize = self.family_cache.values().map(|families| families.capacity() * size_of::<FamilyOwned>()).sum();
        report.add("GlyphCache.family_cache.values.storage", family_values_bytes, self.family_cache.len());
        report.add_slice_storage::<TextLayout>("GlyphCache.reverse.storage", self.reverse.capacity(), self.reverse.len());
        report
    }
}

impl GlyphShaper for GlyphCache {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn begin_document_shaping(&mut self) {
        debug_assert!(self.previous_document.is_none(), "document shaping transactions cannot be nested");
        self.previous_document = Some(GlyphDocumentResources { table: std::mem::take(&mut self.table), reverse: std::mem::replace(&mut self.reverse, Vec::with_capacity(100)), layout_time: std::mem::take(&mut self.layout_time) });
    }

    fn commit_document_shaping(&mut self) {
        self.previous_document = None;
    }

    fn rollback_document_shaping(&mut self) {
        if let Some(previous) = self.previous_document.take() {
            self.table = previous.table;
            self.reverse = previous.reverse;
            self.layout_time = previous.layout_time;
        }
    }

    fn shape_glyph<'a>(&mut self, glyph_metrics: &mut GlyphRegistry<'a>, ch: char, font_size: f32, font_weight: u16, font_slant: FontSlant, color: u32, family: Option<&str>) -> Result<GlyphId, ShapeError> {
        self.shape_with_font_properties(glyph_metrics, ch, font_size, font_weight, font_slant, color, family)
    }
}

#[cfg(test)]
mod tests {
    use super::GlyphCache;
    use floem::text::{FONT_SYSTEM, TextLayout};
    use html::layout::LayoutConstraints;
    use html::pipeline::DocumentFactory;
    use html_view_core::text_backend::{GlyphId, GlyphMetric, GlyphShaper};
    use std::mem::size_of;

    fn shape(cache: &mut GlyphCache, c: char, size: f32, extra_style: &str) -> (GlyphId, GlyphMetric) {
        let html = format!(r#"<span style="font-size: {size}px; {extra_style}">{c}</span>"#);
        let mut factory = DocumentFactory::new();
        let prepared = factory.parse_with_new_pipeline(&html, None);
        let shaped = prepared.shape(cache).expect("test shape should succeed");
        let document = shaped.layout(LayoutConstraints::new(600.0, size as f64).expect("valid test constraints"));
        let view = document.render_view();
        let glyph = view.text().glyph_at(0).expect("test document should contain one glyph");
        let metric = view.text().glyph_metric(glyph).expect("shaped glyph should have a metric");
        (glyph, metric)
    }

    #[test]
    fn split_metric_storage_is_no_larger_per_glyph_than_legacy_tuple_storage() {
        let legacy = size_of::<(char, TextLayout, GlyphMetric)>();
        let split = size_of::<TextLayout>() + size_of::<GlyphMetric>();
        assert!(split <= legacy, "split storage uses {split} bytes per glyph versus {legacy} bytes for the legacy tuple");
    }

    #[test]
    fn document_transaction_rollback_restores_the_previous_layouts() {
        let mut cache = GlyphCache::new();
        let (glyph, _) = shape(&mut cache, 'A', 16.0, "");
        assert_eq!(cache.len(), 1);

        cache.begin_document_shaping();
        assert_eq!(cache.len(), 0);
        cache.rollback_document_shaping();

        assert_eq!(cache.len(), 1);
        let _ = cache.get(glyph);
    }

    #[test]
    fn glyph_metrics_report_real_ascent_and_descent() {
        let mut cache = GlyphCache::new();
        let (_, metric) = shape(&mut cache, 'g', 16.0, "");
        let (ascent, descent) = (metric.ascent(), metric.descent());
        assert!(ascent > 0.0, "ascent should be positive, got {ascent}");
        // Regression: descent used to be hardcoded to 0.0.
        assert!(descent > 0.0, "descent should reflect real font metrics, got {descent}");
    }

    #[test]
    fn larger_font_size_increases_ascent() {
        let mut cache = GlyphCache::new();
        let small = shape(&mut cache, 'x', 12.0, "").1.ascent();
        let large = shape(&mut cache, 'x', 48.0, "").1.ascent();
        assert!(large > small, "ascent should scale with font size: 48px {large} vs 12px {small}");
    }

    #[test]
    fn baseline_offset_is_positive_and_scales_with_size() {
        let mut cache = GlyphCache::new();
        let small = shape(&mut cache, 'A', 12.0, "").1.baseline_offset();
        let large = shape(&mut cache, 'A', 48.0, "").1.baseline_offset();
        // The baseline sits below the box top, and its position is font-derived
        // so it grows with the font size.
        assert!(small > 0.0, "baseline offset should be positive, got {small}");
        assert!(large > small, "baseline offset should grow with font size: 48px {large} vs 12px {small}");
    }

    #[test]
    fn explicit_family_at_string_index_zero_is_selected() {
        for family in ["DejaVu Serif", "Liberation Serif"] {
            {
                let font_system = FONT_SYSTEM.lock();
                if !font_system.db().faces().any(|face| face.families.iter().any(|(name, _)| name == family)) {
                    continue;
                }
            }

            let mut cache = GlyphCache::new();
            let style = format!("font-family: '{family}'");
            let (glyph, _) = shape(&mut cache, 'A', 16.0, &style);
            let font_id = cache.get(glyph).layout_runs().next().unwrap().glyphs[0].font_id;
            let font_system = FONT_SYSTEM.lock();
            let face = font_system.db().face(font_id).unwrap();
            assert!(face.families.iter().any(|(name, _)| name == family), "requested {family}, selected {:?}", face.families);
        }
    }
}
