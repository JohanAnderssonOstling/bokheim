mod renderer;
#[doc(hidden)]
pub mod text;

mod document {
    pub(crate) use html_dom::MemoryUsageReport;
    pub(crate) use html_layout::{FontStyle, GlyphId, GlyphMetric};
}

pub use epub_provider::TocEntry;
pub use renderer::{HTMLRenderer, RendererCommand, RendererEvent, RendererHandle, RendererHost, RendererOperation};
