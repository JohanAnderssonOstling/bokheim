//! Pure cover extraction and JPEG thumbnail generation for supported books.

#[cfg(feature = "publication-extractors")]
use epub_provider::EpubProvider;
use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::{JpegDecoder, JpegEncoder};
use image::io::Reader as ImageReader;
use image::{ColorType, DynamicImage, ImageDecoder, ImageEncoder, ImageFormat, RgbImage};
#[cfg(feature = "publication-extractors")]
use mobi::headers::ExthRecord;
#[cfg(feature = "publication-extractors")]
use std::fs::File;
use std::io::{BufWriter, Cursor};
#[cfg(feature = "publication-extractors")]
use std::io::{Read, Seek, SeekFrom};
#[cfg(feature = "publication-extractors")]
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
const MAX_COVER_IMAGE_BYTES: usize = 24 * 1024 * 1024;
const MAX_COVER_IMAGE_PIXELS: u64 = 24_000_000;
const MAX_COVER_IMAGE_DIMENSION: u32 = 20_000;
const EPUB_FALLBACK_MIN_WIDTH: u32 = 300;
const EPUB_FALLBACK_MIN_HEIGHT: u32 = 400;
const EPUB_FALLBACK_MIN_ASPECT_RATIO: f64 = 0.45;
const EPUB_FALLBACK_MAX_ASPECT_RATIO: f64 = 0.90;

pub type ThumbnailResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Extracts a publication cover and returns a resized JPEG. Publications with
/// no usable embedded cover, and formats without an extractor, return `None`.
#[cfg(feature = "publication-extractors")]
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

/// Normalizes an already-extracted cover image to the same resized JPEG used
/// by publication scanning. This keeps browser/WASM imports on the native
/// thumbnail contract without requiring a temporary filesystem path.
pub fn generate_thumbnail_from_image_bytes(bytes: &[u8]) -> ThumbnailResult<Vec<u8>> {
    let format = image::guess_format(bytes)?;
    encode_thumbnail(decode_cover_image(bytes, format)?)
}

#[cfg(feature = "publication-extractors")]
fn epub_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let epub = std::fs::File::open(path).and_then(EpubProvider::try_from_reader)?;
    if let Some(image_path) = epub.declared_cover_image_path()? {
        let format = image_format(&image_path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unsupported EPUB cover image format: {image_path}")))?;
        let bytes = epub.read_bytes(&image_path)?;
        let image = decode_cover_image(&bytes, format)?;
        if reasonable_cover_dimensions(image.width(), image.height()) {
            return Ok(Some(image));
        }
    }

    for image_path in epub.image_paths_in_reading_order()? {
        let Ok(bytes) = epub.read_bytes(&image_path) else { continue };
        let Ok(format) = image::guess_format(&bytes) else { continue };
        let Ok((width, height)) = ImageReader::with_format(Cursor::new(&bytes), format).into_dimensions() else { continue };
        if !reasonable_cover_dimensions(width, height) {
            continue;
        }
        if let Ok(image) = decode_cover_image(&bytes, format) {
            return Ok(Some(image));
        }
    }

    Ok(None)
}

/// Returns whether an image is large enough and sufficiently cover-shaped to
/// be preferred over another publication-cover source. Catalogue servers use
/// the same geometry contract for upstream thumbnails and EPUB fallbacks.
pub fn reasonable_cover_dimensions(width: u32, height: u32) -> bool {
    if width < EPUB_FALLBACK_MIN_WIDTH || height < EPUB_FALLBACK_MIN_HEIGHT {
        return false;
    }
    let aspect_ratio = f64::from(width) / f64::from(height);
    (EPUB_FALLBACK_MIN_ASPECT_RATIO..=EPUB_FALLBACK_MAX_ASPECT_RATIO).contains(&aspect_ratio)
}

#[cfg(feature = "publication-extractors")]
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

#[cfg(feature = "publication-extractors")]
fn m4b_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let tag = mp4ameta::Tag::read_from_path(path)?;
    let Some(artwork) = tag.artwork() else { return Ok(None) };
    Ok(Some(decode_cover_image(artwork.data, image::guess_format(artwork.data)?)?))
}

#[cfg(feature = "publication-extractors")]
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
            if reasonable_cover_dimensions(image.width(), image.height()) {
                return Ok(Some(image));
            }
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
            if reasonable_cover_dimensions(image.width(), image.height()) {
                return Ok(Some(image));
            }
        }
    }

    Ok(None)
}

#[cfg(feature = "publication-extractors")]
fn mobi_image_offset(metadata: &mobi::MobiMetadata, record: ExthRecord) -> Option<usize> {
    let bytes: [u8; 4] = metadata.exth_record(record)?.first()?.as_slice().try_into().ok()?;
    usize::try_from(u32::from_be_bytes(bytes)).ok()
}

#[cfg(feature = "publication-extractors")]
fn decode_mobi_image_record(file: &mut File, records: &[mobi::record::PdbRecord], record_index: usize, file_len: u64) -> ThumbnailResult<Option<RgbImage>> {
    let Some((start, len)) = mobi_record_bounds(records, record_index, file_len) else { return Ok(None) };
    if len > MAX_COVER_IMAGE_BYTES as u64 {
        return Ok(None);
    }
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

#[cfg(feature = "publication-extractors")]
fn mobi_record_bounds(records: &[mobi::record::PdbRecord], record_index: usize, file_len: u64) -> Option<(u64, u64)> {
    let start = u64::from(records.get(record_index)?.offset);
    let end = records.get(record_index + 1).map_or(file_len, |record| u64::from(record.offset));
    (start < end && end <= file_len).then(|| (start, end - start))
}

#[cfg(all(feature = "publication-extractors", feature = "pdf"))]
fn pdf_cover(path: &Path) -> ThumbnailResult<Option<RgbImage>> {
    let rendered = pdf_reader_core::render_first_page_image(path, 300)?;
    let (rgba, pixel_width, pixel_height) = rendered.into_parts();
    let rgb = rgba.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect::<Vec<_>>();
    Ok(Some(RgbImage::from_raw(pixel_width, pixel_height, rgb).ok_or("PDF renderer returned invalid image dimensions")?))
}

#[cfg(all(feature = "publication-extractors", not(feature = "pdf")))]
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

#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn embedded_bmp_cover_is_normalized_to_jpeg() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 3, image::Rgb([24, 96, 192])));
        let mut bmp = Cursor::new(Vec::new());
        source.write_to(&mut bmp, ImageFormat::Bmp).unwrap();

        let thumbnail = generate_thumbnail_from_image_bytes(bmp.get_ref()).unwrap();

        assert_eq!(image::guess_format(&thumbnail).unwrap(), ImageFormat::Jpeg);
        let decoded = image::load_from_memory(&thumbnail).unwrap();
        assert_eq!(decoded.to_rgb8().dimensions(), (THUMBNAIL_WIDTH, 900));
    }
}

fn encode_jpeg(rgb: &[u8], width: u32, height: u32) -> ThumbnailResult<Vec<u8>> {
    let mut encoded = BufWriter::new(Vec::new());
    JpegEncoder::new(&mut encoded).write_image(rgb, width, height, ColorType::Rgb8)?;
    Ok(encoded.into_inner().map_err(std::io::Error::other)?)
}

fn decode_cover_image(bytes: &[u8], format: ImageFormat) -> ThumbnailResult<RgbImage> {
    if bytes.len() > MAX_COVER_IMAGE_BYTES {
        return Err("cover image exceeds the encoded size limit".into());
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format).into_dimensions()?;
    validate_cover_image_dimensions(width, height)?;
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

fn validate_cover_image_dimensions(width: u32, height: u32) -> ThumbnailResult<()> {
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or("cover image dimensions overflow")?;
    if width == 0 || height == 0 || width > MAX_COVER_IMAGE_DIMENSION || height > MAX_COVER_IMAGE_DIMENSION || pixels > MAX_COVER_IMAGE_PIXELS {
        return Err("cover image exceeds the pixel limit".into());
    }
    Ok(())
}

#[cfg(feature = "publication-extractors")]
fn image_format(path: &str) -> Option<ImageFormat> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

#[cfg(all(test, feature = "publication-extractors"))]
mod publication_extractor_tests {
    use super::*;
    use mobi::record::PdbRecord;
    use std::io::Write;

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

    #[test]
    fn validates_epub_fallback_image_geometry() {
        assert!(reasonable_cover_dimensions(300, 400));
        assert!(reasonable_cover_dimensions(600, 900));
        assert!(!reasonable_cover_dimensions(299, 600));
        assert!(!reasonable_cover_dimensions(600, 399));
        assert!(!reasonable_cover_dimensions(300, 800));
        assert!(!reasonable_cover_dimensions(800, 600));
    }

    #[test]
    fn rejects_cover_images_with_pathological_decoded_dimensions() {
        assert!(validate_cover_image_dimensions(4_000, 6_000).is_ok());
        assert!(validate_cover_image_dimensions(4_001, 6_000).is_err());
        assert!(validate_cover_image_dimensions(1, MAX_COVER_IMAGE_DIMENSION + 1).is_err());
    }

    #[test]
    fn uses_first_reasonable_spine_image_when_cover_art_is_absent() {
        let file = tempfile::Builder::new().suffix(".epub").tempfile().unwrap();
        let mut archive = zip::ZipWriter::new(file.reopen().unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", deflated).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        archive.start_file("EPUB/package.opf", deflated).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Image fallback</dc:title></metadata><manifest><item id="unreferenced" href="images/unreferenced.jpg" media-type="image/jpeg"/><item id="page" href="page.xhtml" media-type="application/xhtml+xml"/><item id="tiny" href="images/tiny.jpg" media-type="image/jpeg"/><item id="candidate" href="images/illustration-1.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="page"/></spine></package>"#)
            .unwrap();
        archive.start_file("EPUB/page.xhtml", deflated).unwrap();
        archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="images/tiny.jpg"/><img src="images/illustration-1.jpg"/></body></html>"#).unwrap();
        archive.start_file("EPUB/images/tiny.jpg", deflated).unwrap();
        archive.write_all(&encode_jpeg(&vec![20; 64 * 64 * 3], 64, 64).unwrap()).unwrap();
        archive.start_file("EPUB/images/unreferenced.jpg", deflated).unwrap();
        let unreferenced_pixels = [20, 40, 190].into_iter().cycle().take(400 * 600 * 3).collect::<Vec<_>>();
        archive.write_all(&encode_jpeg(&unreferenced_pixels, 400, 600).unwrap()).unwrap();
        archive.start_file("EPUB/images/illustration-1.jpg", deflated).unwrap();
        let candidate_pixels = [190, 35, 25].into_iter().cycle().take(400 * 600 * 3).collect::<Vec<_>>();
        archive.write_all(&encode_jpeg(&candidate_pixels, 400, 600).unwrap()).unwrap();
        archive.finish().unwrap();

        let jpeg = generate_thumbnail(file.path()).unwrap().expect("reasonable image fallback");
        let rendered = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        assert_eq!(rendered.dimensions(), (THUMBNAIL_WIDTH, 900));
        let center = rendered.get_pixel(THUMBNAIL_WIDTH / 2, 450);
        assert!(center[0] > 170 && center[1] < 55 && center[2] < 45, "unexpected fallback pixel: {center:?}");
    }

}
