mod layout_model {
    mod glyph_metrics;
    mod inline;
    mod layout_state;
    mod layout_tree;

    pub(crate) use glyph_metrics::GlyphMetrics;
    pub use glyph_metrics::{GlyphMetric, GlyphMetricError};
    pub use inline::GlyphId;
    pub(crate) use inline::{InlineContent, InlineKind, InlineRun};
    pub(crate) use layout_state::{AnchorPosition, DecorationFragment, GlyphAdvanceRun, GlyphOffsetRun, ImageFragment, LayoutState, Line};
    pub(crate) use layout_tree::{BlockBox, BoxType, Children, LayoutBox, LayoutMode, LayoutTree, ListItemMarker, TableBox, TableCellBox, TableRowBox};
}

pub(crate) mod document {
    pub(crate) use crate::layout_model::{
        AnchorPosition, BlockBox, BoxType, Children, DecorationFragment, GlyphAdvanceRun, GlyphId, GlyphMetric, GlyphMetricError, GlyphMetrics, GlyphOffsetRun, ImageFragment, InlineContent, InlineKind, InlineRun, LayoutBox, LayoutMode,
        LayoutState, LayoutTree, Line, ListItemMarker, TableBox, TableCellBox, TableRowBox,
    };
    pub use html_dom::*;
    pub use html_style_model::*;
}

#[cfg(test)]
pub(crate) mod parser {
    pub(crate) use crate::test_support::DocumentFactory;
}

#[cfg(test)]
pub mod style {
    pub use html_style::*;
}

#[path = "layout/mod.rs"]
mod layout;

pub mod cfi;
mod shaping;
mod stages;

pub use layout::LayoutTimings;
// Stable values crossing the layout-to-renderer boundary.
pub use crate::layout_model::{GlyphId, GlyphMetric};
pub use html_dom::DocumentTocNode;
pub use html_style_model::{FontStyle, ListStylePosition, StyleStringId};
pub use shaping::{GlyphRegistry, GlyphShaper, ShapeError};
pub use stages::{
    BoxTextFormat, ImageMetrics, LaidOutDocument, LayoutConstraintError, LayoutConstraints, PrepareError, PreparedDocument, RenderAllImageFragments, RenderDecorations, RenderGlyphAdvanceRun, RenderGlyphAdvanceRuns, RenderGlyphOffsetRun,
    RenderGlyphOffsetRuns, RenderImageFragment, RenderImageFragments, RenderLine, RenderLines, RenderTextRun, RenderTextRuns, RenderView, ShapedDocument,
};

#[cfg(test)]
mod allocation_test_support {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static ALLOCATION_COUNT: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub(crate) struct TrackingAllocator;

    fn record_allocation() {
        let _ = ALLOCATION_COUNT.try_with(|count| {
            if let Some(current) = count.get() {
                count.set(Some(current + 1));
            }
        });
    }

    unsafe impl GlobalAlloc for TrackingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            record_allocation();
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            record_allocation();
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            record_allocation();
            unsafe { System.realloc(ptr, layout, new_size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub(crate) fn count_allocations<T>(operation: impl FnOnce() -> T) -> (T, usize) {
        ALLOCATION_COUNT.with(|count| {
            assert!(count.get().is_none(), "allocation counters cannot be nested");
            count.set(Some(0));
        });
        let output = operation();
        let allocations = ALLOCATION_COUNT.with(|count| count.take().expect("allocation tracking is active"));
        (output, allocations)
    }
}

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: allocation_test_support::TrackingAllocator = allocation_test_support::TrackingAllocator;

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::HashMap;

    use crate::document::{FontStyle, GlyphId, GlyphMetric};

    use crate::GlyphShaper;

    pub(crate) struct DocumentFactory;

    impl DocumentFactory {
        pub(crate) fn new() -> Self {
            Self
        }

        pub(crate) fn parse_to_dom(&mut self, html: &str) -> html_dom::Document {
            html_parse::parse_dom_document(html).expect("parser produced a valid DOM")
        }

        pub(crate) fn parse_with_new_pipeline(&mut self, html: &str, css: Option<&str>) -> crate::PreparedDocument {
            match css {
                Some(css) => self.parse_with_new_pipeline_css_chunks(html, &[css]),
                None => self.parse_with_new_pipeline_css_chunks(html, &[]),
            }
        }

        pub(crate) fn parse_with_new_pipeline_css_chunks(&mut self, html: &str, css: &[&str]) -> crate::PreparedDocument {
            let document = html_parse::parse_dom_document(html).expect("parser produced a valid DOM");
            let styled = html_style::style_document(document, css);
            let (document, styles) = styled.into_parts();
            crate::PreparedDocument::try_new(document, styles).expect("style resolver must produce complete styles for its document")
        }
    }

    #[derive(Default)]
    pub(crate) struct TestGlyphShaper {
        glyphs: HashMap<(char, u32), GlyphId>,
    }

    impl TestGlyphShaper {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn len(&self) -> usize {
            self.glyphs.len()
        }
    }

    impl GlyphShaper for TestGlyphShaper {
        fn reset(&mut self) {
            self.glyphs.clear();
        }

        fn shape_glyph<'a>(
            &mut self, glyph_metrics: &mut crate::GlyphRegistry<'a>, ch: char, font_size: f32, _font_weight: u16, _font_style: FontStyle, _color: u32, _font_family: Option<html_style_model::StyleStringId>, _family: Option<&str>,
        ) -> Result<GlyphId, crate::ShapeError> {
            let key = (ch, font_size.to_bits());
            if let Some(&glyph) = self.glyphs.get(&key) {
                return Ok(glyph);
            }
            let metric = GlyphMetric::try_new(ch, font_size * 0.5, font_size * 0.75, font_size * 0.25, font_size * 0.75).map_err(crate::ShapeError::rejected_metric)?;
            let glyph = glyph_metrics.register(metric)?;
            self.glyphs.insert(key, glyph);
            Ok(glyph)
        }
    }
}
