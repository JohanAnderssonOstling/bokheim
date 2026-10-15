//! Card-only crop and stretch, applied once while generating JPEG assets.
use super::{encode_jpeg, validate_cover_image_dimensions, ThumbnailResult};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbImage;

pub const CARD_TARGET_HEIGHT_RATIO: f64 = 1.6;
pub const CARD_MAX_HORIZONTAL_CROP: f64 = 0.07;
/// The same limit bounds stretching short covers and compressing tall ones.
pub const CARD_MAX_VERTICAL_STRETCH: f64 = 0.20;
/// Increment when the crop policy changes, to retire previously cached cards.
pub const CARD_THUMBNAIL_VERSION: u32 = 5;

/// Source crop in pixels and final JPEG dimensions. Detail/master thumbnails
/// never use this geometry; all card densities use the same source crop.
#[derive(Debug, Clone, Copy)]
pub struct CardThumbnailGeometry {
    pub left: f64,
    pub top: f64,
    pub source_width: f64,
    pub source_height: f64,
    pub output_width: u32,
    pub output_height: u32,
}

pub fn card_thumbnail_geometry(source_width: u32, source_height: u32, width: u32) -> ThumbnailResult<CardThumbnailGeometry> {
    validate_cover_image_dimensions(source_width, source_height)?;
    if width == 0 {
        return Err("thumbnail width is zero".into());
    }
    let (original_width, original_height) = (f64::from(source_width), f64::from(source_height));
    let ratio = original_height / original_width;
    let mut crop_width = original_width;
    let mut crop_height = original_height;
    let mut output_ratio = ratio;
    if ratio > CARD_TARGET_HEIGHT_RATIO {
        // Mild compression retains more of a tall cover before its remaining
        // excess is cropped. Split small corrections between the two effects.
        let correction = CARD_TARGET_HEIGHT_RATIO / ratio;
        let compression = correction.sqrt().max(1.0 - CARD_MAX_VERTICAL_STRETCH);
        crop_height = original_width * CARD_TARGET_HEIGHT_RATIO / compression;
        output_ratio = CARD_TARGET_HEIGHT_RATIO;
    } else if ratio < CARD_TARGET_HEIGHT_RATIO {
        // Short covers of every shape share the bounded adjustment. Split
        // mild corrections between crop and vertical stretch.
        let correction = CARD_TARGET_HEIGHT_RATIO / ratio;
        let stretch = correction.sqrt().min(1.0 + CARD_MAX_VERTICAL_STRETCH);
        let crop = (1.0 - stretch / correction).clamp(0.0, CARD_MAX_HORIZONTAL_CROP);
        // If cropping reaches its budget, use the remaining stretch allowance
        // to reach the target when possible instead of stopping unnecessarily.
        let stretch = (correction * (1.0 - crop)).min(1.0 + CARD_MAX_VERTICAL_STRETCH);
        crop_width *= 1.0 - crop;
        output_ratio = (ratio * stretch / (1.0 - crop)).min(CARD_TARGET_HEIGHT_RATIO);
    }
    let mut output_height = (f64::from(width) * output_ratio).round().max(1.0) as u32;
    if ratio < CARD_TARGET_HEIGHT_RATIO {
        // Integer rounding must not push the actual vertical stretch over 20%.
        let max_height = (f64::from(width) * original_height * (1.0 + CARD_MAX_VERTICAL_STRETCH) / crop_width).floor().max(1.0) as u32;
        output_height = output_height.min(max_height);
    }
    validate_cover_image_dimensions(width, output_height)?;
    Ok(CardThumbnailGeometry { left: (original_width - crop_width) / 2.0, top: (original_height - crop_height) / 2.0, source_width: crop_width, source_height: crop_height, output_width: width, output_height })
}

pub fn encode_card_thumbnail_at_width(decoded: &RgbImage, width: u32) -> ThumbnailResult<Vec<u8>> {
    let geometry = card_thumbnail_geometry(decoded.width(), decoded.height(), width)?;
    let source = ImageRef::new(decoded.width(), decoded.height(), decoded.as_raw(), PixelType::U8x3)?;
    let mut destination = Image::new(geometry.output_width, geometry.output_height, PixelType::U8x3);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Hamming)).crop(geometry.left, geometry.top, geometry.source_width, geometry.source_height);
    Resizer::new().resize(&source, &mut destination, Some(&options))?;
    encode_jpeg(destination.buffer(), geometry.output_width, geometry.output_height)
}

pub fn card_thumbnail_suffix(width: u32) -> String {
    format!("card-v{CARD_THUMBNAIL_VERSION}-{width}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ThumbnailVersions, BROWSE_THUMBNAIL_WIDTH, THUMBNAIL_WIDTH};
    use image::Rgb;

    #[test]
    fn square_and_portrait_crops_and_stretch_stay_within_budgets_at_both_densities() {
        for width in [BROWSE_THUMBNAIL_WIDTH, THUMBNAIL_WIDTH] {
            for height in 240..480 {
                let geometry = card_thumbnail_geometry(300, height, width).unwrap();
                let crop = 1.0 - geometry.source_width / 300.0;
                let stretch = f64::from(geometry.output_height) * geometry.source_width / (f64::from(width) * f64::from(height));
                assert!((0.0..=0.07 + 1e-12).contains(&crop), "{crop}");
                assert!(stretch >= 1.0 - 1.0 / f64::from(width) && stretch <= 1.20 + 1e-12, "{stretch}");
                assert!(f64::from(geometry.output_height) / f64::from(width) <= 1.6);
                assert_eq!(geometry.top, 0.0);
                assert!((geometry.left * 2.0 + geometry.source_width - 300.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn square_and_landscape_covers_both_receive_the_bounded_adjustment() {
        let square = card_thumbnail_geometry(600, 600, 300).unwrap();
        assert_eq!(square.output_height, 387);
        assert_eq!(square.source_width, 558.0);
        assert_eq!(square.left, 21.0);
        let landscape = card_thumbnail_geometry(600, 480, 300).unwrap();
        assert_eq!(landscape.output_height, 309);
        assert_eq!(landscape.source_width, 558.0);
        assert_eq!(landscape.left, 21.0);
        assert_eq!(landscape.top, 0.0);
    }

    #[test]
    fn tall_cover_clips_centered_top_and_bottom_in_the_generated_jpeg() {
        let source = RgbImage::from_fn(300, 600, |_, y| if (60..540).contains(&y) { Rgb([20, 160, 30]) } else { Rgb([220, 10, 20]) });
        let geometry = card_thumbnail_geometry(300, 600, 300).unwrap();
        assert!((geometry.top - 31.6718427000).abs() < 1e-9);
        assert!((geometry.source_height - 536.6563145999).abs() < 1e-9);
        let jpeg = encode_card_thumbnail_at_width(&source, 300).unwrap();
        let card = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        assert_eq!(card.dimensions(), (300, 480));
        for y in [30, 240, 449] {
            assert!(card.get_pixel(150, y).0[1] > 140, "uncropped band at {y}");
        }
        assert!(card.get_pixel(150, 0).0[0] > 200, "compression should retain artwork beyond the former crop");
    }

    #[test]
    fn tall_cover_compression_stays_bounded_and_reduces_vertical_cropping() {
        for width in [300, 600] {
            for height in [966, 1020, 1080, 1200, 1300, 1800] {
                let geometry = card_thumbnail_geometry(600, height, width).unwrap();
                let scale = f64::from(geometry.output_height) * geometry.source_width / (f64::from(width) * geometry.source_height);
                assert!((0.80 - 1e-12..=1.0).contains(&scale));
                assert!(geometry.source_height > 960.0 && geometry.source_height <= f64::from(height));
                assert!((geometry.top * 2.0 + geometry.source_height - f64::from(height)).abs() < 1e-9);
                assert_eq!(geometry.output_height * 5, width * 8);
            }
        }
    }

    #[test]
    fn card_derivatives_are_adjusted_but_the_master_keeps_the_complete_cover() {
        let source = RgbImage::from_fn(600, 900, |x, _| if !(5..595).contains(&x) { Rgb([220, 10, 20]) } else { Rgb([20, 160, 30]) });
        let versions = ThumbnailVersions::from_image(&source).unwrap();
        let card = image::load_from_memory(&versions.browse).unwrap().to_rgb8();
        let dense_card = image::load_from_memory(&versions.card_high_density).unwrap().to_rgb8();
        let master = image::load_from_memory(&versions.high_density).unwrap().to_rgb8();
        assert_eq!(card.dimensions(), (300, 480));
        assert_eq!(dense_card.dimensions(), (600, 960));
        assert_eq!(master.dimensions(), (600, 900));
        assert!(master.get_pixel(0, 450).0[0] > 200);
        assert!(card.get_pixel(2, 240).0[1] > 140);
    }

    #[test]
    fn capped_cropping_uses_available_stretch_to_reach_target() {
        let geometry = card_thumbnail_geometry(300, 375, 300).unwrap();
        assert_eq!(geometry.output_height, 480);
        assert_eq!(geometry.source_width, 279.0);
        let stretch = f64::from(geometry.output_height) * geometry.source_width / (300.0 * 375.0);
        assert!((stretch - 1.1904).abs() < 1e-12);
        let wider = card_thumbnail_geometry(300, 360, 300).unwrap();
        assert_eq!(wider.output_height, 464);
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        for (width, height, output_width) in [(0, 10, 300), (10, 0, 300), (10, 10, 0)] {
            assert!(card_thumbnail_geometry(width, height, output_width).is_err());
        }
    }
}
