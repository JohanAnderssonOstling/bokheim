use std::sync::Arc;
use std::time::Instant;

use html_dom::RootFontSize;
use html_layout::{GlyphShaper as LayoutGlyphShaper, LaidOutDocument, LayoutConstraints as LayoutConstraintsOutput, PreparedDocument, ShapedDocument};
use html_parse::{ParsedHtml, parse_html_document};
use html_resources::ResourceProvider;

use crate::parser::DocumentFactory;
use crate::types::LayoutConstraints;
use crate::types::{LayoutCacheKey, ParsedCacheKey, PreparedCacheKey, RenderOnlyCacheKey, ShapedCacheKey};
use crate::{EarliestStage, PipelineError, PipelineInputs, PipelineRetainedBytes, PipelineStageCounts, PipelineTimings, PipelineUpdate, Reuse, ReuseReport};

#[derive(Default)]
pub struct PipelineCacheState {
    pub inputs: Option<PipelineInputs>,
    pub parsed_key: Option<ParsedCacheKey>,
    pub parsed: Option<Arc<ParsedHtml>>,
    pub prepared_key: Option<PreparedCacheKey>,
    pub prepared: Option<PreparedDocument>,
    pub shaped_key: Option<ShapedCacheKey>,
    pub shaped: Option<ShapedDocument>,
    pub laid_out_key: Option<LayoutCacheKey>,
    pub laid_out: Option<LaidOutDocument>,
}

impl PipelineCacheState {
    fn parsed_matches(&self, key: &ParsedCacheKey) -> bool {
        self.parsed_key.as_ref() == Some(key) && self.parsed.is_some()
    }

    fn prepared_matches(&self, key: &PreparedCacheKey) -> bool {
        self.prepared_key.as_ref() == Some(key) && self.prepared.is_some()
    }

    fn shaped_matches(&self, key: &ShapedCacheKey) -> bool {
        self.shaped_key.as_ref() == Some(key) && self.shaped.is_some()
    }

    fn laid_out_matches(&self, key: &LayoutCacheKey) -> bool {
        self.laid_out_key.as_ref() == Some(key) && self.laid_out.is_some()
    }
}

pub struct PipelineSession {
    provider: std::sync::Arc<dyn ResourceProvider>,
    factory: DocumentFactory,
    cache: PipelineCacheState,
}

impl PipelineSession {
    pub fn new(provider: Arc<dyn ResourceProvider>) -> Self {
        Self { provider, factory: DocumentFactory::new(), cache: PipelineCacheState::default() }
    }

    pub fn update(&mut self, requested_inputs: PipelineInputs, glyph_shaper: &mut impl LayoutGlyphShaper) -> Result<PipelineUpdate, PipelineError> {
        let decision_started = Instant::now();
        let base_uri = if requested_inputs.base_uri.is_empty() { "index.html".to_string() } else { requested_inputs.base_uri.clone() };
        self.factory.set_resource_context(self.provider.clone(), base_uri);
        self.factory.set_root_font_size(to_root_font_size(requested_inputs.style_environment.root_font_size));

        let mut timings = PipelineTimings::default();
        if let Some(previous) = self.cache.inputs.as_ref() {
            timings.change_cause = requested_inputs.change_mask(previous);
        }

        let requested_stage = self.normalized_earliest_stage(&requested_inputs);
        let layout_constraints = to_layout_constraints(requested_inputs.layout);
        let parsed_key = requested_inputs.parsed_cache_key();
        let prepared_key = requested_inputs.prepared_cache_key();
        let shaped_key = requested_inputs.shaped_cache_key();
        let layout_key = requested_inputs.layout_cache_key();
        let render_only_key = requested_inputs.render_only_cache_key();

        let mut report = ReuseReport::default();
        let mut stage_runs = PipelineStageCounts::default();
        let mut stage_reuses = PipelineStageCounts::default();

        match requested_stage {
            EarliestStage::Parse => {
                let parsed = self.parse(&requested_inputs.source);
                let prepared = self.factory.build_pipeline_from_parsed(&parsed, &[]);
                let shaped = self.shape(&prepared, glyph_shaper)?;
                let laid_out = shaped.clone().layout_with_metrics(layout_constraints, &requested_inputs.image_metrics);

                self.cache.parsed = Some(parsed.clone());
                self.cache.parsed_key = Some(parsed_key.clone());
                self.cache.prepared = Some(prepared);
                self.cache.prepared_key = Some(prepared_key.clone());
                self.cache.shaped = Some(shaped);
                self.cache.shaped_key = Some(shaped_key.clone());
                self.cache.laid_out = Some(laid_out);
                self.cache.laid_out_key = Some(layout_key.clone());

                stage_runs.parsed += 1;
                stage_runs.styled += 1;
                stage_runs.prepared += 1;
                stage_runs.shaped += 1;
                stage_runs.laid_out += 1;

                report.parsed = Reuse::recomputed(EarliestStage::Parse);
                report.styled = Reuse::recomputed(EarliestStage::Parse);
                report.prepared = Reuse::recomputed(EarliestStage::Parse);
                report.shaped = Reuse::recomputed(EarliestStage::Parse);
                report.laid_out = Reuse::recomputed(EarliestStage::Parse);
            }
            EarliestStage::Style | EarliestStage::Prepare => {
                let parsed = self.parse_cached(&parsed_key, &requested_inputs.source);
                let prepared_reused = self.cache.prepared_matches(&prepared_key);
                let prepared = self.prepare_from_cache_or_rebuild(&parsed, &prepared_key);

                let shaped = self.shape(&prepared, glyph_shaper)?;
                let laid_out = shaped.clone().layout_with_metrics(layout_constraints, &requested_inputs.image_metrics);

                self.cache.parsed = Some(parsed);
                self.cache.parsed_key = Some(parsed_key.clone());
                self.cache.prepared = Some(prepared);
                self.cache.prepared_key = Some(prepared_key.clone());
                self.cache.shaped = Some(shaped);
                self.cache.shaped_key = Some(shaped_key.clone());
                self.cache.laid_out = Some(laid_out);
                self.cache.laid_out_key = Some(layout_key.clone());

                stage_reuses.parsed += 1;
                stage_runs.styled += 1;
                if prepared_reused {
                    stage_reuses.prepared += 1;
                } else {
                    stage_runs.prepared += 1;
                }
                stage_runs.shaped += 1;
                stage_runs.laid_out += 1;

                report.parsed = Reuse::reused(EarliestStage::Style);
                report.styled = Reuse::recomputed(EarliestStage::Style);
                report.prepared = if prepared_reused { Reuse::reused(EarliestStage::Style) } else { Reuse::recomputed(EarliestStage::Style) };
                report.shaped = Reuse::recomputed(EarliestStage::Style);
                report.laid_out = Reuse::recomputed(EarliestStage::Style);
            }
            EarliestStage::Shape => {
                let parsed = self.parse_cached(&parsed_key, &requested_inputs.source);
                let prepared_reused = self.cache.prepared_matches(&prepared_key);
                let prepared = self.prepare_from_cache_or_rebuild(&parsed, &prepared_key);
                let shaped = self.shape(&prepared, glyph_shaper)?;
                let laid_out = shaped.clone().layout_with_metrics(layout_constraints, &requested_inputs.image_metrics);

                self.cache.parsed = Some(parsed);
                self.cache.parsed_key = Some(parsed_key.clone());
                self.cache.prepared = Some(prepared);
                self.cache.prepared_key = Some(prepared_key.clone());
                self.cache.shaped = Some(shaped);
                self.cache.shaped_key = Some(shaped_key.clone());
                self.cache.laid_out = Some(laid_out);
                self.cache.laid_out_key = Some(layout_key.clone());

                stage_reuses.parsed += 1;
                stage_reuses.styled += 1;
                if prepared_reused {
                    stage_reuses.prepared += 1;
                } else {
                    stage_runs.prepared += 1;
                }
                stage_runs.shaped += 1;
                stage_runs.laid_out += 1;

                report.parsed = Reuse::reused(EarliestStage::Shape);
                report.styled = Reuse::reused(EarliestStage::Shape);
                report.prepared = if prepared_reused { Reuse::reused(EarliestStage::Shape) } else { Reuse::recomputed(EarliestStage::Shape) };
                report.shaped = Reuse::recomputed(EarliestStage::Shape);
                report.laid_out = Reuse::recomputed(EarliestStage::Shape);
            }
            EarliestStage::Layout => {
                let parsed = self.parse_cached(&parsed_key, &requested_inputs.source);
                let prepared_reused = self.cache.prepared_matches(&prepared_key);
                let prepared = self.prepare_from_cache_or_rebuild(&parsed, &prepared_key);
                let shaped_reused = self.cache.shaped_matches(&shaped_key);
                let shaped = self.shape_from_cache_or_rebuild(&prepared, &shaped_key, glyph_shaper)?;
                self.relayout_laid_out(layout_constraints, &requested_inputs.image_metrics, &shaped_key, &layout_key, &shaped);

                self.cache.parsed = Some(parsed);
                self.cache.parsed_key = Some(parsed_key.clone());
                self.cache.prepared = Some(prepared);
                self.cache.prepared_key = Some(prepared_key.clone());
                self.cache.shaped = Some(shaped);
                self.cache.shaped_key = Some(shaped_key.clone());

                stage_reuses.parsed += 1;
                stage_reuses.styled += 1;
                if prepared_reused {
                    stage_reuses.prepared += 1;
                } else {
                    stage_runs.prepared += 1;
                }
                if shaped_reused {
                    stage_reuses.shaped += 1;
                } else {
                    stage_runs.shaped += 1;
                }
                stage_runs.laid_out += 1;

                report.parsed = Reuse::reused(EarliestStage::Layout);
                report.styled = Reuse::reused(EarliestStage::Layout);
                report.prepared = if prepared_reused { Reuse::reused(EarliestStage::Layout) } else { Reuse::recomputed(EarliestStage::Layout) };
                report.shaped = if shaped_reused { Reuse::reused(EarliestStage::Layout) } else { Reuse::recomputed(EarliestStage::Layout) };
                report.laid_out = Reuse::recomputed(EarliestStage::Layout);
            }
            EarliestStage::Paint => {
                debug_assert_eq!(self.cache.laid_out_key.as_ref(), Some(&render_only_key.layout));
                stage_reuses.parsed += 1;
                stage_reuses.styled += 1;
                stage_reuses.prepared += 1;
                stage_reuses.shaped += 1;
                stage_reuses.laid_out += 1;

                report.parsed = Reuse::reused(EarliestStage::Paint);
                report.styled = Reuse::reused(EarliestStage::Paint);
                report.prepared = Reuse::reused(EarliestStage::Paint);
                report.shaped = Reuse::reused(EarliestStage::Paint);
                report.laid_out = Reuse::reused(EarliestStage::Paint);
            }
            EarliestStage::None => {
                stage_reuses.parsed += 1;
                stage_reuses.styled += 1;
                stage_reuses.prepared += 1;
                stage_reuses.shaped += 1;
                stage_reuses.laid_out += 1;

                report.parsed = Reuse::reused(EarliestStage::None);
                report.styled = Reuse::reused(EarliestStage::None);
                report.prepared = Reuse::reused(EarliestStage::None);
                report.shaped = Reuse::reused(EarliestStage::None);
                report.laid_out = Reuse::reused(EarliestStage::None);
            }
        }

        self.validate_cache_against_requested(&parsed_key, &prepared_key, &shaped_key, &layout_key, &render_only_key);
        self.cache.inputs = Some(requested_inputs);

        timings.decision_time = decision_started.elapsed();
        timings.stage_runs = stage_runs;
        timings.stage_reuses = stage_reuses;
        timings.retained_bytes = self.retained_bytes();
        Ok(PipelineUpdate { stage: requested_stage, timings, report, anchors_preserved: !matches!(requested_stage, EarliestStage::Parse | EarliestStage::Style | EarliestStage::Prepare | EarliestStage::Shape) })
    }

    fn normalized_earliest_stage(&self, requested_inputs: &PipelineInputs) -> EarliestStage {
        let base_stage = self.cache.inputs.as_ref().map(|previous| requested_inputs.earliest_stage(previous)).unwrap_or(EarliestStage::Parse);
        let parsed_key = requested_inputs.parsed_cache_key();
        let prepared_key = requested_inputs.prepared_cache_key();
        let shaped_key = requested_inputs.shaped_cache_key();
        let layout_key = requested_inputs.layout_cache_key();

        match base_stage {
            EarliestStage::Parse => EarliestStage::Parse,
            EarliestStage::Style | EarliestStage::Prepare => {
                if !self.cache.parsed_matches(&parsed_key) {
                    EarliestStage::Parse
                } else if !self.cache.prepared_matches(&prepared_key) {
                    EarliestStage::Prepare
                } else {
                    base_stage
                }
            }
            EarliestStage::Shape => {
                if !self.cache.parsed_matches(&parsed_key) {
                    EarliestStage::Parse
                } else if !self.cache.prepared_matches(&prepared_key) {
                    EarliestStage::Prepare
                } else if self.cache.shaped_matches(&shaped_key) {
                    EarliestStage::Shape
                } else {
                    EarliestStage::Shape
                }
            }
            EarliestStage::Layout => {
                if !self.cache.parsed_matches(&parsed_key) {
                    EarliestStage::Parse
                } else if !self.cache.prepared_matches(&prepared_key) {
                    EarliestStage::Prepare
                } else if !self.cache.shaped_matches(&shaped_key) {
                    EarliestStage::Shape
                } else if self.cache.laid_out_matches(&layout_key) {
                    EarliestStage::Layout
                } else {
                    EarliestStage::Layout
                }
            }
            EarliestStage::Paint => {
                if self.cache.laid_out_matches(&layout_key) {
                    EarliestStage::Paint
                } else {
                    EarliestStage::Layout
                }
            }
            EarliestStage::None => {
                if self.cache.laid_out_matches(&layout_key) {
                    EarliestStage::Paint
                } else {
                    EarliestStage::Layout
                }
            }
        }
    }

    fn parse_cached(&mut self, key: &ParsedCacheKey, source: &str) -> Arc<ParsedHtml> {
        if self.cache.parsed_matches(key) {
            return self.cache.parsed.clone().expect("parsed html should exist when parsed key matches");
        }
        let parsed = Arc::new(parse_html_document(source));
        self.cache.parsed = Some(parsed.clone());
        self.cache.parsed_key = Some(key.clone());
        parsed
    }

    fn retained_bytes(&self) -> PipelineRetainedBytes {
        PipelineRetainedBytes {
            parsed: self.cache.inputs.as_ref().map_or(0, |inputs| inputs.source.len()),
            styled: self.cache.prepared.as_ref().map_or(0, |prepared| prepared.memory_usage_bytes()),
            shaped: self.cache.shaped.as_ref().map_or(0, |shaped| shaped.memory_usage_bytes()),
            laid_out: self.cache.laid_out.as_ref().map_or(0, |laid_out| laid_out.memory_usage_bytes()),
        }
    }

    fn parse(&mut self, source: &str) -> Arc<ParsedHtml> {
        Arc::new(parse_html_document(source))
    }

    fn prepare_from_cache_or_rebuild(&mut self, parsed: &Arc<ParsedHtml>, requested_prepared_key: &PreparedCacheKey) -> PreparedDocument {
        if self.cache.prepared_matches(requested_prepared_key) {
            debug_assert_eq!(self.cache.prepared_key.as_ref(), Some(requested_prepared_key));
            return self.cache.prepared.clone().expect("prepared document should exist when prepared key matches");
        }
        self.factory.build_pipeline_from_parsed(parsed.as_ref(), &[])
    }

    fn shape_from_cache_or_rebuild(&mut self, prepared: &PreparedDocument, requested_shaped_key: &ShapedCacheKey, glyph_shaper: &mut impl LayoutGlyphShaper) -> Result<ShapedDocument, PipelineError> {
        if self.cache.shaped_matches(requested_shaped_key) {
            debug_assert_eq!(self.cache.shaped_key.as_ref(), Some(requested_shaped_key));
            return self.cache.shaped.clone().ok_or_else(|| PipelineError("cached shaped document missing".to_owned()));
        }
        self.shape(prepared, glyph_shaper)
    }

    fn relayout_laid_out(&mut self, constraints: LayoutConstraintsOutput, image_metrics: &html_layout::ImageMetrics, requested_shaped_key: &ShapedCacheKey, requested_layout_key: &LayoutCacheKey, shaped: &ShapedDocument) {
        let can_relayout = self.cache.laid_out.is_some() && self.cache.shaped_matches(requested_shaped_key);
        match self.cache.laid_out.as_mut() {
            Some(laid_out) if can_relayout => {
                debug_assert_eq!(self.cache.shaped_key.as_ref(), Some(requested_shaped_key));
                lay_out_recompute(laid_out, constraints, image_metrics);
                self.cache.laid_out_key = Some(requested_layout_key.clone());
            }
            _ => {
                let laid_out = shaped.clone().layout_with_metrics(constraints, image_metrics);
                self.cache.laid_out = Some(laid_out);
                self.cache.laid_out_key = Some(requested_layout_key.clone());
            }
        }
    }

    fn shape(&mut self, prepared: &PreparedDocument, glyph_shaper: &mut impl LayoutGlyphShaper) -> Result<ShapedDocument, PipelineError> {
        prepared.shape(glyph_shaper).map_err(|error| PipelineError(error.to_string()))
    }

    fn validate_cache_against_requested(&self, parsed_key: &ParsedCacheKey, prepared_key: &PreparedCacheKey, shaped_key: &ShapedCacheKey, layout_key: &LayoutCacheKey, render_only_key: &RenderOnlyCacheKey) {
        if self.cache.inputs.is_none() {
            return;
        }
        debug_assert_eq!(self.cache.inputs.is_some(), true);

        if self.cache.parsed_matches(parsed_key) {
            debug_assert!(self.cache.parsed.is_some());
        }
        if self.cache.prepared_matches(prepared_key) {
            debug_assert!(self.cache.prepared.is_some());
        }
        if self.cache.shaped_matches(shaped_key) {
            debug_assert!(self.cache.shaped.is_some());
        }
        if self.cache.laid_out_matches(layout_key) {
            debug_assert!(self.cache.laid_out.is_some());
            debug_assert_eq!(self.cache.laid_out_key.as_ref(), Some(layout_key));
        }
        if !self.cache.laid_out.is_none() {
            debug_assert_eq!(self.cache.laid_out_key.as_ref(), Some(&render_only_key.layout));
        }
    }

    pub fn document(&self) -> Option<&LaidOutDocument> {
        self.cache.laid_out.as_ref()
    }

    pub fn document_mut(&mut self) -> Option<&mut LaidOutDocument> {
        self.cache.laid_out.as_mut()
    }
}

fn lay_out_recompute(laid_out: &mut LaidOutDocument, constraints: LayoutConstraintsOutput, image_metrics: &html_layout::ImageMetrics) {
    laid_out.relayout_with_metrics(constraints, image_metrics);
}

fn to_root_font_size(root_font_size: u32) -> RootFontSize {
    RootFontSize::new(root_font_size as f32).unwrap_or_default()
}

fn to_layout_constraints(requested: LayoutConstraints) -> LayoutConstraintsOutput {
    LayoutConstraintsOutput::new(requested.viewport_width, requested.line_height).expect("requested constraints must be valid")
}

#[cfg(test)]
mod tests {
    use super::PipelineSession;
    use crate::{
        EarliestStage, FontEnvironmentRevision, FontEnvironmentRevision as FontEnv, ImageMetricsRevision, LayoutConstraints, PaintSettingsRevision, PipelineChange, PipelineError, PipelineInputs, ResourceRevision, Reuse, SourceRevision,
        StyleEnvironment, StylesheetRevision,
    };
    use html_layout::{GlyphId, GlyphMetric, GlyphShaper as LayoutGlyphShaper, LaidOutDocument};
    use html_resources::ResourceProvider;
    use std::{
        collections::HashMap,
        io,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct MockProvider;

    impl ResourceProvider for MockProvider {
        fn read_bytes(&self, _uri: &str) -> io::Result<Vec<u8>> {
            Err(io::Error::new(io::ErrorKind::NotFound, "no resources in mock provider"))
        }

        fn exists(&self, _uri: &str) -> bool {
            false
        }

        fn resolve(&self, _base: &str, href: &str) -> String {
            href.to_owned()
        }

        fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Clone, Default)]
    struct MutableProvider {
        resources: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl MutableProvider {
        fn put_bytes(&self, path: &str, bytes: Vec<u8>) {
            self.resources.lock().expect("resource lock").insert(path.to_owned(), bytes);
        }
    }

    impl ResourceProvider for MutableProvider {
        fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
            let resources = self.resources.lock().expect("resource lock");
            resources.get(uri).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, uri.to_owned()))
        }

        fn exists(&self, uri: &str) -> bool {
            self.resources.lock().expect("resource lock").contains_key(uri)
        }

        fn resolve(&self, _base: &str, href: &str) -> String {
            href.to_owned()
        }

        fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Clone, Default)]
    struct TracingShaper {
        calls: usize,
        cache: HashMap<(char, u32), GlyphId>,
    }

    impl LayoutGlyphShaper for TracingShaper {
        fn reset(&mut self) {
            self.cache.clear();
        }

        fn shape_glyph<'a>(
            &mut self, glyph_metrics: &mut html_layout::GlyphRegistry<'a>, ch: char, font_size: f32, _font_weight: u16, _font_style: html_layout::FontStyle, _color: u32, _font_family: Option<html_layout::StyleStringId>,
            _family: Option<&str>,
        ) -> Result<GlyphId, html_layout::ShapeError> {
            let _ = glyph_metrics;
            self.calls += 1;
            let key = (ch, font_size.to_bits());
            if let Some(&glyph) = self.cache.get(&key) {
                return Ok(glyph);
            }

            let idx = u32::try_from(self.cache.len()).expect("glyph ids must fit");
            let glyph = idx;
            let metric = GlyphMetric::try_new(ch, 1.0, 1.0, 0.0, 0.0).expect("deterministic metrics must be valid");
            glyph_metrics.register(metric).expect("glyph metrics registry capacity should not be exceeded");
            self.cache.insert(key, glyph);
            Ok(glyph)
        }
    }

    #[derive(Clone, Default)]
    struct SizedShaper {
        cache: HashMap<(char, u32), GlyphId>,
    }

    impl LayoutGlyphShaper for SizedShaper {
        fn reset(&mut self) {
            self.cache.clear();
        }

        fn shape_glyph<'a>(
            &mut self, glyph_metrics: &mut html_layout::GlyphRegistry<'a>, ch: char, font_size: f32, _font_weight: u16, _font_style: html_layout::FontStyle, _color: u32, _font_family: Option<html_layout::StyleStringId>,
            _family: Option<&str>,
        ) -> Result<GlyphId, html_layout::ShapeError> {
            let key = (ch, font_size.to_bits());
            if let Some(&glyph) = self.cache.get(&key) {
                return Ok(glyph);
            }

            let idx = u32::try_from(self.cache.len()).expect("glyph ids must fit");
            let glyph = idx;
            let width = font_size.max(1.0) * 0.75;
            let metric = GlyphMetric::try_new(ch, width, font_size, font_size * 0.2, 0.0).expect("deterministic metrics must be valid");
            glyph_metrics.register(metric).expect("glyph metrics registry capacity should not be exceeded");
            self.cache.insert(key, glyph);
            Ok(glyph)
        }
    }

    fn base_inputs(html: &str, source_revision: SourceRevision) -> PipelineInputs {
        PipelineInputs {
            source: html.to_owned(),
            source_revision,
            base_uri: "index.html".to_owned(),
            resource_revision: ResourceRevision::INITIAL,
            stylesheet_revision: StylesheetRevision::INITIAL,
            style_environment: StyleEnvironment::default(),
            font_environment: FontEnvironmentRevision::INITIAL,
            image_metrics_revision: ImageMetricsRevision::INITIAL,
            layout: LayoutConstraints { viewport_width: 600.0, line_height: StyleEnvironment::default().root_font_size as f64 },
            image_metrics: Default::default(),
            paint: PaintSettingsRevision::INITIAL,
        }
    }

    fn build_snapshot(document: &LaidOutDocument) -> RenderSnapshot {
        let view = document.render_view();
        let mut glyphs = Vec::with_capacity(view.glyph_count());
        for idx in 0..view.glyph_count() {
            glyphs.push(view.glyph_at(idx).unwrap_or(0));
        }
        let mut lines = Vec::with_capacity(view.line_count());
        for line_idx in 0..view.line_count() {
            let line = view.line(line_idx).expect("line should exist");
            lines.push(LineSnapshot {
                start: line.start(),
                end: line.end(),
                point_x_bits: line.point().x.to_bits(),
                point_y_bits: line.point().y.to_bits(),
                height_bits: line.height().to_bits(),
                baseline_bits: line.baseline().to_bits(),
                word_spacing_bits: line.word_spacing().to_bits(),
                letter_spacing_bits: line.letter_spacing().to_bits(),
            });
        }
        RenderSnapshot {
            glyphs,
            glyph_advances: (0..view.line_count())
                .map(|line_idx| {
                    let Some(runs) = view.line_glyph_advances(line_idx) else { return Vec::new() };
                    runs.iter()
                        .map(|run| {
                            let range = run.range();
                            (range.start, range.end, run.advance().to_bits())
                        })
                        .collect()
                })
                .collect(),
            line_links: (0..view.glyph_count() as u32).map(|glyph_idx| view.link_href_for_glyph(glyph_idx)).collect(),
            lines,
            decorations: view
                .decorations()
                .iter()
                .map(|decoration| {
                    let rect = decoration.rect();
                    DecorationSnapshot { x0_bits: rect.x0.to_bits(), y0_bits: rect.y0.to_bits(), x1_bits: rect.x1.to_bits(), y1_bits: rect.y1.to_bits(), color: decoration.color(), is_inline: decoration.is_inline() }
                })
                .collect(),
            images: view
                .all_image_fragments()
                .iter()
                .map(|image| ImageSnapshot {
                    line_idx: image.line_idx(),
                    image_idx: image.image_idx(),
                    offset_x_bits: image.offset().x.to_bits(),
                    offset_y_bits: image.offset().y.to_bits(),
                    width_bits: image.size().width.to_bits(),
                    height_bits: image.size().height.to_bits(),
                })
                .collect(),
            anchor_glyphs: {
                let mut anchors: Vec<(u16, u32)> = view.anchor_glyphs().collect();
                anchors.sort_by_key(|(id_idx, _)| *id_idx);
                anchors
            },
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct LineSnapshot {
        start: u32,
        end: u32,
        point_x_bits: u64,
        point_y_bits: u64,
        height_bits: u64,
        baseline_bits: u64,
        word_spacing_bits: u64,
        letter_spacing_bits: u64,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct DecorationSnapshot {
        x0_bits: u64,
        y0_bits: u64,
        x1_bits: u64,
        y1_bits: u64,
        color: u32,
        is_inline: bool,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ImageSnapshot {
        line_idx: usize,
        image_idx: u32,
        offset_x_bits: u64,
        offset_y_bits: u64,
        width_bits: u64,
        height_bits: u64,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct RenderSnapshot {
        glyphs: Vec<GlyphId>,
        glyph_advances: Vec<Vec<(u32, u32, u64)>>,
        line_links: Vec<Option<u16>>,
        lines: Vec<LineSnapshot>,
        decorations: Vec<DecorationSnapshot>,
        images: Vec<ImageSnapshot>,
        anchor_glyphs: Vec<(u16, u32)>,
    }

    fn update_and_snapshot(updates: &[PipelineInputs]) -> Result<RenderSnapshot, PipelineError> {
        let mut shaper = TracingShaper::default();
        let mut session = PipelineSession::new(Arc::new(MockProvider));
        for input in updates {
            session.update(input.clone(), &mut shaper)?;
        }
        let document = session.document().expect("document should exist");
        Ok(build_snapshot(document))
    }

    #[test]
    fn width_change_triggers_layout_only() {
        let mut shaper = TracingShaper::default();
        let mut session = PipelineSession::new(Arc::new(MockProvider));

        let mut baseline = base_inputs("<html><body><p>Hello world</p></body></html>", SourceRevision::INITIAL);
        let update = session.update(baseline.clone(), &mut shaper).unwrap();
        assert_eq!(update.stage, EarliestStage::Parse);
        assert_eq!(update.report.parsed, Reuse::recomputed(EarliestStage::Parse));

        let shape_calls = shaper.calls;
        baseline.layout.viewport_width = 420.0;
        let layout_only = session.update(baseline.clone(), &mut shaper).unwrap();
        assert_eq!(layout_only.stage, EarliestStage::Layout);
        assert_eq!(layout_only.report.parsed, Reuse::reused(EarliestStage::Layout));
        assert_eq!(layout_only.report.styled, Reuse::reused(EarliestStage::Layout));
        assert_eq!(layout_only.report.prepared, Reuse::reused(EarliestStage::Layout));
        assert_eq!(layout_only.report.shaped, Reuse::reused(EarliestStage::Layout));
        assert_eq!(layout_only.report.laid_out, Reuse::recomputed(EarliestStage::Layout));
        assert_eq!(shaper.calls, shape_calls);
    }

    #[test]
    fn font_environment_change_reuses_parse_style_and_prepare() {
        let mut shaper = TracingShaper::default();
        let mut session = PipelineSession::new(Arc::new(MockProvider));

        let mut update_inputs = base_inputs("<html><body><p>Glyphs: abcdef</p></body></html>", SourceRevision::INITIAL);
        let _ = session.update(update_inputs.clone(), &mut shaper).unwrap();
        let before_calls = shaper.calls;

        update_inputs.font_environment = FontEnv::INITIAL.next();
        let font_update = session.update(update_inputs, &mut shaper).unwrap();
        assert_eq!(font_update.stage, EarliestStage::Shape);
        assert_eq!(font_update.report.parsed, Reuse::reused(EarliestStage::Shape));
        assert_eq!(font_update.report.styled, Reuse::reused(EarliestStage::Shape));
        assert_eq!(font_update.report.prepared, Reuse::reused(EarliestStage::Shape));
        assert_eq!(font_update.report.shaped, Reuse::recomputed(EarliestStage::Shape));
        assert_eq!(font_update.report.laid_out, Reuse::recomputed(EarliestStage::Shape));
        assert!(shaper.calls > before_calls);
    }

    #[test]
    fn font_backend_generation_change_is_shape_stage_and_matches_clean_rebuild() {
        let source = "<html><body><p>Glyph backend swap changes shaping.</p></body></html>";
        let baseline_inputs = {
            let mut inputs = base_inputs(source, SourceRevision::INITIAL);
            inputs.font_environment = FontEnvironmentRevision::INITIAL;
            inputs
        };
        let changed_inputs = {
            let mut inputs = baseline_inputs.clone();
            inputs.font_environment = FontEnvironmentRevision::next(baseline_inputs.font_environment);
            inputs
        };

        let mut shaper = SizedShaper::default();
        let mut session = PipelineSession::new(Arc::new(MockProvider));
        let _ = session.update(baseline_inputs.clone(), &mut shaper).unwrap();

        let update = session.update(changed_inputs.clone(), &mut shaper).unwrap();
        assert_eq!(update.stage, EarliestStage::Shape);
        assert_eq!(update.report.styled, Reuse::reused(EarliestStage::Shape));
        assert_eq!(update.report.prepared, Reuse::reused(EarliestStage::Shape));
        assert_eq!(update.report.shaped, Reuse::recomputed(EarliestStage::Shape));
        assert_eq!(update.report.laid_out, Reuse::recomputed(EarliestStage::Shape));
        let incremental = build_snapshot(session.document().expect("document should exist after backend-generation update"));
        let mut fresh_shaper = SizedShaper::default();
        let mut fresh_session = PipelineSession::new(Arc::new(MockProvider));
        let _ = fresh_session.update(changed_inputs.clone(), &mut fresh_shaper).unwrap();
        let fresh = build_snapshot(fresh_session.document().expect("document should exist after full rebuild"));

        assert_eq!(incremental, fresh);
    }

    #[test]
    fn incremental_and_fresh_rebuilds_match_for_width_then_font_then_width() {
        let source = "<html><body><p>Hello world</p><ul><li>one</li><li>two</li><li>three</li></ul></body></html>";
        let mut updates = vec![
            {
                let mut base = base_inputs(source, SourceRevision::INITIAL);
                base.layout.viewport_width = 580.0;
                base
            },
            {
                let mut font_up = base_inputs(source, SourceRevision::INITIAL);
                font_up.layout.viewport_width = 580.0;
                font_up.style_environment.root_font_size = 20;
                font_up.layout.line_height = 20.0;
                font_up
            },
            {
                let mut final_input = base_inputs(source, SourceRevision::INITIAL);
                final_input.style_environment.root_font_size = 20;
                final_input.layout = LayoutConstraints { viewport_width: 430.0, line_height: 20.0 };
                final_input
            },
        ];

        let incremental = update_and_snapshot(&updates).expect("incremental pipeline should succeed");
        let fresh = {
            let mut shaper = TracingShaper::default();
            let mut session = PipelineSession::new(Arc::new(MockProvider));
            let mut inputs = updates.pop().expect("final input must exist");
            inputs.source_revision = SourceRevision::INITIAL;
            session.update(inputs, &mut shaper).unwrap();
            build_snapshot(session.document().expect("document should exist"))
        };

        assert_eq!(incremental, fresh);
    }

    #[test]
    fn width_changes_can_be_repeated_without_triggering_reparse_or_reshaping() {
        let mut shaper = TracingShaper::default();
        let mut session = PipelineSession::new(Arc::new(MockProvider));

        let input = base_inputs("<html><body><p>layout stability test</p></body></html>", SourceRevision::INITIAL);
        let update = session.update(input.clone(), &mut shaper).unwrap();
        assert_eq!(update.stage, EarliestStage::Parse);

        let after_parse_calls = shaper.calls;
        let mut next = input.clone();
        next.layout.viewport_width = 450.0;
        let width_update_1 = session.update(next.clone(), &mut shaper).unwrap();
        assert_eq!(width_update_1.stage, EarliestStage::Layout);
        assert_eq!(shaper.calls, after_parse_calls);

        let no_change = session.update(next.clone(), &mut shaper).unwrap();
        assert_eq!(no_change.stage, EarliestStage::Paint);
        assert_eq!(shaper.calls, after_parse_calls);

        next.layout.viewport_width = 640.0;
        let width_update_2 = session.update(next.clone(), &mut shaper).unwrap();
        assert_eq!(width_update_2.stage, EarliestStage::Layout);
        assert_eq!(shaper.calls, after_parse_calls);
    }

    #[test]
    fn simultaneous_style_and_layout_changes_choose_style_stage() {
        let mut shaper = TracingShaper::default();
        let mut session = PipelineSession::new(Arc::new(MockProvider));

        let mut inputs = base_inputs("<html><body><p>one</p></body></html>", SourceRevision::INITIAL);
        let _ = session.update(inputs.clone(), &mut shaper).unwrap();

        inputs.stylesheet_revision = StylesheetRevision::next(inputs.stylesheet_revision);
        inputs.layout.viewport_width = 420.0;
        let update = session.update(inputs, &mut shaper).unwrap();
        assert_eq!(update.stage, EarliestStage::Prepare);
        assert_eq!(update.report.styled, Reuse::recomputed(EarliestStage::Style));
        assert_eq!(update.report.prepared, Reuse::recomputed(EarliestStage::Style));
    }

    #[test]
    fn stylesheet_replacement_and_restoration_maps_to_style_stage_and_preserves_fresh_rebuild_equivalence() {
        let mut shaper = SizedShaper::default();
        let provider = MutableProvider::default();
        provider.put_bytes("style.css", b"p { font-size: 9px; }".to_vec());

        let source = r#"<html>
            <head>
                <link rel="stylesheet" href="style.css" />
            </head>
            <body><p>Long text used to prove wrapping changes across style revisions.</p></body>
            </html>"#;
        let mut update_inputs = base_inputs(source, SourceRevision::INITIAL);
        update_inputs.stylesheet_revision = StylesheetRevision::INITIAL;

        let mut session = PipelineSession::new(Arc::new(provider.clone()));
        session.update(update_inputs.clone(), &mut shaper).expect("baseline update should succeed");
        let baseline = {
            let document = session.document().expect("document should exist");
            build_snapshot(document)
        };

        provider.put_bytes("style.css", b"p { font-size: 36px; }".to_vec());
        update_inputs.stylesheet_revision = StylesheetRevision::next(update_inputs.stylesheet_revision);
        let style_update = session.update(update_inputs.clone(), &mut shaper).expect("style replacement update should succeed");
        assert_eq!(style_update.stage, EarliestStage::Prepare);
        assert_eq!(style_update.report.styled, Reuse::recomputed(EarliestStage::Style));
        let replaced = {
            let document = session.document().expect("document should exist");
            build_snapshot(document)
        };
        assert_ne!(baseline, replaced);

        provider.put_bytes("style.css", b"p { font-size: 9px; }".to_vec());
        update_inputs.stylesheet_revision = StylesheetRevision::next(update_inputs.stylesheet_revision);
        let restore = session.update(update_inputs, &mut shaper).expect("stylesheet restoration update should succeed");
        assert_eq!(restore.stage, EarliestStage::Prepare);
        let restored = {
            let document = session.document().expect("document should exist");
            build_snapshot(document)
        };
        assert_eq!(restore.report.parsed, Reuse::reused(EarliestStage::Style));
        assert_eq!(baseline, restored);
    }

    #[test]
    fn pipeline_changes_map_to_expected_earliest_stage() {
        let previous = base_inputs("<html><body><p>x</p></body></html>", SourceRevision::INITIAL);
        let mut next = previous.clone();
        next.source = "<html><body><p>y</p></body></html>".to_owned();
        assert_eq!(next.earliest_stage(&previous), EarliestStage::Parse);

        let mut next = previous.clone();
        next.stylesheet_revision = StylesheetRevision::next(previous.stylesheet_revision);
        assert_eq!(next.earliest_stage(&previous), EarliestStage::Style);

        let mut next = previous.clone();
        next.font_environment = FontEnv::next(previous.font_environment);
        assert_eq!(next.earliest_stage(&previous), EarliestStage::Shape);

        let mut next = previous.clone();
        next.layout.viewport_width = 500.0;
        assert_eq!(next.earliest_stage(&previous), EarliestStage::Layout);

        let mut next = previous.clone();
        next.layout.line_height = 20.0;
        assert_eq!(next.earliest_stage(&previous), EarliestStage::Layout);

        let mut next = previous.clone();
        next.image_metrics_revision = ImageMetricsRevision::next(previous.image_metrics_revision);
        assert_eq!(next.earliest_stage(&previous), EarliestStage::Layout);

        let mut next = previous.clone();
        next.paint = PaintSettingsRevision::next(previous.paint);
        assert_eq!(next.earliest_stage(&previous), EarliestStage::Paint);

        let changes = [
            (PipelineChange::SourceChanged(SourceRevision::INITIAL), EarliestStage::Parse),
            (PipelineChange::ResourceBaseChanged(ResourceRevision::INITIAL), EarliestStage::Parse),
            (PipelineChange::StylesheetsChanged(StylesheetRevision::INITIAL), EarliestStage::Style),
            (PipelineChange::StyleEnvironmentChanged(StyleEnvironment::default()), EarliestStage::Style),
            (PipelineChange::FontEnvironmentChanged(FontEnv::INITIAL), EarliestStage::Shape),
            (PipelineChange::ImageMetricsChanged(ImageMetricsRevision::INITIAL), EarliestStage::Layout),
            (PipelineChange::LayoutConstraintsChanged(LayoutConstraints { viewport_width: 600.0, line_height: 20.0 }), EarliestStage::Layout),
            (PipelineChange::PaintSettingsChanged(PaintSettingsRevision::INITIAL), EarliestStage::Paint),
        ];
        for (change, expected) in changes {
            assert_eq!(change.invalidated_stage(), expected);
        }
    }
}
