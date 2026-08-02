use kurbo::{Point, Size};

use crate::{GlyphShaper, PaintDirection, Painter, RendererCore};
use html_pipeline::PipelineInputs;

impl RendererCore {
    fn apply_image_poll(&mut self, glyph_shaper: &mut impl GlyphShaper, image_poll: crate::resources::ImagePipelinePoll) {
        if image_poll.decoded {
            self.host.request_repaint();
        }

        if image_poll.dimensions_changed.is_empty() {
            return;
        }

        let images = self.document.render_view().images();
        let (next_inputs, layout_needed) = apply_image_poll_updates(
            &self.pipeline_inputs,
            &image_poll.dimensions_changed,
            |image_idx| images.get(image_idx as usize).map(|resource| (resource.width, resource.height, resource.width_attr, resource.height_attr)),
            |image_idx| self.image_pipeline.get_decoded(image_idx).map(|decoded| (decoded.image.width, decoded.image.height)),
        );

        if !layout_needed {
            return;
        }

        let mut next_inputs = next_inputs;
        next_inputs.image_metrics_revision = next_inputs.image_metrics_revision.next();
        let _ = self.apply_pipeline_inputs(next_inputs, glyph_shaper);
    }

    /// Updates viewport-dependent layout and incorporates completed image work.
    pub fn prepare_frame(&mut self, glyph_shaper: &mut impl GlyphShaper, viewport_size: Size) {
        let image_poll = self.image_pipeline.poll();
        self.apply_image_poll(glyph_shaper, image_poll);
        self.configure_layout_for_viewport(glyph_shaper, viewport_size);
    }

    /// Paints the current page and completes image prefetch and navigation
    /// bookkeeping for the frame.
    pub fn paint_frame(&mut self, painter: &mut impl Painter) {
        match self.viewport.direction {
            PaintDirection::Forward => {
                self.paint_forward(painter);
            }
            PaintDirection::Backward => {
                self.paint_backward(painter);
            }
        }
        self.prefetch_images_for_current_page();
        if self.nav.pending_nav_anchor_update {
            self.update_nav_anchor_from_layout();
            self.persist_nav_position();
            self.nav.pending_nav_anchor_update = false;
        }
        self.update_nav_signal();
    }

    pub(crate) fn configure_layout_for_viewport(&mut self, glyph_shaper: &mut impl GlyphShaper, viewport_size: Size) {
        const MIN_GAP: f64 = 40.0;
        self.layout.size = viewport_size;
        self.layout.size /= self.layout.scale;
        let effective_col_width = if self.layout.size.width > 0.0 { self.layout.base_col_width.min(self.layout.size.width) } else { self.layout.base_col_width };
        if (effective_col_width - self.layout.col_width).abs() > 24.0 {
            self.layout.col_width = effective_col_width;
            self.relayout_for_column_width(glyph_shaper);
        }
        let raw_col_count = ((self.layout.size.width + MIN_GAP) / (self.layout.col_width + MIN_GAP)).floor();
        self.layout.col_count = raw_col_count.max(1.0);
        self.layout.col_gap = ((self.layout.size.width - self.layout.col_count * self.layout.col_width) / (self.layout.col_count + 1.0)).max(0.0);
    }

    /// Returns whether a point in renderer-layout coordinates is over the
    /// painted bounds of a linked glyph.
    pub fn link_at(&self, position: Point) -> bool {
        let Some(glyph_idx) = self.hit_test_glyph(position) else {
            return false;
        };
        if self.document.render_view().link_href_for_glyph(glyph_idx).is_none() {
            return false;
        }
        let Some(line_idx) = self.find_line_for_glyph(glyph_idx) else {
            return false;
        };
        let Some(screen) = self.frame.last_line_positions.get(line_idx).copied().flatten() else {
            return false;
        };
        let Some(line) = self.document.render_view().line(line_idx) else {
            return false;
        };
        let glyph_start = screen.point.x + self.glyph_x_in_line(&line, glyph_idx);
        let view = self.document.render_view();
        let glyph = view.glyph_at(glyph_idx as usize).unwrap_or_default();
        let glyph_width = view.glyph_metric(glyph).expect("layout glyph IDs are registered").advance() as f64;
        position.x >= glyph_start && position.x <= glyph_start + glyph_width
    }

    pub fn begin_selection_at(&mut self, position: Point, select_whole_lines: bool) -> bool {
        let Some(glyph_idx) = self.hit_test_glyph(position) else {
            return false;
        };
        self.selection.selection_anchor = Some(glyph_idx);
        self.selection.selection_active = Some(glyph_idx);
        self.selection.is_selecting = true;
        self.selection.select_whole_lines = select_whole_lines;
        self.selection.pending_link_glyph = Some(glyph_idx);
        self.update_selection_text();
        self.host.request_repaint();
        true
    }

    pub fn update_selection_at(&mut self, position: Point, select_whole_lines: bool) -> bool {
        if !self.selection.is_selecting {
            return false;
        }
        if select_whole_lines {
            self.selection.select_whole_lines = true;
        }
        let Some(glyph_idx) = self.hit_test_glyph(position) else {
            return false;
        };
        self.selection.selection_active = Some(glyph_idx);
        if self.selection.selection_anchor != self.selection.selection_active {
            self.selection.pending_link_glyph = None;
        }
        self.update_selection_text();
        self.host.request_repaint();
        true
    }

    /// Completes selection and returns the glyph that still qualifies as a
    /// click. The UI backend decides whether and how that click is activated.
    pub fn finish_selection(&mut self) -> Option<u32> {
        self.selection.is_selecting = false;
        self.update_selection_text();
        let click = match (self.selection.selection_anchor, self.selection.selection_active, self.selection.pending_link_glyph) {
            (Some(anchor), Some(active), Some(pending)) if anchor == active && pending == anchor => Some(anchor),
            _ => None,
        };
        self.selection.pending_link_glyph = None;
        click
    }

    pub fn copy_selection_to_clipboard(&self) -> Result<bool, String> {
        let Some(text) = self.selection.selected_text_markdown.as_deref() else {
            return Ok(false);
        };
        self.host.set_clipboard(text)?;
        Ok(true)
    }

    pub fn copy_image_to_clipboard(&self, image_idx: u32) -> Result<(), String> {
        let decoded = self.image_pipeline.get_decoded(image_idx).ok_or_else(|| format!("image {image_idx} is not decoded yet"))?;
        self.host.set_clipboard_image(decoded.image.width as usize, decoded.image.height as usize, decoded.image.data.data().to_vec())
    }
}

fn apply_image_poll_updates(
    pipeline_inputs: &PipelineInputs, dimensions_changed: &[u32], image_metadata: impl Fn(u32) -> Option<(u32, u32, Option<u32>, Option<u32>)>, decode_sizes: impl Fn(u32) -> Option<(u32, u32)>,
) -> (PipelineInputs, bool) {
    let mut next_inputs = pipeline_inputs.clone();
    let mut layout_needed = false;

    for image_idx in dimensions_changed {
        let (resource_width, resource_height, width_attr, height_attr) = match image_metadata(*image_idx) {
            Some((resource_width, resource_height, width_attr, height_attr)) => (resource_width, resource_height, width_attr, height_attr),
            None => continue,
        };
        let Some((decoded_width, decoded_height)) = decode_sizes(*image_idx) else {
            continue;
        };

        let uses_intrinsic_width = width_attr.is_none();
        let uses_intrinsic_height = height_attr.is_none();
        let previous = next_inputs.image_metrics.get(*image_idx).unwrap_or((resource_width, resource_height));
        if (uses_intrinsic_width && previous.0 != decoded_width) || (uses_intrinsic_height && previous.1 != decoded_height) {
            layout_needed = true;
        }
        next_inputs.image_metrics.set(*image_idx, decoded_width, decoded_height);
    }

    (next_inputs, layout_needed)
}

#[cfg(test)]
mod tests {
    use super::apply_image_poll_updates;
    use crate::{FontStyle, GlyphId, GlyphMetric, GlyphShaper};
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestShaper {
        glyphs: HashMap<(char, u32), GlyphId>,
    }

    impl GlyphShaper for TestShaper {
        fn reset(&mut self) {
            self.glyphs.clear();
        }

        fn shape_glyph<'a>(
            &mut self, glyph_metrics: &mut crate::GlyphRegistry<'a>, ch: char, font_size: f32, _font_weight: u16, _font_style: FontStyle, _color: u32, _font_family: Option<crate::StyleStringId>, _family: Option<&str>,
        ) -> Result<GlyphId, crate::layout::ShapeError> {
            let key = (ch, font_size.to_bits());
            if let Some(&glyph) = self.glyphs.get(&key) {
                return Ok(glyph);
            }
            let metric = GlyphMetric::try_new(ch, font_size * 0.5, font_size * 0.75, font_size * 0.25, font_size * 0.75).unwrap();
            let glyph = glyph_metrics.register(metric).unwrap();
            self.glyphs.insert(key, glyph);
            Ok(glyph)
        }
    }

    fn test_resources() -> Vec<(u32, u32, Option<u32>, Option<u32>)> {
        vec![(64, 32, None, None), (12, 12, Some(12), Some(12)), (40, 80, Some(40), None)]
    }

    fn base_inputs() -> html_pipeline::PipelineInputs {
        html_pipeline::PipelineInputs {
            source: "<html />".to_string(),
            source_revision: html_pipeline::SourceRevision::INITIAL,
            base_uri: "doc.html".to_string(),
            resource_revision: html_pipeline::ResourceRevision::INITIAL,
            stylesheet_revision: html_pipeline::StylesheetRevision::INITIAL,
            style_environment: html_pipeline::StyleEnvironment::default(),
            font_environment: html_pipeline::FontEnvironmentRevision::INITIAL,
            image_metrics_revision: html_pipeline::ImageMetricsRevision::INITIAL,
            layout: html_pipeline::LayoutConstraints { viewport_width: 600.0, line_height: 16.0 },
            image_metrics: html_layout::ImageMetrics::default(),
            paint: html_pipeline::PaintSettingsRevision::INITIAL,
        }
    }

    #[test]
    fn image_poll_no_layout_needed_when_dimensions_do_not_change() {
        let resources = test_resources();
        let mut next_inputs = base_inputs();
        next_inputs.image_metrics = html_layout::ImageMetrics::default();

        let (updated, layout_needed) = apply_image_poll_updates(
            &next_inputs,
            &[0, 1, 2],
            |idx| resources.get(idx as usize).copied(),
            |idx| match idx {
                0 => Some((64, 32)),
                1 => Some((16, 16)),
                2 => Some((40, 80)),
                _ => None,
            },
        );

        assert!(!layout_needed, "intrinsic-only image changes should not force layout if no intrinsic dimensions changed");
        assert_eq!(updated.image_metrics.get(0), Some((64, 32)));
        assert_eq!(updated.image_metrics.get(1), Some((16, 16)));
        assert_eq!(updated.image_metrics.get(2), Some((40, 80)));
    }

    #[test]
    fn image_poll_out_of_order_batch_has_stable_result_and_single_revision_bump() {
        let resources = test_resources();
        let mut base = base_inputs();
        base.image_metrics.set(0, 10, 10);
        base.image_metrics.set(1, 11, 12);
        base.image_metrics.set(2, 13, 14);

        let (updated_ab, layout_ab) = apply_image_poll_updates(
            &base,
            &[2, 0, 1],
            |idx| resources.get(idx as usize).copied(),
            |idx| match idx {
                0 => Some((20, 10)),
                1 => Some((11, 12)),
                2 => Some((13, 99)),
                _ => None,
            },
        );

        let (updated_ba, layout_ba) = apply_image_poll_updates(
            &base,
            &[1, 2, 0],
            |idx| resources.get(idx as usize).copied(),
            |idx| match idx {
                0 => Some((20, 10)),
                1 => Some((11, 12)),
                2 => Some((13, 99)),
                _ => None,
            },
        );

        assert!(layout_ab);
        assert!(layout_ba);

        assert_eq!(updated_ab.image_metrics.get(0), updated_ba.image_metrics.get(0));
        assert_eq!(updated_ab.image_metrics.get(1), updated_ba.image_metrics.get(1));
        assert_eq!(updated_ab.image_metrics.get(2), updated_ba.image_metrics.get(2));

        // A caller applying this batch should only need one revision bump.
        assert_eq!(updated_ab.image_metrics_revision.next(), base.image_metrics_revision.next());
        assert_eq!(updated_ba.image_metrics_revision.next(), base.image_metrics_revision.next());
    }

    #[test]
    fn image_poll_with_out_of_order_batch_applies_one_revision_bump() {
        let resources = test_resources();
        let mut base = base_inputs();
        base.image_metrics.set(0, 10, 10);
        base.image_metrics.set(1, 11, 12);
        base.image_metrics.set(2, 13, 14);

        let (updated, layout_needed) = apply_image_poll_updates(
            &base,
            &[2, 0, 1],
            |idx| resources.get(idx as usize).copied(),
            |idx| match idx {
                0 => Some((20, 10)),
                1 => Some((11, 12)),
                2 => Some((13, 99)),
                _ => None,
            },
        );

        assert!(layout_needed);
        assert_eq!(updated.image_metrics.get(0), Some((20, 10)));
        assert_eq!(updated.image_metrics.get(1), Some((11, 12)));
        assert_eq!(updated.image_metrics.get(2), Some((13, 99)));
        assert_eq!(updated.image_metrics.get(0), Some((20, 10)));
    }
}
