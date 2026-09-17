use super::*;

/// Only a rectangular, gapless mosaic of opaque, non-overlapping images.
/// Visible text, paths, optional content and uncertain graphics state are rejected by walk().
pub(super) fn compose(document: &Document, candidates: Vec<Candidate<'_>>, rotation: i64, reason: &mut &'static str) -> Option<RgbImage> {
    *reason = "Image tiles are not a simple opaque mosaic";
    let mut bounds = Vec::new();
    let mut union = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for candidate in &candidates {
        let state = &candidate.state;
        if !axis_aligned(state.matrix) || state.grouped || candidate.stream.dict.has(b"SMask") || candidate.stream.dict.has(b"Mask") || candidate.stream.dict.has(b"SMaskInData") {
            return None;
        }
        let rect = intersect(transformed([0., 0., 1., 1.], state.matrix)?, state.clip)?;
        if bounds.iter().any(|other| intersect(rect, *other).is_some()) {
            return None;
        }
        union = [union[0].min(rect[0]), union[1].min(rect[1]), union[2].max(rect[2]), union[3].max(rect[3])];
        bounds.push(rect);
    }
    let area = (union[2] - union[0]) * (union[3] - union[1]);
    let covered: f64 = bounds.iter().map(|r| (r[2] - r[0]) * (r[3] - r[1])).sum();
    if (covered - area).abs() > area * 1e-8 {
        return None;
    }
    let width = THUMBNAIL_WIDTH;
    let height = (f64::from(width) * (union[3] - union[1]) / (union[2] - union[0])).round().max(1.) as u32;
    validate_cover_image_dimensions(width, height).ok()?;
    let mut output = RgbImage::new(width, height);
    for (candidate, rect) in candidates.into_iter().zip(bounds) {
        let image = decode(document, &candidate.stream, candidate.resources, true, true, reason)?;
        let image = place(image, candidate.state)?;
        let x = |v: f64| ((v - union[0]) / (union[2] - union[0]) * f64::from(width)).round() as u32;
        let y = |v: f64| ((union[3] - v) / (union[3] - union[1]) * f64::from(height)).round() as u32;
        let (left, right, top, bottom) = (x(rect[0]), x(rect[2]), y(rect[3]), y(rect[1]));
        if right > left && bottom > top {
            let tile = image::imageops::resize(&image, right - left, bottom - top, image::imageops::FilterType::Lanczos3);
            image::imageops::replace(&mut output, &tile, i64::from(left), i64::from(top));
        }
    }
    Some(match rotation {
        90 => image::imageops::rotate90(&output),
        180 => image::imageops::rotate180(&output),
        270 => image::imageops::rotate270(&output),
        _ => output,
    })
}
