//! Pure cover extraction and JPEG thumbnail generation for supported books.

use epub_provider::{EpubProvider, ResourceProvider};
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::io::Reader as ImageReader;
use image::{ColorType, ImageEncoder, ImageFormat, RgbImage};
use std::io::{BufWriter, Cursor};
use std::path::Path;

const THUMBNAIL_WIDTH: u32 = 300;

pub type ThumbnailResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Extracts a publication cover and returns a resized JPEG. Publications with
/// no cover, and formats without an extractor, return `None`.
pub fn generate_thumbnail(book_path: &Path) -> ThumbnailResult<Option<Vec<u8>>> {
    let extension = book_path.extension().and_then(|extension| extension.to_str()).unwrap_or_default().to_ascii_lowercase();
    let image = match extension.as_str() {
        "epub" => epub_cover(book_path)?,
        "cbz" => cbz_cover(book_path)?,
        "m4b" => m4b_cover(book_path)?,
        "pdf" => pdf_cover(book_path)?,
        _ => None,
    };
    image.map(encode_thumbnail).transpose()
}

fn epub_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let epub = std::fs::File::open(path).and_then(EpubProvider::try_from_reader)?;
    let Some(image_path) = epub.cover_image_path()? else { return Ok(None) };
    let format = image_format(&image_path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unsupported EPUB cover image format: {image_path}")))?;
    let bytes = epub.read_bytes(&image_path)?;
    Ok(Some(ImageReader::with_format(Cursor::new(bytes), format).decode()?.to_rgb8()))
}

fn cbz_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let source = html_publication::PublicationSource::from_path(path.to_path_buf())?;
    let cover_resource = match &source.layout {
        html_publication::PublicationLayout::PagedImages { cover_resource, .. } => cover_resource.as_deref(),
        html_publication::PublicationLayout::Reflowable => None,
    };
    let Some(cover_resource) = cover_resource else { return Ok(None) };
    let bytes = source.provider.read_bytes(cover_resource)?;
    Ok(Some(ImageReader::new(Cursor::new(bytes)).with_guessed_format()?.decode()?.to_rgb8()))
}

fn m4b_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let tag = mp4ameta::Tag::read_from_path(path)?;
    let Some(artwork) = tag.artwork() else { return Ok(None) };
    Ok(Some(ImageReader::new(Cursor::new(artwork.data)).with_guessed_format()?.decode()?.to_rgb8()))
}

#[cfg(feature = "pdf")]
fn pdf_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let rendered = pdf_reader_core::PdfDocumentSession::open(path)?.render_page(0, Some(300))?;
    let rgb = rendered.rgba().chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect::<Vec<_>>();
    Ok(Some(RgbImage::from_raw(rendered.pixel_width(), rendered.pixel_height(), rgb).ok_or("PDF renderer returned invalid image dimensions")?))
}

#[cfg(not(feature = "pdf"))]
fn pdf_cover(_: &Path) -> ThumbnailResult<Option<RgbImage>> {
    Ok(None)
}

fn encode_thumbnail(decoded: RgbImage) -> ThumbnailResult<Vec<u8>> {
    let (source_width, source_height) = decoded.dimensions();
    if source_width == 0 || source_height == 0 {
        return Err("cover image has zero dimensions".into());
    }
    let destination_height = ((source_height as f64 * f64::from(THUMBNAIL_WIDTH) / source_width as f64).round() as u32).max(1);
    let source = Image::from_vec_u8(source_width, source_height, decoded.into_raw(), PixelType::U8x3)?;
    let mut destination = Image::new(THUMBNAIL_WIDTH, destination_height, PixelType::U8x3);
    Resizer::new().resize(&source, &mut destination, None)?;

    let mut encoded = BufWriter::new(Vec::new());
    JpegEncoder::new(&mut encoded).write_image(destination.buffer(), THUMBNAIL_WIDTH, destination_height, ColorType::Rgb8)?;
    Ok(encoded.into_inner().map_err(std::io::Error::other)?)
}

fn image_format(path: &str) -> Option<ImageFormat> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}
