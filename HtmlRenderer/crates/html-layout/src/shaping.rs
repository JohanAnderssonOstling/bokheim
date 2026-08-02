use crate::document::{ComputedStyles, FontStyle, GlyphId, GlyphMetric, GlyphMetricError, GlyphMetrics, InlineContent, InlineKind, LayoutTree, StyleStringId, StyleView, TextTransform};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeError {
    kind: ShapeErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ShapeErrorKind {
    UnregisteredGlyphId { glyph_id: GlyphId, metrics_len: usize },
    RejectedMetric { reason: GlyphMetricError },
    RegistryCapacityExhausted { metrics_len: usize },
}

impl ShapeError {
    pub fn unregistered_glyph_id(glyph_id: GlyphId, metrics_len: usize) -> Self {
        Self { kind: ShapeErrorKind::UnregisteredGlyphId { glyph_id, metrics_len } }
    }

    pub fn rejected_metric(reason: GlyphMetricError) -> Self {
        Self { kind: ShapeErrorKind::RejectedMetric { reason } }
    }

    pub fn registry_capacity_exhausted(metrics_len: usize) -> Self {
        Self { kind: ShapeErrorKind::RegistryCapacityExhausted { metrics_len } }
    }

    pub fn glyph_id(self) -> GlyphId {
        match self.kind {
            ShapeErrorKind::UnregisteredGlyphId { glyph_id, .. } => glyph_id,
            ShapeErrorKind::RejectedMetric { .. } => GlyphId::MAX,
            ShapeErrorKind::RegistryCapacityExhausted { .. } => GlyphId::MAX,
        }
    }

    pub fn metrics_len(self) -> usize {
        match self.kind {
            ShapeErrorKind::UnregisteredGlyphId { metrics_len, .. } => metrics_len,
            ShapeErrorKind::RejectedMetric { .. } => 0,
            ShapeErrorKind::RegistryCapacityExhausted { metrics_len } => metrics_len,
        }
    }

    pub fn reason(self) -> Option<GlyphMetricError> {
        match self.kind {
            ShapeErrorKind::RejectedMetric { reason } => Some(reason),
            ShapeErrorKind::UnregisteredGlyphId { .. } | ShapeErrorKind::RegistryCapacityExhausted { .. } => None,
        }
    }
}

impl fmt::Display for ShapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ShapeErrorKind::UnregisteredGlyphId { glyph_id, metrics_len } => {
                write!(formatter, "glyph shaper returned ID {} with only {} registered metrics", glyph_id, metrics_len)
            }
            ShapeErrorKind::RejectedMetric { reason } => write!(formatter, "glyph shaper returned invalid glyph metric: {reason:?}"),
            ShapeErrorKind::RegistryCapacityExhausted { metrics_len } => {
                write!(formatter, "glyph registry exceeded capacity after {metrics_len} registered metrics")
            }
        }
    }
}

impl std::error::Error for ShapeError {}

/// Append-only access to the glyph metrics owned by an active shaping phase.
///
/// The backing store is intentionally not part of the public API:
///
/// ```compile_fail
/// let _ = html_layout::GlyphMetrics::default();
/// ```
pub struct GlyphRegistry<'a> {
    glyph_metrics: &'a mut GlyphMetrics,
}

impl<'a> GlyphRegistry<'a> {
    pub(crate) fn new(glyph_metrics: &'a mut GlyphMetrics) -> Self {
        Self { glyph_metrics }
    }

    pub fn register(&mut self, metric: GlyphMetric) -> Result<GlyphId, ShapeError> {
        self.glyph_metrics.register(metric).map_err(|_| ShapeError::registry_capacity_exhausted(self.len()))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.glyph_metrics.len()
    }

    #[inline]
    pub fn get(&self, glyph: GlyphId) -> Option<GlyphMetric> {
        self.glyph_metrics.get_checked(glyph)
    }

    #[inline]
    pub fn contains(&self, glyph: GlyphId) -> bool {
        self.glyph_metrics.get_checked(glyph).is_some()
    }
}

/// Backend supplied by a renderer adapter for turning styled characters into
/// dense glyph IDs and framework-neutral metrics.
pub trait GlyphShaper {
    fn reset(&mut self);

    fn shape_glyph<'a>(
        &mut self, glyph_metrics: &mut GlyphRegistry<'a>, ch: char, font_size: f32, font_weight: u16, font_style: FontStyle, color: u32, font_family: Option<StyleStringId>, family: Option<&str>,
    ) -> Result<GlyphId, ShapeError>;
}

/// Shapes all styled text in a document. Whitespace normalization and CSS text
/// transforms are framework-neutral; only individual glyph shaping is
/// delegated to the renderer backend.
pub(crate) fn shape_document(styles: &ComputedStyles, layout_tree: &LayoutTree, inline_content: &mut InlineContent, glyph_shaper: &mut impl GlyphShaper, glyph_metrics: &mut GlyphMetrics) -> Result<(), ShapeError> {
    if inline_content.glyphs().is_empty() {
        return Ok(());
    }

    collapse_whitespace(styles, layout_tree, inline_content);
    if inline_content.glyphs().is_empty() {
        return Ok(());
    }
    let mut glyph_metrics = GlyphRegistry::new(glyph_metrics);

    let mut box_family: Vec<Option<String>> = vec![None; layout_tree.box_count()];
    for (idx, family) in box_family.iter_mut().enumerate() {
        let style = get_style(styles, layout_tree, idx);
        if let Some(family_idx) = style.font_family() {
            *family = styles.string(family_idx).map(str::to_owned);
        }
    }

    let mut prev_char: Option<char> = None;
    let mut prev_box: Option<usize> = None;
    for (box_idx, range) in text_runs(inline_content) {
        let style = get_style(styles, layout_tree, box_idx);
        let small_caps = style.font_variant_small_caps();
        let transform = style.text_transform();
        let font_size = style.font_size();
        let font_weight = style.font_weight();
        let font_style = style.font_style();
        let color = style.color();
        let font_family_idx = style.font_family();
        let family = box_family[box_idx].as_deref();

        for i in range {
            let mut character = char::from_u32(inline_content.glyph_at(i as usize).unwrap_or('?' as GlyphId) as u32).unwrap_or('?');
            if small_caps && character.is_alphabetic() {
                character = character.to_uppercase().next().unwrap_or(character);
            }
            match transform {
                TextTransform::Uppercase => character = character.to_uppercase().next().unwrap_or(character),
                TextTransform::Lowercase => character = character.to_lowercase().next().unwrap_or(character),
                TextTransform::Capitalize => {
                    let boundary = prev_box != Some(box_idx) || prev_char.map(|previous| previous.is_whitespace() || previous == '\n').unwrap_or(true);
                    if boundary && character.is_alphabetic() {
                        character = character.to_uppercase().next().unwrap_or(character);
                    }
                }
                TextTransform::None => {}
            }

            let glyph = glyph_shaper.shape_glyph(&mut glyph_metrics, character, font_size, font_weight, font_style, color, font_family_idx, family)?;
            if !glyph_metrics.contains(glyph) {
                return Err(ShapeError::unregistered_glyph_id(glyph, glyph_metrics.len()));
            }
            inline_content.glyphs_mut()[i as usize] = glyph;
            prev_char = Some(character);
            prev_box = Some(box_idx);
        }
    }
    Ok(())
}

fn collapse_whitespace(styles: &ComputedStyles, layout_tree: &LayoutTree, inline_content: &mut InlineContent) {
    let mut new_glyphs = Vec::with_capacity(inline_content.glyphs().len());
    let mut remap = vec![u32::MAX; inline_content.glyphs().len()];
    let mut prev_was_space = false;
    let mut at_line_start = true;

    for (box_idx, range) in text_runs(inline_content) {
        let white_space = get_style(styles, layout_tree, box_idx).white_space();
        let collapse = white_space.collapses_spaces();
        let preserve_newlines = white_space.preserves_newlines();

        for i in range {
            let i = i as usize;
            let char_code = inline_content.glyph_at(i).unwrap_or('?' as GlyphId);
            let character = char::from_u32(char_code as u32).unwrap_or('?');
            if character == '\n' {
                if preserve_newlines {
                    remap[i] = new_glyphs.len() as u32;
                    new_glyphs.push('\n' as GlyphId);
                    prev_was_space = false;
                    at_line_start = true;
                } else if collapse && !prev_was_space && !at_line_start {
                    remap[i] = new_glyphs.len() as u32;
                    new_glyphs.push(' ' as GlyphId);
                    prev_was_space = true;
                }
            } else if character.is_whitespace() && character != '\u{00A0}' {
                if collapse {
                    if !prev_was_space && !at_line_start {
                        remap[i] = new_glyphs.len() as u32;
                        new_glyphs.push(' ' as GlyphId);
                        prev_was_space = true;
                    }
                } else {
                    remap[i] = new_glyphs.len() as u32;
                    new_glyphs.push(char_code);
                    prev_was_space = character == ' ' || character == '\t';
                    at_line_start = false;
                }
            } else {
                remap[i] = new_glyphs.len() as u32;
                new_glyphs.push(char_code);
                prev_was_space = false;
                at_line_start = false;
            }
        }
    }

    for run in inline_content.inline_runs_mut() {
        let (InlineKind::Text { glyphs } | InlineKind::Marker { glyphs }) = &mut run.kind else {
            continue;
        };
        let old_start = glyphs.start as usize;
        let old_end = glyphs.end as usize;
        let new_start = (old_start..old_end).find_map(|i| remap.get(i).copied().filter(|mapped| *mapped != u32::MAX));
        let new_end = (old_start..old_end).filter_map(|i| remap.get(i).copied().filter(|mapped| *mapped != u32::MAX)).next_back().map(|mapped| mapped + 1);
        *glyphs = match (new_start, new_end) {
            (Some(start), Some(end)) => start..end,
            _ => 0..0,
        };
    }
    inline_content.replace_glyphs(new_glyphs);
}

fn get_style<'a>(styles: &'a ComputedStyles, layout_tree: &LayoutTree, box_idx: usize) -> StyleView<'a> {
    let indices = layout_tree.get_box_style_indices(box_idx).unwrap_or_else(|| styles.default_indices());
    styles.view(indices).expect("validated style handle")
}

fn text_runs(inline_content: &InlineContent) -> Vec<(usize, std::ops::Range<u32>)> {
    inline_content
        .inline_runs()
        .iter()
        .filter_map(|run| match &run.kind {
            InlineKind::Text { glyphs } | InlineKind::Marker { glyphs } if glyphs.start < glyphs.end => Some((run.box_idx as usize, glyphs.clone())),
            _ => None,
        })
        .collect()
}
