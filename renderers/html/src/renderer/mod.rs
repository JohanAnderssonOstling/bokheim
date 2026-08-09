use std::path::PathBuf;
use std::rc::Rc;

use floem::prelude::Decorators;
use kurbo::Rect;
use peniko::{Brush, Color};

use crate::document::MemoryUsageReport;
use crate::text::glyph_cache::GlyphCache;
use html_view_core::{LaidOutDocument, RendererSession};

mod adapter;
mod boundary;
mod nav;
pub(crate) mod painter;
mod view;

pub use adapter::RendererHandle;
pub use boundary::{RendererCommand, RendererEvent, RendererHost, RendererOperation};

pub struct HTMLRenderer {
    adapter: Rc<adapter::FloemAdapter>,
    renderer: RendererSession<GlyphCache>,
}

impl HTMLRenderer {
    fn from_renderer(adapter: Rc<adapter::FloemAdapter>, renderer: RendererSession<GlyphCache>) -> Self {
        Self { adapter, renderer }.keyboard_navigable().style(move |style| style.width_full().height_full().flex_grow(1.0).min_width(0).min_height(0).background(Brush::Solid(Color::WHITE)))
    }

    pub fn set_render_column_width(&mut self, width: f64) {
        self.renderer.set_column_width(width);
    }

    pub fn document(&self) -> &LaidOutDocument {
        self.renderer.document()
    }

    pub fn memory_usage_report(&self) -> MemoryUsageReport {
        let mut report = self.renderer.document().memory_usage_report();
        report.extend_prefixed("Renderer.glyph_cache", self.renderer.glyph_shaper().memory_usage_report());
        report
    }

    pub fn from_source_path(path: PathBuf) -> Self {
        Self::from_source_path_with_nav(path, None)
    }

    pub fn from_epub_bytes(bytes: Vec<u8>) -> Self {
        Self::from_epub_bytes_with_nav(bytes, None)
    }

    pub fn from_source_path_with_nav(path: PathBuf, nav_state: Option<&str>) -> Self {
        let adapter = adapter::FloemAdapter::new();
        let host: Rc<dyn RendererHost> = adapter.clone();
        let source = html_publication::PublicationSource::from_path(path).expect("failed to open HTML publication");
        let (provider, document_uris, start_index) = source.into_parts();
        let renderer = RendererSession::from_provider_with_nav(host, GlyphCache::new(), provider, document_uris, start_index, nav_state, Default::default());
        Self::from_renderer(adapter, renderer)
    }

    pub fn from_epub_bytes_with_nav(bytes: Vec<u8>, nav_state: Option<&str>) -> Self {
        let adapter = adapter::FloemAdapter::new();
        let host: Rc<dyn RendererHost> = adapter.clone();
        let source = html_publication::PublicationSource::from_epub_bytes(bytes).expect("failed to open EPUB publication");
        let (provider, document_uris, start_index) = source.into_parts();
        let renderer = RendererSession::from_provider_with_nav(host, GlyphCache::new(), provider, document_uris, start_index, nav_state, Default::default());
        Self::from_renderer(adapter, renderer)
    }

    pub fn current_column_width(&self) -> f64 {
        self.renderer.column_width()
    }

    pub fn current_font_size(&self) -> f32 {
        self.renderer.root_font_size()
    }

    pub fn current_scale(&self) -> f64 {
        self.renderer.scale()
    }

    pub fn command_handle(&self) -> RendererHandle {
        self.adapter.handle()
    }

    pub fn set_event_handler(&mut self, callback: impl Fn(RendererEvent) + 'static) {
        self.adapter.set_event_callback(callback);
        self.renderer.emit_state_snapshot();
    }

    pub fn apply(&mut self, command: RendererCommand) {
        self.renderer.apply(command);
    }

    pub fn toc(&self) -> std::io::Result<Option<Vec<crate::TocEntry>>> {
        self.renderer.toc()
    }

    pub fn document_toc(&self) -> Vec<crate::TocEntry> {
        self.renderer.document_toc()
    }

    pub fn get_current_cfi(&self) -> Option<String> {
        self.renderer.current_cfi()
    }

    pub fn set_toc_anchor_strings_by_doc(&mut self, anchors_by_doc: Vec<Vec<String>>) {
        self.renderer.set_toc_anchor_strings_by_doc(anchors_by_doc);
    }

    pub fn set_search_query(&mut self, query: &str) {
        self.renderer.set_search_query(query);
    }

    pub fn next_match(&mut self) {
        self.renderer.next_match();
    }

    pub fn prev_match(&mut self) {
        self.renderer.previous_match();
    }
}
