//! Fixed-page image geometry, independent of GPUI and archive I/O.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageZoom {
    FitPage,
    FitWidth,
    Fixed(f32),
}

#[derive(Clone, Copy, Debug)]
pub struct PageLayout {
    pub width: f32,
    pub height: f32,
    pub left: f32,
    pub top: f32,
    pub scale: f32,
    pub max_offset: f32,
    pub pan_x: f32,
}

impl PageZoom {
    /// Pixels per source unit. Page units may be image pixels or PDF points.
    pub fn scale(self, page: (f32, f32), viewport: (f32, f32)) -> f32 {
        let width = viewport.0.max(f32::MIN_POSITIVE) / page.0.max(f32::MIN_POSITIVE);
        let height = viewport.1.max(f32::MIN_POSITIVE) / page.1.max(f32::MIN_POSITIVE);
        match self {
            Self::FitPage => width.min(height),
            Self::FitWidth => width,
            Self::Fixed(scale) => scale.clamp(0.01, 16.0),
        }
    }
}

impl PageLayout {
    pub fn new(page: (u32, u32), viewport: (f32, f32), zoom: PageZoom, offset: f32, pan_x: f32) -> Self {
        let (vw, vh) = (viewport.0.max(1.0), viewport.1.max(1.0));
        let (pw, ph) = (page.0.max(1) as f32, page.1.max(1) as f32);
        let scale = zoom.scale((pw, ph), (vw, vh));
        let (width, height) = (pw * scale, ph * scale);
        let max_offset = ((height - vh) / height).clamp(0.0, 1.0);
        let pan_x = pan_x.clamp(0.0, (width - vw).max(0.0));
        Self { width, height, left: ((vw - width) * 0.5).max(0.0) - pan_x, top: if height <= vh { (vh - height) * 0.5 } else { -offset.clamp(0.0, max_offset) * height }, scale, max_offset, pan_x }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_accepts_pdf_points_and_image_pixels_and_caps_wide_pages() {
        let viewport = (800.0, 600.0);
        assert_eq!(PageZoom::FitPage.scale((600.0, 1200.0), viewport), 0.5);
        assert_eq!(PageZoom::FitPage.scale((2400.0, 4800.0), viewport), 0.125);
        assert_eq!(PageZoom::FitPage.scale((1600.0, 300.0), viewport), 0.5);
        assert_eq!(PageZoom::FitWidth.scale((1600.0, 3000.0), viewport), 0.5);
    }

    #[test]
    fn fitting_and_panning_preserve_aspect_ratio_and_bounds() {
        let fit = PageLayout::new((1000, 2000), (800.0, 600.0), PageZoom::FitPage, 0.0, 0.0);
        assert_eq!((fit.width, fit.height, fit.left, fit.top), (300.0, 600.0, 250.0, 0.0));
        let width = PageLayout::new((1000, 2000), (800.0, 600.0), PageZoom::FitWidth, 1.0, 20.0);
        assert_eq!((width.width, width.height, width.top, width.pan_x), (800.0, 1600.0, -1000.0, 0.0));
        let zoomed = PageLayout::new((1000, 2000), (800.0, 600.0), PageZoom::Fixed(2.0), 1.0, 10000.0);
        assert_eq!(zoomed.left, -1200.0);
        assert_eq!(zoomed.top, -3400.0);
    }
}
