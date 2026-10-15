//! Pure cover extraction and JPEG thumbnail generation for supported books.

#[cfg(feature = "book-memory")]
use book_content::{Book, BookFormat};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::ImageReader;
use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, DynamicImage, ImageEncoder, ImageFormat, RgbImage};
use std::io::Error;
#[cfg(feature = "book-extractors")]
use std::io::SeekFrom;
use std::io::{BufWriter, Cursor};
use std::io::{Read, Seek};

/// Covers are displayed at `LARGE_BOOK_COVER_WIDTH_PX` *logical* pixels, so the
/// stored asset has to carry the device pixel ratio too: 200 logical at DPR 3
/// is 600 real pixels. Anything smaller is upscaled on HiDPI displays and on
/// phones.
///
/// The 600px asset remains the synchronized source of truth. The 300px browse
/// bucket is cached on the server and device, and can be rebuilt from it.
pub const BROWSE_THUMBNAIL_WIDTH: u32 = 300;
pub const THUMBNAIL_WIDTH: u32 = 600;
const MAX_COVER_IMAGE_BYTES: usize = 24 * 1024 * 1024;
const MAX_COVER_IMAGE_PIXELS: u64 = 24_000_000;
const MAX_COVER_IMAGE_DIMENSION: u32 = 20_000;
const EPUB_FALLBACK_MIN_WIDTH: u32 = 300;
const EPUB_FALLBACK_MIN_HEIGHT: u32 = 400;
const EPUB_FALLBACK_MIN_ASPECT_RATIO: f64 = 0.45;
const EPUB_FALLBACK_MAX_ASPECT_RATIO: f64 = 0.90;

pub type ThumbnailResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

mod card;
pub use card::{card_thumbnail_geometry, card_thumbnail_suffix, encode_card_thumbnail_at_width, CardThumbnailGeometry, CARD_MAX_HORIZONTAL_CROP, CARD_MAX_VERTICAL_STRETCH, CARD_TARGET_HEIGHT_RATIO, CARD_THUMBNAIL_VERSION};

/// An unmodified high-density master plus adjusted 300px and 600px cards.
/// Every derivative is produced independently from the decoded original.
pub struct ThumbnailVersions {
    pub browse: Vec<u8>,
    pub card_high_density: Vec<u8>,
    pub high_density: Vec<u8>,
}

impl ThumbnailVersions {
    pub fn from_image(image: &RgbImage) -> ThumbnailResult<Self> {
        Ok(Self { browse: encode_card_thumbnail_at_width(image, BROWSE_THUMBNAIL_WIDTH)?, card_high_density: encode_card_thumbnail_at_width(image, THUMBNAIL_WIDTH)?, high_density: encode_thumbnail_at_width(image, THUMBNAIL_WIDTH)? })
    }
}

/// Returns whether an image is large enough and sufficiently cover-shaped to
/// be preferred over another book-cover source. Providers and undeclared EPUB
/// fallbacks use this heuristic; declared artwork can have any valid shape.
pub fn reasonable_cover_dimensions(width: u32, height: u32) -> bool {
    if width < EPUB_FALLBACK_MIN_WIDTH || height < EPUB_FALLBACK_MIN_HEIGHT {
        return false;
    }
    let aspect_ratio = f64::from(width) / f64::from(height);
    (EPUB_FALLBACK_MIN_ASPECT_RATIO..=EPUB_FALLBACK_MAX_ASPECT_RATIO).contains(&aspect_ratio)
}

// The renderers provide pixels already composited onto their background.
fn rgb_from_rgba(pixels: &[u8], width: u32, height: u32) -> ThumbnailResult<RgbImage> {
    let expected = (u64::from(width) * u64::from(height)).checked_mul(4).ok_or("renderer dimensions overflow")?;
    if width == 0 || height == 0 || u64::try_from(pixels.len())? != expected {
        return Err("renderer returned invalid RGBA dimensions".into());
    }
    let rgb = pixels.chunks_exact(4).flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]).collect();
    RgbImage::from_raw(width, height, rgb).ok_or_else(|| "renderer returned invalid RGB dimensions".into())
}

pub fn render_pdf_cover(reader: impl Read + Seek + Send + 'static) -> ThumbnailResult<RgbImage> {
    let rendered = pdf_reader_core::render_first_page_image_from_reader(reader, THUMBNAIL_WIDTH as u16)?;
    rgb_from_rgba(rendered.rgba(), rendered.pixel_width(), rendered.pixel_height())
}

pub fn encode_thumbnail_at_width(decoded: &RgbImage, width: u32) -> ThumbnailResult<Vec<u8>> {
    let (source_width, source_height) = decoded.dimensions();
    if source_width == 0 || source_height == 0 || width == 0 {
        return Err("cover image has zero dimensions".into());
    }
    let destination_height = ((source_height as f64 * f64::from(width) / source_width as f64).round() as u32).max(1);
    validate_cover_image_dimensions(width, destination_height)?;
    if source_width == width {
        return encode_jpeg(decoded.as_raw(), source_width, source_height);
    }
    let source = ImageRef::new(source_width, source_height, decoded.as_raw(), PixelType::U8x3)?;
    let mut destination = Image::new(width, destination_height, PixelType::U8x3);
    Resizer::new().resize(&source, &mut destination, Some(&ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Hamming))))?;

    encode_jpeg(destination.buffer(), width, destination_height)
}

#[cfg(test)]
mod core_tests {
    use super::*;

    #[test]
    fn extreme_aspect_ratio_is_rejected_before_thumbnail_allocation() {
        let image = RgbImage::new(1, MAX_COVER_IMAGE_DIMENSION);
        assert!(encode_thumbnail_at_width(&image, THUMBNAIL_WIDTH).is_err());
    }

    #[test]
    fn embedded_bmp_cover_is_normalized_to_jpeg() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 3, image::Rgb([24, 96, 192])));
        let mut bmp = Cursor::new(Vec::new());
        source.write_to(&mut bmp, ImageFormat::Bmp).unwrap();

        let thumbnail = (image::guess_format(bmp.get_ref()).map_err(Into::into).and_then(|format| decode_cover_image(bmp.get_ref(), format)).and_then(|image| encode_thumbnail_at_width(&image, THUMBNAIL_WIDTH))).unwrap();

        assert_eq!(image::guess_format(&thumbnail).unwrap(), ImageFormat::Jpeg);
        let decoded = image::load_from_memory(&thumbnail).unwrap();
        assert_eq!(decoded.to_rgb8().dimensions(), (THUMBNAIL_WIDTH, 900));
    }
}

fn encode_jpeg(rgb: &[u8], width: u32, height: u32) -> ThumbnailResult<Vec<u8>> {
    let mut encoded = BufWriter::new(Vec::new());
    JpegEncoder::new(&mut encoded).write_image(rgb, width, height, ColorType::Rgb8.into())?;
    Ok(encoded.into_inner().map_err(Error::other)?)
}

pub fn decode_cover_image(bytes: &[u8], format: ImageFormat) -> ThumbnailResult<RgbImage> {
    if bytes.len() > MAX_COVER_IMAGE_BYTES {
        return Err("cover image exceeds the encoded size limit".into());
    }
    if format == ImageFormat::Jpeg {
        return (|bytes: &[u8]| -> ThumbnailResult<RgbImage> {
            use jpeg_decoder::PixelFormat;

            let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
            decoder.read_info()?;
            let info = decoder.info().ok_or("JPEG metadata missing after header parsing")?;
            let (width, height) = (u32::from(info.width), u32::from(info.height));
            validate_cover_image_dimensions(width, height)?;
            if width > THUMBNAIL_WIDTH {
                let requested_height = ((u64::from(height) * u64::from(THUMBNAIL_WIDTH)).div_ceil(u64::from(width))).clamp(1, u64::from(u16::MAX)) as u16;
                decoder.scale(THUMBNAIL_WIDTH as u16, requested_height)?;
            }
            let samples = decoder.decode()?;
            let info = decoder.info().ok_or("JPEG metadata missing after decoding")?;
            let (width, height) = (u32::from(info.width), u32::from(info.height));
            let rgb = match info.pixel_format {
                PixelFormat::RGB24 => samples,
                PixelFormat::L8 => samples.into_iter().flat_map(|value| [value; 3]).collect(),
                PixelFormat::L16 => {
                    let samples = samples.chunks_exact(2).map(|bytes| u16::from_ne_bytes([bytes[0], bytes[1]])).collect();
                    return Ok(DynamicImage::ImageLuma16(image::ImageBuffer::from_raw(width, height, samples).ok_or("invalid JPEG grayscale buffer")?).into_rgb8());
                }
                PixelFormat::CMYK32 => samples
                    .chunks_exact(4)
                    .flat_map(|pixel| {
                        let black = 255 - u16::from(pixel[3]);
                        [0, 1, 2].map(|channel| ((255 - u16::from(pixel[channel])) * black / 255) as u8)
                    })
                    .collect(),
            };
            Ok(RgbImage::from_raw(width, height, rgb).ok_or("invalid JPEG pixel buffer")?)
        })(bytes);
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format).into_dimensions()?;
    validate_cover_image_dimensions(width, height)?;
    let decoded = ImageReader::with_format(Cursor::new(bytes), format).decode()?;
    // Reduce RGBA/16-bit images before RGB conversion: converting the full
    // source would retain a second book-sized pixel buffer.
    let reduced = if width > THUMBNAIL_WIDTH {
        let height = (u64::from(height) * u64::from(THUMBNAIL_WIDTH)).div_ceil(u64::from(width)).max(1) as u32;
        decoded.thumbnail_exact(THUMBNAIL_WIDTH, height)
    } else {
        decoded
    };
    Ok(reduced.into_rgb8())
}

// image 0.25's JPEG decoder does not expose DCT scaling. Use the existing
// jpeg-decoder dependency to retain bounded thumbnail decoding and the same
// grayscale/CMYK conversion as our previous image 0.24 path.

fn validate_cover_image_dimensions(width: u32, height: u32) -> ThumbnailResult<()> {
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or("cover image dimensions overflow")?;
    if width == 0 || height == 0 || width > MAX_COVER_IMAGE_DIMENSION || height > MAX_COVER_IMAGE_DIMENSION || pixels > MAX_COVER_IMAGE_PIXELS {
        return Err("cover image exceeds the pixel limit".into());
    }
    Ok(())
}

/// Reads one cover resource and decodes it as SVG or a raster image.
#[cfg(feature = "book-extractors")]
fn decode_cover_resource(book: &mut Book, uri: &str) -> ThumbnailResult<RgbImage> {
    let bytes = book.read_resource_sync(uri, MAX_COVER_IMAGE_BYTES as u64)?;
    if uri.rsplit('.').next().is_some_and(|extension| extension.eq_ignore_ascii_case("svg")) {
        return decode_svg_cover(&bytes);
    }
    let format = image::guess_format(&bytes)?;
    decode_cover_image(&bytes, format)
}

/// Container parsing and record bounds belong to book-content. Thumbnail
/// selection owns geometry checks and image decoding for both formats.
#[cfg(feature = "book-extractors")]
fn book_cover(mut book: Book, format: BookFormat) -> ThumbnailResult<Option<RgbImage>> {
    let preferred = book.preferred_cover_resources()?;
    for uri in &preferred {
        match decode_cover_resource(&mut book, uri) {
            // Explicit artwork can be square, landscape or unusually tall.
            // Keep shape heuristics for undeclared reading-order fallbacks.
            // A tiny MOBI thumbnail still yields to its full cover record.
            Ok(image) if format == BookFormat::Epub || (image.width() >= EPUB_FALLBACK_MIN_WIDTH && image.height() >= EPUB_FALLBACK_MIN_HEIGHT) => return Ok(Some(image)),
            Err(error) if format == BookFormat::Epub || is_read_failure(&*error) => return Err(error),
            _ => {}
        }
    }
    for uri in book.fallback_cover_resources()?.into_iter().filter(|uri| !preferred.contains(uri)) {
        let Ok(bytes) = book.read_resource_sync(&uri, MAX_COVER_IMAGE_BYTES as u64) else { continue };
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

#[cfg(feature = "book-extractors")]
fn decode_svg_cover(bytes: &[u8]) -> ThumbnailResult<RgbImage> {
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &options)?;
    let size = tree.size();
    let source_width = size.width();
    let source_height = size.height();
    if !source_width.is_finite() || !source_height.is_finite() || source_width <= 0.0 || source_height <= 0.0 {
        return Err("SVG cover has invalid dimensions".into());
    }
    let aspect_ratio = f64::from(source_width) / f64::from(source_height);
    let destination_height = (f64::from(THUMBNAIL_WIDTH) / aspect_ratio).round().max(1.0);
    if destination_height > f64::from(MAX_COVER_IMAGE_DIMENSION) || u64::from(THUMBNAIL_WIDTH) * destination_height as u64 > MAX_COVER_IMAGE_PIXELS {
        return Err("SVG cover exceeds the pixel limit".into());
    }
    let destination_height = destination_height as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(THUMBNAIL_WIDTH, destination_height).ok_or("could not allocate SVG cover pixmap")?;
    // JPEG has no alpha channel. A white mat preserves the expected appearance
    // of transparent SVG book covers instead of turning transparent pixels black.
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    let scale = THUMBNAIL_WIDTH as f32 / source_width;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    rgb_from_rgba(pixmap.data(), THUMBNAIL_WIDTH, destination_height)
}

#[cfg(all(test, feature = "book-extractors"))]
mod book_extractor_tests {
    use super::*;
    use std::io::Write;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    #[test]
    fn declared_epub_covers_of_every_shape_use_generated_card_geometry() {
        for (height, browse_height, dense_height) in [(480, 309, 619), (600, 387, 774), (900, 480, 960), (1200, 480, 960)] {
            let mut png = Cursor::new(Vec::new());
            DynamicImage::ImageRgb8(RgbImage::from_pixel(600, height, image::Rgb([180, 40, 20]))).write_to(&mut png, ImageFormat::Png).unwrap();
            let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, bytes) in [
                ("mimetype", b"application/epub+zip".as_slice()),
                ("META-INF/container.xml", br#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.as_slice()),
                ("book.opf", br#"<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">cover-test</dc:identifier><dc:title>Declared cover</dc:title><dc:language>en</dc:language></metadata><manifest><item id="art" href="art.png" media-type="image/png" properties="cover-image"/><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#.as_slice()),
                ("chapter.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter</title></head><body><p>Book content.</p></body></html>"#.as_slice()),
                ("art.png", png.get_ref().as_slice()),
            ] {
                archive.start_file(name, stored).unwrap();
                archive.write_all(bytes).unwrap();
            }
            let versions = generate_thumbnail_versions_with_pdf_renderer("epub", archive.finish().unwrap(), |_| panic!("EPUB should not invoke PDF rendering")).unwrap().expect("declared cover must be retained regardless of shape");
            for (bytes, expected) in [(&versions.browse, (300, browse_height)), (&versions.card_high_density, (600, dense_height)), (&versions.high_density, (600, height))] {
                assert_eq!(image::load_from_memory(bytes).unwrap().to_rgb8().dimensions(), expected);
            }
        }
    }

    #[test]
    fn decodes_supported_cover_formats_after_image_upgrade() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(400, 600, image::Rgb([180, 40, 20])));
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Gif, ImageFormat::WebP, ImageFormat::Bmp, ImageFormat::Tiff] {
            let mut encoded = Cursor::new(Vec::new());
            source.write_to(&mut encoded, format).unwrap();
            let decoded = decode_cover_image(encoded.get_ref(), format).unwrap();
            assert_eq!(decoded.dimensions(), (400, 600), "{format:?}");
            for (actual, expected) in decoded.get_pixel(0, 0).0.into_iter().zip([180, 40, 20]) {
                assert!(actual.abs_diff(expected) <= 3, "{format:?}");
            }
        }
    }

    #[test]
    fn reduces_large_png_before_rgb_conversion_and_preserves_both_cover_sizes() {
        for source in [
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1200, 1800, image::Rgb([180, 40, 20]))),
            DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1200, 1800, image::Rgba([180, 40, 20, 255]))),
            DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(1200, 1800, image::Rgba([46260_u16, 10280, 5140, 65535]))),
        ] {
            let mut png = Cursor::new(Vec::new());
            source.write_to(&mut png, ImageFormat::Png).unwrap();
            let cover = decode_cover_image(png.get_ref(), ImageFormat::Png).unwrap();
            assert_eq!(cover.dimensions(), (600, 900));
            assert_eq!(cover.get_pixel(300, 450).0, [180, 40, 20]);
            let versions = ThumbnailVersions::from_image(&cover).unwrap();
            assert_eq!(image::load_from_memory(&versions.browse).unwrap().width(), 300);
            let high = image::load_from_memory(&versions.high_density).unwrap();
            assert_eq!((high.width(), high.height()), (600, 900));
        }
    }
}

mod pdf_cover;

/// The book's cover, or none. A book whose bytes cannot be read as a cover
/// has none: reading it again gives the same result. Only an I/O failure
/// that says nothing about the bytes is an error, for the caller to retry.
#[cfg(feature = "book-extractors")]
pub fn generate_thumbnail_versions_with_pdf_renderer<R: Read + Seek + Send + Sync + 'static>(extension: &str, reader: R, render: impl FnOnce(R) -> ThumbnailResult<RgbImage>) -> ThumbnailResult<Option<ThumbnailVersions>> {
    match cover_versions(extension, reader, render) {
        Err(error) if !is_read_failure(&*error) => {
            log::warn!("{extension} book has no usable cover: {error}");
            Ok(None)
        }
        result => result,
    }
}

/// An I/O failure while reading the book, as opposed to bytes that are not
/// a usable cover.
#[cfg(feature = "book-extractors")]
fn is_read_failure(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.downcast_ref::<std::io::Error>().is_some_and(|error| !matches!(error.kind(), std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput | std::io::ErrorKind::UnexpectedEof))
}

/// Extract simple opaque PDF image covers directly; render complex pages with PDFium.
/// Resizing/encoding consumes owned pixels after PDFium finishes rendering.
#[cfg(feature = "book-extractors")]
fn cover_versions<R: Read + Seek + Send + Sync + 'static>(extension: &str, mut reader: R, render: impl FnOnce(R) -> ThumbnailResult<RgbImage>) -> ThumbnailResult<Option<ThumbnailVersions>> {
    let cover = match extension {
        "epub" | "mobi" | "azw" | "azw3" => {
            let format = if extension == "epub" { BookFormat::Epub } else { BookFormat::Mobi };
            book_cover(Book::open_reader(format, reader)?, format)?
        }
        "m4b" => {
            let tag = mp4ameta::Tag::read_from(&mut reader)?;
            tag.artwork().map(|artwork| decode_cover_image(artwork.data, image::guess_format(artwork.data)?)).transpose()?
        }
        "mp3folder" => {
            let mut archive = zip::ZipArchive::new(reader)?;
            let mut cover = None;
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index)?;
                let name = entry.name().to_ascii_lowercase();
                if ["cover.jpg", "cover.jpeg", "cover.png", "cover.webp", "folder.jpg", "folder.jpeg", "folder.png", "folder.webp"].contains(&name.as_str()) {
                    if entry.size() > audiobook_archive::MAX_COVER_BYTES {
                        return Err("audiobook cover exceeds the byte limit".into());
                    }
                    let mut bytes = Vec::new();
                    entry.read_to_end(&mut bytes)?;
                    cover = image::guess_format(&bytes).ok().and_then(|format| decode_cover_image(&bytes, format).ok());
                    break;
                }
            }
            if cover.is_none() && archive.len() != 0 {
                #[cfg(not(target_arch = "wasm32"))]
                let facts = {
                    let mut audio = tempfile::tempfile()?;
                    std::io::copy(&mut archive.by_index(0)?, &mut audio)?;
                    audio.seek(SeekFrom::Start(0))?;
                    audiobook_folder::inspect_mp3(audio, true).ok()
                };
                #[cfg(target_arch = "wasm32")]
                let facts = {
                    let mut audio = Vec::new();
                    archive.by_index(0)?.read_to_end(&mut audio)?;
                    audiobook_folder::inspect_mp3(std::io::Cursor::new(audio), true).ok()
                };
                if let Some(bytes) = facts.and_then(|facts| facts.artwork) {
                    cover = image::guess_format(&bytes).ok().and_then(|format| decode_cover_image(&bytes, format).ok());
                }
            }
            cover
        }
        "pdf" => Some(match pdf_cover::extract(&mut reader) {
            Some(image) => image,
            None => {
                reader.seek(SeekFrom::Start(0))?;
                render(reader)?
            }
        }),
        _ => None,
    };
    cover.as_ref().map(ThumbnailVersions::from_image).transpose()
}

#[cfg(all(test, feature = "book-extractors"))]
mod audiobook_sidecar_tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn mp3_folder_cover_is_used_for_thumbnails() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(600, 900).write_to(&mut png, ImageFormat::Png).unwrap();
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("01.mp3", stored).unwrap();
        archive.write_all(b"audio").unwrap();
        archive.start_file("cover.png", stored).unwrap();
        archive.write_all(png.get_ref()).unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        assert!(generate_thumbnail_versions_with_pdf_renderer("mp3folder", std::io::Cursor::new(bytes), render_pdf_cover).unwrap().is_some());
    }

    #[test]
    fn mp3_embedded_art_is_used_when_folder_has_no_cover_file() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(600, 900).write_to(&mut png, ImageFormat::Png).unwrap();
        let mut picture = vec![0];
        picture.extend_from_slice(b"image/png\0");
        picture.extend_from_slice(&[3, 0]);
        picture.extend_from_slice(png.get_ref());
        let mut frame = Vec::new();
        frame.extend_from_slice(b"APIC");
        frame.extend_from_slice(&(picture.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(&picture);
        let size = frame.len() as u32;
        let mut mp3 = vec![b'I', b'D', b'3', 3, 0, 0, ((size >> 21) & 0x7f) as u8, ((size >> 14) & 0x7f) as u8, ((size >> 7) & 0x7f) as u8, (size & 0x7f) as u8];
        mp3.extend(frame);
        mp3.extend_from_slice(include_bytes!("../../book-inspection/tests/fixtures/silence.mp3"));
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("01.mp3", stored).unwrap();
        archive.write_all(&mp3).unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        assert!(generate_thumbnail_versions_with_pdf_renderer("mp3folder", std::io::Cursor::new(bytes), render_pdf_cover).unwrap().is_some());
    }
}
