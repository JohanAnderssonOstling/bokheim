//! Bounded image decoding shared by the renderer and its headless checks.

use image::{DynamicImage, ImageDecoder, ImageReader, RgbaImage};
use std::io::Cursor;

const MAX_SOURCE_PIXELS: u64 = 24_000_000;
const MAX_DISPLAY_PIXELS: u64 = 8_000_000;

pub(super) struct DecodedPage {
    pub pixels: RgbaImage,
    pub width: u32,
    pub height: u32,
}

pub(super) fn decode(bytes: Vec<u8>) -> Result<DecodedPage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().map_err(|error| error.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(|error| error.to_string())?;
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || width > 32_000 || height > 32_000 || pixels > MAX_SOURCE_PIXELS {
        return Err("Comic page exceeds the image dimension limit".into());
    }
    // ImageReader::decode also reserves the output buffer before giving the
    // remaining allocation budget to the decoder.
    limits.reserve(decoder.total_bytes()).map_err(|error| error.to_string())?;
    decoder.set_limits(limits).map_err(|error| error.to_string())?;
    let decoded = DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())?;
    let scale = (MAX_DISPLAY_PIXELS as f64 / pixels as f64).sqrt().min(4096.0 / f64::from(width.max(height))).min(1.0);
    let mut rgba = if scale < 1.0 { decoded.resize((f64::from(width) * scale).round().max(1.0) as u32, (f64::from(height) * scale).round().max(1.0) as u32, image::imageops::FilterType::Triangle).to_rgba8() } else { decoded.to_rgba8() };
    // GPUI's decoded image buffers use BGRA channel order.
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let (width, height) = rgba.dimensions();
    Ok(DecodedPage { pixels: rgba, width, height })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

    #[test]
    fn decodes_to_bgra_and_rejects_non_images() {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 3, Rgba([10, 20, 30, 255]))).write_to(&mut bytes, ImageFormat::Png).unwrap();
        let decoded = decode(bytes.into_inner()).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 3));
        assert_eq!(&decoded.pixels.as_raw()[..4], &[30, 20, 10, 255]);
        assert!(decode(b"invalid page".to_vec()).is_err());
    }

    #[test]
    fn bounds_source_dimensions_and_scales_display_pixels() {
        let oversized = gpui::Image::from_rgba(&RgbaImage::new(32_001, 1)).unwrap();
        assert!(decode(oversized.bytes).err().unwrap().contains("image dimension limit"));

        let wide = gpui::Image::from_rgba(&RgbaImage::new(5_000, 1)).unwrap();
        let decoded = decode(wide.bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (4_096, 1));
        assert_eq!(decoded.pixels.len(), 4_096 * 4);
    }
}
