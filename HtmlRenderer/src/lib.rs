mod renderer;
#[doc(hidden)]
pub mod text;

mod document {
    pub(crate) use html::document::MemoryUsageReport;
}

pub use epub_provider::TocEntry;
pub use html::render::{FragmentImagePolicy, FragmentRenderError, FragmentRenderOptions, FragmentRenderer, Painter, RecordingPainter, RenderCommand, RenderScene, RenderedFragment};
pub use renderer::{HTMLRenderer, RendererCommand, RendererEvent, RendererHandle, RendererHost, RendererOperation};
