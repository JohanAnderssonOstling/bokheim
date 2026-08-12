//! Pure cover extraction and JPEG thumbnail generation for supported books.

use epub_provider::{EpubProvider, ResourceProvider};
use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::{JpegDecoder, JpegEncoder};
use image::io::Reader as ImageReader;
use image::{ColorType, DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, RgbImage};
use mobi::headers::ExthRecord;
use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// Covers are displayed at `LARGE_BOOK_COVER_WIDTH_PX` *logical* pixels, so the
/// stored asset has to carry the device pixel ratio too: 200 logical at DPR 3
/// is 600 real pixels. Anything smaller is upscaled on HiDPI displays and on
/// phones.
///
/// Raising this does not touch thumbnails that already exist — publishing is
/// skipped whenever the file is present — so libraries scanned before the
/// change keep their old resolution until those books are rescanned.
const THUMBNAIL_WIDTH: u32 = 600;

pub type ThumbnailResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Extracts a publication cover and returns a resized JPEG. Publications with
/// no cover, and formats without an extractor, return `None`.
pub fn generate_thumbnail(book_path: &Path) -> ThumbnailResult<Option<Vec<u8>>> {
    let extension = book_path.extension().and_then(|extension| extension.to_str()).unwrap_or_default().to_ascii_lowercase();
    let image = match extension.as_str() {
        "epub" => epub_cover(book_path)?,
        "cbz" => cbz_cover(book_path)?,
        "m4b" => m4b_cover(book_path)?,
        "mobi" | "azw" | "azw3" => mobi_cover(book_path)?,
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
    Ok(Some(decode_cover_image(&bytes, format)?))
}

fn cbz_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let source = html_publication::PublicationSource::from_path(path.to_path_buf())?;
    let cover_resource = match &source.layout {
        html_publication::PublicationLayout::PagedImages { cover_resource, .. } => cover_resource.as_deref(),
        html_publication::PublicationLayout::Reflowable => None,
    };
    let Some(cover_resource) = cover_resource else { return Ok(None) };
    let bytes = source.provider.read_bytes(cover_resource)?;
    Ok(Some(decode_cover_image(&bytes, image::guess_format(&bytes)?)?))
}

fn m4b_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let tag = mp4ameta::Tag::read_from_path(path)?;
    let Some(artwork) = tag.artwork() else { return Ok(None) };
    Ok(Some(decode_cover_image(artwork.data, image::guess_format(artwork.data)?)?))
}

fn mobi_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    // Parsing only MobiMetadata avoids reading and decoding the book's text.
    // CoverOffset and ThumbOffset are relative to first_image_index.
    let metadata = mobi::MobiMetadata::from_path(path)?;
    let file_len = std::fs::metadata(path)?.len();
    let records = &metadata.records.records;
    let first_image_index = metadata.mobi.first_image_index as usize;
    let mut file = File::open(path)?;
    let mut attempted = Vec::with_capacity(2);

    // The embedded thumbnail is normally already close to display size and is
    // considerably cheaper to decode than the full-resolution cover.
    for record in [ExthRecord::ThumbOffset, ExthRecord::CoverOffset] {
        let Some(relative_index) = mobi_image_offset(&metadata, record) else { continue };
        let Some(record_index) = first_image_index.checked_add(relative_index) else { continue };
        attempted.push(record_index);
        if let Some(image) = decode_mobi_image_record(&mut file, records, record_index, file_len)? {
            return Ok(Some(image));
        }
    }

    // Older MOBI files may omit the EXTH cover fields. Images are stored from
    // first_image_index onward, and the first decodable image is their
    // conventional cover fallback.
    for record_index in first_image_index..records.len() {
        if attempted.contains(&record_index) {
            continue;
        }
        if let Some(image) = decode_mobi_image_record(&mut file, records, record_index, file_len)? {
            return Ok(Some(image));
        }
    }

    Ok(None)
}

fn mobi_image_offset(metadata: &mobi::MobiMetadata, record: ExthRecord) -> Option<usize> {
    let bytes: [u8; 4] = metadata.exth_record(record)?.first()?.as_slice().try_into().ok()?;
    usize::try_from(u32::from_be_bytes(bytes)).ok()
}

fn decode_mobi_image_record(file: &mut File, records: &[mobi::record::PdbRecord], record_index: usize, file_len: u64) -> ThumbnailResult<Option<RgbImage>> {
    let Some((start, len)) = mobi_record_bounds(records, record_index, file_len) else { return Ok(None) };
    let Ok(len) = usize::try_from(len) else { return Ok(None) };
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; len];
    file.read_exact(&mut bytes)?;
    let Ok(format) = image::guess_format(&bytes) else { return Ok(None) };
    match decode_cover_image(&bytes, format) {
        Ok(image) => Ok(Some(image)),
        // A malformed image record should not prevent trying the full cover or
        // another image record from the same publication.
        Err(_) => Ok(None),
    }
}

fn mobi_record_bounds(records: &[mobi::record::PdbRecord], record_index: usize, file_len: u64) -> Option<(u64, u64)> {
    let start = u64::from(records.get(record_index)?.offset);
    let end = records.get(record_index + 1).map_or(file_len, |record| u64::from(record.offset));
    (start < end && end <= file_len).then(|| (start, end - start))
}

#[cfg(feature = "pdf")]
fn pdf_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let rendered = pdf_reader_core::render_first_page_image(path, 300)?;
    let (rgba, pixel_width, pixel_height) = rendered.into_parts();
    let rgb = rgba.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect::<Vec<_>>();
    Ok(Some(RgbImage::from_raw(pixel_width, pixel_height, rgb).ok_or("PDF renderer returned invalid image dimensions")?))
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
    if source_width == THUMBNAIL_WIDTH {
        return encode_jpeg(decoded.as_raw(), source_width, source_height);
    }
    let source = Image::from_vec_u8(source_width, source_height, decoded.into_raw(), PixelType::U8x3)?;
    let mut destination = Image::new(THUMBNAIL_WIDTH, destination_height, PixelType::U8x3);
    Resizer::new().resize(&source, &mut destination, Some(&ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Hamming))))?;

    encode_jpeg(destination.buffer(), THUMBNAIL_WIDTH, destination_height)
}

fn encode_jpeg(rgb: &[u8], width: u32, height: u32) -> ThumbnailResult<Vec<u8>> {
    let mut encoded = BufWriter::new(Vec::new());
    JpegEncoder::new(&mut encoded).write_image(rgb, width, height, ColorType::Rgb8)?;
    Ok(encoded.into_inner().map_err(std::io::Error::other)?)
}

fn decode_cover_image(bytes: &[u8], format: ImageFormat) -> ThumbnailResult<RgbImage> {
    if format == ImageFormat::Jpeg {
        let mut decoder = JpegDecoder::new(Cursor::new(bytes))?;
        let (source_width, source_height) = decoder.dimensions();
        if source_width > THUMBNAIL_WIDTH {
            let requested_height = ((u64::from(source_height) * u64::from(THUMBNAIL_WIDTH)).div_ceil(u64::from(source_width))).clamp(1, u64::from(u16::MAX)) as u16;
            decoder.scale(THUMBNAIL_WIDTH as u16, requested_height)?;
        }
        return Ok(DynamicImage::from_decoder(decoder)?.to_rgb8());
    }
    Ok(ImageReader::with_format(Cursor::new(bytes), format).decode()?.to_rgb8())
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

#[cfg(test)]
mod tests {
    use super::*;
    use mobi::record::PdbRecord;

    #[test]
    fn reads_big_endian_mobi_image_offset() {
        let mut metadata = mobi::MobiMetadata::default();
        metadata.exth.records.insert(ExthRecord::ThumbOffset, vec![42_u32.to_be_bytes().to_vec()]);

        assert_eq!(mobi_image_offset(&metadata, ExthRecord::ThumbOffset), Some(42));
    }

    #[test]
    fn validates_mobi_record_bounds() {
        let records = vec![PdbRecord { id: 0, offset: 100 }, PdbRecord { id: 1, offset: 180 }];

        assert_eq!(mobi_record_bounds(&records, 0, 250), Some((100, 80)));
        assert_eq!(mobi_record_bounds(&records, 1, 250), Some((180, 70)));
        assert_eq!(mobi_record_bounds(&records, 2, 250), None);
        assert_eq!(mobi_record_bounds(&records, 1, 150), None);
    }
}
