use gpui::RenderImage;
use image::{Frame, RgbaImage};
use std::sync::Arc;

/// Decoded BGRA pixels ready for GPUI; format decoders perform channel conversion.
pub struct PageImage {
    pub image: Arc<RenderImage>,
    /// Source dimensions used for zoom, even when the decoder downsamples.
    pub width: u32,
    pub height: u32,
    pub byte_len: usize,
}

impl PageImage {
    pub fn from_bgra(pixels: RgbaImage) -> Self {
        let (width, height) = pixels.dimensions();
        let byte_len = pixels.as_raw().len();
        Self { image: Arc::new(RenderImage::new(vec![Frame::new(pixels)])), width, height, byte_len }
    }

    pub fn with_source_size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }
}
