use crate::document::FontStyle;
use floem::peniko::Color;
use floem::text::{Attrs, AttrsList, FamilyOwned, Style as TextFontStyle, TextLayout};
use lightningcss::properties::text::TextAlign;
use rustc_data_structures::fx::FxHashMap;
use std::time::{Duration, Instant};

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
    table: FxHashMap<(char, u8, u16, u8, u32, u16), u16>,
    family_cache: FxHashMap<u16, Vec<FamilyOwned>>,
    reverse: Vec<(char, TextLayout)>,
    pub layout_time: Duration,
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self { table: FxHashMap::default(), family_cache: FxHashMap::default(), reverse: Vec::with_capacity(100), layout_time: Duration::ZERO }
    }
}

impl GlyphCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get_or_insert(&mut self, char: char, mut font: Attrs, parse_state: &ParseState) -> (&TextLayout, u16) {
        use std::collections::hash_map::Entry;

        let font_size = font.font_size as u8;
        font = font.raw_weight(parse_state.font_weight);
        font = font.style(parse_state.text_style);

        let style_key = match parse_state.text_style {
            TextFontStyle::Normal => 0,
            TextFontStyle::Italic => 1,
            TextFontStyle::Oblique => 2,
        };
        let key = (char, font_size, parse_state.font_weight, style_key, 0x000000FF, 0);
        let index = match self.table.entry(key) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let start = Instant::now();

                let index = self.reverse.len() as u16;
                let mut layout = TextLayout::new();
                let mut buf = [0u8; 4];
                let s = char.encode_utf8(&mut buf);

                layout.set_text(s, AttrsList::new(font));
                self.reverse.push((char, layout));

                self.layout_time += start.elapsed();
                *e.insert(index)
            }
        };
        (&self.reverse.get(index as usize).unwrap().1, index)
    }
    pub fn get(&self, index: u16) -> &TextLayout {
        &self.reverse.get(index as usize).unwrap().1
    }

    /// Shape a character using font properties
    pub fn shape_with_font_properties(&mut self, c: char, font_size: f32, font_weight: u16, font_style: FontStyle, color: u32, font_family: Option<u16>, family: Option<&str>) -> u16 {
        use std::collections::hash_map::Entry;

        // Use provided properties
        let font_size = font_size as u8;
        let family_key = font_family.unwrap_or(0);
        let text_style = if font_style == FontStyle::Italic { TextFontStyle::Italic } else { TextFontStyle::Normal };

        let style_key = match text_style {
            TextFontStyle::Normal => 0,
            TextFontStyle::Italic => 1,
            TextFontStyle::Oblique => 2,
        };
        let key = (c, font_size, font_weight, style_key, color, family_key);
        match self.table.entry(key) {
            Entry::Occupied(e) => *e.get(),
            Entry::Vacant(e) => {
                let start = Instant::now();

                let index = self.reverse.len() as u16;
                let mut layout = TextLayout::new();
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);

                let color = Color::rgba8((color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8, (color & 0xFF) as u8);
                let mut font = Attrs::new().font_size(font_size as f32).raw_weight(font_weight).style(text_style).color(color);
                if let Some(family) = family {
                    if family_key != 0 {
                        let families = self.family_cache.entry(family_key).or_insert_with(|| FamilyOwned::parse_list(family).collect());
                        if !families.is_empty() {
                            font = font.family(families);
                        }
                    }
                }

                layout.set_text(s, AttrsList::new(font));
                self.reverse.push((c, layout));

                self.layout_time += start.elapsed();
                *e.insert(index)
            }
        }
    }

    pub fn get_char(&self, index: u16) -> char {
        self.reverse.get(index as usize).unwrap().0
    }

    pub fn len(&self) -> usize {
        self.reverse.len()
    }

    pub fn glyph_metrics(&self, index: u16) -> (f64, f64) {
        let layout = self.get(index);

        (layout.size().height, 0.0)
    }

    pub fn total_memory_usage(&self) -> usize {
        let entry_size = size_of::<((char, u8, u16, u8, u32, u16), u16)>();

        let table_overhead = size_of::<FxHashMap<(char, u8, u16, u8), u16>>();
        let table_data = self.table.len() * entry_size;

        let reverse_overhead = size_of::<Vec<(char, TextLayout)>>();
        let reverse_data: usize = self.reverse.iter().map(|_| size_of::<(char, TextLayout)>()).sum();

        table_overhead + table_data + reverse_overhead + reverse_data
        //self.reverse.len()
    }
}
