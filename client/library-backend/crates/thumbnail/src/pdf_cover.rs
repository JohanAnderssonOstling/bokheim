//! Extract a sole opaque image from image-only content with right-angle placement.
//! Inline images, nested forms, text, paths, graphics settings and transparency use PDFium.
//! Unknown drawing operations always fall back to rendering.
use super::*;
use lopdf::{Dictionary, Document, Object, Stream};

mod color;

type Matrix = [f64; 6];
type Rect = [f64; 4];
const IDENTITY: Matrix = [1., 0., 0., 1., 0., 0.];
const CONTENT_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
struct State {
    matrix: Matrix,
    clip: Rect,
}

pub(super) fn extract(reader: &mut (impl Read + Seek)) -> Option<RgbImage> {
    let document = &pdf_range_reader::cover(reader).ok()?;
    let page = document.objects.values().find_map(|object| {
        let dictionary = object.as_dict().ok()?;
        (dictionary.get(b"BokheimInspectionPage").and_then(Object::as_i64).ok() == Some(1)).then_some(dictionary)
    })?;
    let media = rectangle(inherited(document, page, b"MediaBox")?)?;
    let clip = match inherited(document, page, b"CropBox") {
        Some(bounds) => intersect(media, rectangle(bounds)?)?,
        None => media,
    };
    let rotation = match inherited(document, page, b"Rotate") {
        Some(value) => value.as_i64().ok()?.rem_euclid(360),
        None => 0,
    };
    if rotation % 90 != 0 {
        return None;
    }
    let resources = inherited(document, page, b"Resources")?.as_dict().ok()?;
    if page.has(b"Group") {
        return None;
    }

    // Find the sole image and the graphics state it is drawn with.
    let mut state = State { matrix: IDENTITY, clip };
    let mut remaining = CONTENT_LIMIT;
    let mut candidate: Option<(&Stream, State)> = None;
    let contents = resolve(document, page.get(b"Contents").ok()?)?;
    let mut bytes = Vec::new();
    let objects = match contents {
        Object::Array(items) => items.iter().collect::<Vec<_>>(),
        object => vec![object],
    };
    for object in objects {
        let stream = resolve(document, object)?.as_stream().ok()?;
        let content = stream.get_plain_content_with_limit(remaining).ok()?;
        remaining = remaining.checked_sub(content.len().checked_add(1)?)?;
        bytes.extend(content);
        bytes.push(b'\n');
    }
    let mut stack = Vec::new();
    // Strict parsing rejects trailing malformed data instead of accepting a partial page.
    for operation in lopdf::content::Content::decode_strict(&bytes).ok()?.operations {
        let args = &operation.operands;
        match operation.operator.as_str() {
            "q" if args.is_empty() => {
                if stack.len() >= 32 {
                    return None;
                }
                stack.push(state.clone());
            }
            "Q" if args.is_empty() => state = stack.pop()?,
            "cm" if args.len() == 6 => {
                let [a, b, c, d, e, f] = state.matrix;
                let [aa, bb, cc, dd, ee, ff]: Matrix = numbers(args)?.try_into().ok()?;
                let matrix = [a * aa + c * bb, b * aa + d * bb, a * cc + c * dd, b * cc + d * dd, a * ee + c * ff + e, b * ee + d * ff + f];
                if !matrix.iter().all(|v| v.is_finite()) {
                    return None;
                }
                state.matrix = matrix;
            }
            "Do" if args.len() == 1 && candidate.is_none() => {
                let objects = resolve(document, resources.get(b"XObject").ok()?)?.as_dict().ok()?;
                let object = objects.get(args[0].as_name().ok()?).ok()?;
                let stream = resolve(document, object)?.as_stream().ok()?;
                if stream.dict.get(b"Subtype").and_then(Object::as_name).ok()? != b"Image" || stream.dict.has(b"OC") {
                    return None;
                }
                candidate = Some((stream, state.clone()));
            }
            _ => return None,
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let (stream, placed) = candidate?;

    // Decode the image to RGB.
    let dict = &stream.dict;
    if dict.get(b"Subtype").and_then(Object::as_name).ok()? != b"Image" {
        return None;
    }
    // Preblended matte colours and standalone stencils need separate handling.
    if [b"Matte".as_slice(), b"Mask", b"SMask", b"SMaskInData"].iter().any(|key| dict.has(key)) {
        return None;
    }
    if let Ok(value) = dict.get(b"ImageMask") {
        if value.as_bool().ok()? {
            return None;
        }
    }
    let width = u32::try_from(dict.get(b"Width").ok()?.as_i64().ok()?).ok()?;
    let height = u32::try_from(dict.get(b"Height").ok()?.as_i64().ok()?).ok()?;
    validate_cover_image_dimensions(width, height).ok()?;
    // Peel ordinary PDF filters in order, retaining the final image codec and its
    // own DecodeParms. A parameter array belongs to individual filters, not all of them.
    let (encoded, codec, codec_parameters) = 'payload: {
        let filters = match stream.dict.get(b"Filter") {
            Ok(value) => match resolve(document, value)? {
                Object::Array(values) => values.iter().map(|value| resolve(document, value)?.as_name().ok()).collect::<Option<Vec<_>>>()?,
                value => vec![value.as_name().ok()?],
            },
            Err(_) => Vec::new(),
        };
        if filters.len() > 8 {
            return None;
        }
        let decode_parms = match stream.dict.get(b"DecodeParms") {
            Ok(value) => Some(resolve(document, value)?),
            Err(_) => None,
        };
        let mut bytes = stream.content.clone();
        let limit = usize::try_from(MAX_COVER_IMAGE_PIXELS).ok()?.checked_mul(4)?.checked_add(MAX_COVER_IMAGE_DIMENSION as usize)?;
        if bytes.len() > limit {
            return None;
        }
        for (index, filter) in filters.iter().enumerate() {
            let parameter = match decode_parms {
                Some(Object::Array(values)) if values.len() == filters.len() => resolve(document, &values[index])?,
                Some(Object::Array(_)) => return None,
                Some(value) if filters.len() == 1 => value,
                None => &Object::Null,
                _ => return None,
            };
            let parameters = match parameter {
                Object::Null => Dictionary::new(),
                value => value.as_dict().ok()?.clone(),
            };
            if matches!(*filter, b"DCTDecode" | b"JPXDecode") {
                if index + 1 != filters.len() || bytes.len() > MAX_COVER_IMAGE_BYTES {
                    return None;
                }
                break 'payload (bytes, filter.to_vec(), parameters);
            }
            if !matches!(*filter, b"FlateDecode" | b"LZWDecode" | b"ASCII85Decode") {
                return None;
            }
            let mut dictionary = Dictionary::new();
            dictionary.set("Filter", Object::Name(filter.to_vec()));
            dictionary.set("DecodeParms", parameters);
            bytes = Stream::new(dictionary, bytes).get_plain_content_with_limit(limit).ok()?;
        }
        (bytes, Vec::new(), Dictionary::new())
    };
    let jpx = codec == b"JPXDecode";
    let space;
    let mut samples;
    let bits = if jpx { 8 } else { dict.get(b"BitsPerComponent").and_then(Object::as_i64).ok()? };
    if jpx {
        let image = hayro_jpeg2000::Image::new(&encoded, &Default::default()).ok()?;
        if image.has_alpha() || (image.width(), image.height()) != (width, height) {
            return None;
        }
        let embedded = match image.color_space() {
            hayro_jpeg2000::ColorSpace::Gray => color::Space::Device(1),
            hayro_jpeg2000::ColorSpace::RGB => color::Space::Device(3),
            hayro_jpeg2000::ColorSpace::CMYK => color::Space::Device(4),
            hayro_jpeg2000::ColorSpace::Icc { profile, num_channels } => color::Space::icc(profile, usize::from(*num_channels))?,
            hayro_jpeg2000::ColorSpace::Unknown { .. } => return None,
        };
        let mut context = hayro_jpeg2000::DecoderContext::default();
        let decoded = image.decode(&mut context).ok()?;
        let data = decoded.data_u8();
        let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        if data.len() != pixels.checked_mul(embedded.channels() + usize::from(image.has_alpha()))? {
            return None;
        }
        // A PDF ColorSpace overrides the colour space embedded in JP2.
        space = match dict.get(b"ColorSpace") {
            Ok(value) => color::resolve_space(document, resources, value)?,
            Err(_) => embedded,
        };
        samples = data;
    } else {
        space = color::resolve_space(document, resources, dict.get(b"ColorSpace").ok()?)?;
        let channels = space.channels();
        if !matches!(bits, 1 | 2 | 4 | 8 | 16) {
            return None;
        }
        samples = match codec.as_slice() {
            b"DCTDecode" if bits == 8 => {
                let limit = usize::try_from(u64::from(width) * u64::from(height)).ok()?.checked_mul(channels)?;
                let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(&encoded));
                decoder.set_max_decoding_buffer_size(limit);
                decoder.read_info().ok()?;
                let info = decoder.info()?;
                if (u32::from(info.width), u32::from(info.height), info.pixel_format.pixel_bytes()) != (width, height, channels) {
                    return None;
                }
                if let Ok(transform) = codec_parameters.get(b"ColorTransform") {
                    let transform = match (transform.as_i64().ok()?, channels) {
                        (0, 3) => jpeg_decoder::ColorTransform::RGB,
                        (0, 4) => jpeg_decoder::ColorTransform::CMYK,
                        (1, 3) => jpeg_decoder::ColorTransform::YCbCr,
                        (1, 4) => jpeg_decoder::ColorTransform::YCCK,
                        _ => return None,
                    };
                    decoder.set_color_transform(transform);
                }
                let mut samples = decoder.decode().ok()?;
                // Undo jpeg-decoder's CMYK polarity normalization before PDF /Decode.
                if channels == 4 {
                    for sample in &mut samples {
                        *sample = 255 - *sample;
                    }
                }
                samples
            }
            b"" => unpack(&encoded, width, height, channels, bits)?,
            _ => return None,
        };
    }
    let channels = space.channels();
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if samples.len() != pixels.checked_mul(channels)? {
        return None;
    }
    // ISO 32000: /Decode is ignored for JPXDecode, whose samples are normalized
    // by the JPEG2000 decoder. All other codecs produce PDF sample values.
    if !jpx {
        let mapping = match dict.get(b"Decode") {
            Ok(value) => Some(numbers(resolve(document, value)?.as_array().ok()?)?),
            Err(_) => None,
        };
        let indexed = matches!(space, color::Space::Indexed(_));
        if (indexed && bits > 8) || mapping.as_ref().is_some_and(|values| values.len() != channels * 2) {
            return None;
        }
        if indexed || mapping.is_some() {
            let default_max = if indexed { ((1_u32 << bits) - 1) as f64 } else { 1. };
            let scale = if indexed { 1. } else { 255. };
            for pixel in samples.chunks_exact_mut(channels) {
                for (i, sample) in pixel.iter_mut().enumerate() {
                    let (min, max) = mapping.as_deref().map_or((0., default_max), |values| (values[2 * i], values[2 * i + 1]));
                    let value = min + f64::from(*sample) / 255. * (max - min);
                    *sample = (value * scale).round().clamp(0., 255.) as u8;
                }
            }
        }
    }
    let mut image = space.rgb(samples, width, height)?;

    // Place the image as drawn: mirror, rotate, crop to the clip and scale.
    let [a, b, c, d, e, f] = placed.matrix;
    // Do not silently approximate skew as rotation.
    if !((b == 0. && c == 0. && a != 0. && d != 0.) || (a == 0. && d == 0. && b != 0. && c != 0.)) {
        return None;
    }
    if b == 0. && c == 0. {
        if a < 0. {
            image::imageops::flip_horizontal_in_place(&mut image);
        }
        if d < 0. {
            image::imageops::flip_vertical_in_place(&mut image);
        }
    } else {
        image = image::imageops::rotate90(&image);
        if c < 0. {
            image::imageops::flip_horizontal_in_place(&mut image);
        }
        if b > 0. {
            image::imageops::flip_vertical_in_place(&mut image);
        }
    }
    let corners = [(0., 0.), (0., 1.), (1., 0.), (1., 1.)].map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
    let bounds = [corners.iter().map(|p| p.0).reduce(f64::min)?, corners.iter().map(|p| p.1).reduce(f64::min)?, corners.iter().map(|p| p.0).reduce(f64::max)?, corners.iter().map(|p| p.1).reduce(f64::max)?];
    if !bounds.iter().all(|v| v.is_finite()) {
        return None;
    }
    let visible = intersect(bounds, placed.clip)?;
    let sx = f64::from(image.width()) / (bounds[2] - bounds[0]);
    let sy = f64::from(image.height()) / (bounds[3] - bounds[1]);
    // PDF coordinates start at the bottom; decoded raster rows start at the top.
    let x0 = ((visible[0] - bounds[0]) * sx).round().clamp(0., f64::from(image.width())) as u32;
    let x1 = ((visible[2] - bounds[0]) * sx).round().clamp(0., f64::from(image.width())) as u32;
    let y0 = ((bounds[3] - visible[3]) * sy).round().clamp(0., f64::from(image.height())) as u32;
    let y1 = ((bounds[3] - visible[1]) * sy).round().clamp(0., f64::from(image.height())) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    image = image::imageops::crop_imm(&image, x0, y0, x1 - x0, y1 - y0).to_image();
    let width = image.width().min(THUMBNAIL_WIDTH);
    let height = (f64::from(width) * (visible[3] - visible[1]) / (visible[2] - visible[0])).round();
    if !height.is_finite() || height < 1. || height > f64::from(MAX_COVER_IMAGE_DIMENSION) {
        return None;
    }
    let height = height as u32;
    validate_cover_image_dimensions(width, height).ok()?;
    if image.dimensions() != (width, height) {
        image = image::imageops::resize(&image, width, height, image::imageops::FilterType::Lanczos3);
    }
    Some(match rotation {
        90 => image::imageops::rotate90(&image),
        180 => image::imageops::rotate180(&image),
        270 => image::imageops::rotate270(&image),
        _ => image,
    })
}

fn rectangle(object: &Object) -> Option<Rect> {
    let rect: Rect = numbers(object.as_array().ok()?)?.try_into().ok()?;
    (rect[2] > rect[0] && rect[3] > rect[1]).then_some(rect)
}
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (r[2] > r[0] && r[3] > r[1]).then_some(r)
}
fn resolve<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Object> {
    document.dereference(object).ok().map(|(_, object)| object)
}
fn inherited<'a>(document: &'a Document, mut page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..256 {
        if let Ok(value) = page.get(key) {
            return resolve(document, value);
        }
        page = resolve(document, page.get(b"Parent").ok()?)?.as_dict().ok()?;
    }
    None
}
fn numbers(objects: &[Object]) -> Option<Vec<f64>> {
    objects
        .iter()
        .map(|object| {
            let number = match object {
                Object::Integer(value) => *value as f64,
                Object::Real(value) => f64::from(*value),
                _ => return None,
            };
            number.is_finite().then_some(number)
        })
        .collect()
}

// Packed samples restart on each byte-aligned image row.
fn unpack(bytes: &[u8], width: u32, height: u32, channels: usize, bits: i64) -> Option<Vec<u8>> {
    let bits = usize::try_from(bits).ok()?;
    if !matches!(bits, 1 | 2 | 4 | 8 | 16) {
        return None;
    }
    let count = (width as usize).checked_mul(channels)?;
    let stride = count.checked_mul(bits)?.div_ceil(8);
    if bytes.len() != stride.checked_mul(height as usize)? {
        return None;
    }
    let mut output = Vec::with_capacity(count.checked_mul(height as usize)?);
    for row in bytes.chunks_exact(stride) {
        for i in 0..count {
            let value = if bits == 16 { u32::from(u16::from_be_bytes([row[i * 2], row[i * 2 + 1]])) } else { u32::from((row[i * bits / 8] >> (8 - bits - (i * bits % 8))) & ((1_u16 << bits) - 1) as u8) };
            output.push(((value * 255 + ((1_u32 << bits) - 1) / 2) / ((1_u32 << bits) - 1)) as u8);
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn fixture(content: &str, rotation: i64) -> Document {
        let mut document = Document::with_version("1.5");
        let pages = document.new_object_id();
        let image = document.add_object(Stream::new(
            dictionary! {
                "Type"=>"XObject", "Subtype"=>"Image", "Width"=>2, "Height"=>3,
                "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8,
            },
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 255, 0, 255, 0, 255, 255],
        ));
        let content = document.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
        let page = document.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>pages, "MediaBox"=>vec![0.into(),0.into(),2.into(),3.into()],
            "Resources"=>dictionary!{"XObject"=>dictionary!{"Cover"=>image}}, "Contents"=>content,
        });
        document.objects.insert(pages, dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1,"Rotate"=>rotation}.into());
        let root = document.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        document.trailer.set("Root", root);
        document
    }
    fn run(mut document: Document) -> Option<RgbImage> {
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        extract(&mut Cursor::new(bytes))
    }
    fn image_mut(document: &mut Document) -> &mut Stream {
        document.objects.values_mut().find_map(|object| object.as_stream_mut().ok().filter(|stream| stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image"))).unwrap()
    }
    const DRAW: &str = "q 2 0 0 3 0 0 cm /Cover Do Q";

    #[test]
    fn packed_samples_restart_each_row_and_keep_sixteen_bit_endpoints() {
        assert_eq!(unpack(&[0b00011000, 0b11100100], 3, 2, 1, 2).unwrap(), [0, 85, 170, 255, 170, 85]);
        assert_eq!(unpack(&[0x08, 0xf0], 3, 1, 1, 4).unwrap(), [0, 136, 255]);
        assert_eq!(unpack(&[0, 0, 0x80, 0, 255, 255], 3, 1, 1, 16).unwrap(), [0, 128, 255]);
        assert!(unpack(&[0], 3, 2, 1, 4).is_none());
    }
    #[test]
    fn palette_decode_mapping_preserves_colours_and_masks_require_rendering() {
        let mut document = fixture(DRAW, 0);
        let image = image_mut(&mut document);
        image.dict.set("ColorSpace", vec![Object::Name(b"Indexed".to_vec()), Object::Name(b"DeviceRGB".to_vec()), 1.into(), Object::String(vec![255, 0, 0, 0, 0, 255], lopdf::StringFormat::Hexadecimal)]);
        image.dict.set("BitsPerComponent", 1);
        image.set_content(vec![0x40, 0x80, 0x40]);
        image.dict.set("Mask", vec![0.into(), 0.into()]);
        assert!(run(document.clone()).is_none());
        let image = image_mut(&mut document);
        image.dict.remove(b"Mask");
        image.dict.set("Decode", vec![1.into(), 0.into()]);
        let cover = run(document).unwrap();
        assert_eq!(cover.get_pixel(0, 0).0, [0, 0, 255]);
        assert_eq!(cover.get_pixel(1, 0).0, [255, 0, 0]);
    }
    #[test]
    fn explicit_masks_require_rendering() {
        let mut document = fixture(DRAW, 0);
        let mask = document.add_object(Stream::new(dictionary! {"Subtype"=>"Image","Width"=>2,"Height"=>3,"ImageMask"=>true,"BitsPerComponent"=>1}, vec![0x40; 3]));
        image_mut(&mut document).dict.set("Mask", mask);
        assert!(run(document).is_none());
    }
    #[test]
    fn marked_content_with_comments_requires_rendering() {
        let content = format!("/Figure << /Example [1 % comment before closing array\n] % comment before closing dictionary\n>> BDC {DRAW} EMC");
        assert!(run(fixture(&content, 0)).is_none());
    }

    #[test]
    fn inline_images_and_truncated_content_require_rendering() {
        for content in ["q 2 0 0 3 0 0 cm BI /W 2 /H 3 /CS /G /BPC 8 ID  EI BI EI Q".to_owned(), format!("{DRAW} BI /W 1 /H 1 /CS /G /BPC 8 ID x EI"), format!("{DRAW} BI /W 1"), format!("{DRAW} (unterminated")] {
            assert!(run(fixture(&content, 0)).is_none());
        }
    }
    #[test]
    fn image_tiles_require_rendering_even_when_gapless() {
        for content in ["q 1 0 0 3 0 0 cm /Cover Do Q q 1 0 0 3 1 0 cm /Cover Do Q", "q 0.9 0 0 3 0 0 cm /Cover Do Q q 1 0 0 3 1 0 cm /Cover Do Q"] {
            assert!(run(fixture(content, 0)).is_none());
        }
    }
    #[test]
    fn calibrated_spaces_require_rendering_including_palette_bases_and_defaults() {
        for name in ["CalGray", "CalRGB", "Lab"] {
            let space = Object::Array(vec![Object::Name(name.as_bytes().to_vec()), dictionary! {"WhitePoint"=>vec![0.9642.into(),1.into(),0.8249.into()]}.into()]);
            let mut document = fixture(DRAW, 0);
            image_mut(&mut document).dict.set("ColorSpace", space.clone());
            assert!(run(document).is_none(), "{name}");
            let indexed = Object::Array(vec![Object::Name(b"Indexed".to_vec()), space.clone(), 0.into(), Object::String(vec![0; 3], lopdf::StringFormat::Hexadecimal)]);
            assert!(color::resolve_space(&Document::new(), &Dictionary::new(), &indexed).is_none());
            let resources = dictionary! {"ColorSpace"=>dictionary! {"DefaultRGB"=>space}};
            assert!(color::resolve_space(&Document::new(), &resources, &Object::Name(b"DeviceRGB".to_vec())).is_none());
        }
    }
    #[test]
    fn small_image_margins_and_inherited_page_rotations_preserve_pixels() {
        let original = run(fixture(DRAW, 0)).unwrap();
        assert_eq!(original.dimensions(), (2, 3));
        for (rotation, expected) in [(90, image::imageops::rotate90(&original)), (180, image::imageops::rotate180(&original)), (270, image::imageops::rotate270(&original)), (-90, image::imageops::rotate270(&original))] {
            assert_eq!(run(fixture(DRAW, rotation)).unwrap(), expected);
        }
        let mut document = fixture(DRAW, 0);
        let id = *document.get_pages().get(&1).unwrap();
        document.get_object_mut(id).unwrap().as_dict_mut().unwrap().set("MediaBox", vec![(-1).into(), (-1).into(), 3.into(), 4.into()]);
        assert_eq!(run(document).unwrap(), original);
    }
    #[test]
    fn image_mirroring_and_right_angle_placement_preserve_pixels() {
        let original = run(fixture(DRAW, 0)).unwrap();
        assert_eq!(run(fixture("q -2 0 0 3 2 0 cm /Cover Do Q", 0)).unwrap(), image::imageops::flip_horizontal(&original));
        assert_eq!(run(fixture("q 2 0 0 -3 0 3 cm /Cover Do Q", 0)).unwrap(), image::imageops::flip_vertical(&original));
        for (matrix, expected) in [("0 -2 3 0 0 2", image::imageops::rotate90(&original)), ("0 2 -3 0 3 0", image::imageops::rotate270(&original))] {
            let mut document = fixture(&format!("q {matrix} cm /Cover Do Q"), 0);
            let id = *document.get_pages().get(&1).unwrap();
            document.get_object_mut(id).unwrap().as_dict_mut().unwrap().set("MediaBox", vec![0.into(), 0.into(), 3.into(), 2.into()]);
            assert_eq!(run(document).unwrap(), expected);
        }
    }
    #[test]
    fn page_crop_is_supported_but_clipping_commands_require_rendering() {
        let original = run(fixture(DRAW, 0)).unwrap();
        let expected = image::imageops::crop_imm(&original, 1, 0, 1, 2).to_image();
        assert!(run(fixture(&format!("1 1 1 2 re W n {DRAW}"), 0)).is_none());
        assert!(run(fixture(&format!("1 1 1 2 re W* n {DRAW}"), 0)).is_none());
        assert!(run(fixture(&format!("1 1 m 2 1 l 2 3 l 1 3 l h W n {DRAW}"), 0)).is_none());
        for path in ["1 1 1 2 re 0 0 1 1 re W n", "W n", "1 1 1 2 re W", "0 0 m 1 1 2 2 3 3 c W n"] {
            assert!(run(fixture(&format!("{path} {DRAW}"), 0)).is_none(), "{path}");
        }
        let mut document = fixture(DRAW, 0);
        let id = *document.get_pages().get(&1).unwrap();
        document.get_object_mut(id).unwrap().as_dict_mut().unwrap().set("CropBox", vec![1.into(), 1.into(), 2.into(), 3.into()]);
        assert_eq!(run(document).unwrap(), expected);
        assert!(run(fixture(&format!("q 1 1 1 2 re W n Q {DRAW}"), 0)).is_none());
    }
    #[test]
    fn visible_content_complex_clipping_and_skew_require_rendering() {
        for contents in
            [format!("{DRAW} BT (Title) Tj ET"), format!("{DRAW} {DRAW}"), format!("{DRAW} 0 0 1 1 re f"), format!("0 0 m 1 1 l W n {DRAW}"), format!("{DRAW} BT 7 Tr (OCR) Tj ET"), format!("{DRAW} BT 3 Tr (OCR) Tj 0 Tr (Title) Tj ET")]
        {
            assert!(run(fixture(&contents, 0)).is_none(), "{contents}");
        }
        assert!(run(fixture(&format!("{DRAW} BT 3 Tr (OCR) Tj ET"), 0)).is_none());
    }
    #[test]
    fn all_text_including_invisible_ocr_requires_rendering() {
        for text in ["() Tj", "( ) Tj", "[( ) 10 ()] TJ", "(Title) Tj"] {
            assert!(run(fixture(&format!("{DRAW} BT {text} ET"), 0)).is_none());
            assert!(run(fixture(&format!("{DRAW} BT 3 Tr {text} ET"), 0)).is_none());
        }
    }
    #[test]
    fn non_image_commands_require_rendering_before_or_after_the_image() {
        for command in ["BT ET", "0 g", "1 0 0 rg", "1 w", "/GS0 gs", "0 0 1 1 re W n", "/Figure BMC EMC", "n"] {
            assert!(run(fixture(&format!("{command} {DRAW}"), 0)).is_none(), "{command}");
            assert!(run(fixture(&format!("{DRAW} {command}"), 0)).is_none(), "{command}");
        }
    }
    #[test]
    fn all_marked_content_requires_rendering() {
        let tagged = format!("/Figure <</MCID 0>> BDC {DRAW} EMC");
        assert!(run(fixture(&tagged, 0)).is_none());
        assert!(run(fixture(&format!("/OC <<>> BDC {DRAW} EMC"), 0)).is_none());
        assert!(run(fixture(&format!("/Figure BMC {DRAW}"), 0)).is_none());
    }
    #[test]
    fn nested_forms_require_rendering() {
        let mut document = fixture(DRAW, 0);
        let page_id = *document.get_pages().get(&1).unwrap();
        let resources = document.get_object(page_id).unwrap().as_dict().unwrap().get(b"Resources").unwrap().clone();
        let form = document.add_object(Stream::new(
            dictionary! {
                "Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),2.into(),3.into()],
                "Matrix"=>vec![1.into(),0.into(),0.into(),1.into(),1.into(),0.into()], "Resources"=>resources,
            },
            DRAW.as_bytes().to_vec(),
        ));
        let contents = document.add_object(Stream::new(Dictionary::new(), b"/Nested Do".to_vec()));
        let page = document.get_object_mut(page_id).unwrap().as_dict_mut().unwrap();
        page.set("Contents", contents);
        page.set("Resources", dictionary! {"XObject"=>dictionary!{"Nested"=>form}});
        assert!(run(document).is_none());
    }
    #[test]
    fn skew_and_transparency_groups_require_rendering() {
        assert!(run(fixture("q 2 0.5 0 3 0 0 cm /Cover Do Q", 0)).is_none());
        let mut document = fixture(DRAW, 0);
        let page = *document.get_pages().get(&1).unwrap();
        document.get_object_mut(page).unwrap().as_dict_mut().unwrap().set("Group", dictionary! {"S"=>"Transparency"});
        assert!(run(document).is_none());
        let mut document = fixture(DRAW, 0);
        image_mut(&mut document).dict.set("SMaskInData", 1);
        assert!(run(document).is_none());
    }
    #[test]
    fn soft_masks_require_rendering_even_when_opaque() {
        for value in [255, 254, 128, 0] {
            let mut document = fixture(DRAW, 0);
            let mask = document.add_object(Stream::new(
                dictionary! {
                    "Type"=>"XObject", "Subtype"=>"Image", "Width"=>2, "Height"=>3,
                    "ColorSpace"=>"DeviceGray", "BitsPerComponent"=>8,
                },
                vec![value; 6],
            ));
            image_mut(&mut document).dict.set("SMask", mask);
            assert!(run(document).is_none());
        }
    }
    #[test]
    fn decode_ranges_are_applied_before_gray_rgb_and_cmyk_conversion() {
        let mut document = fixture(DRAW, 0);
        let image = image_mut(&mut document);
        image.dict.set("Decode", vec![1.into(), 0.into(), 1.into(), 0.into(), 1.into(), 0.into()]);
        assert_eq!(run(document).unwrap().get_pixel(0, 0).0, [0, 255, 255]);
        let mut document = fixture(DRAW, 0);
        let image = image_mut(&mut document);
        image.dict.set("ColorSpace", Object::Name(b"DeviceCMYK".to_vec()));
        image.dict.set("Decode", vec![1.into(), 0.into(), 1.into(), 0.into(), 1.into(), 0.into(), 1.into(), 0.into()]);
        image.set_content([255, 0, 255, 255].repeat(6));
        assert_eq!(run(document).unwrap().get_pixel(0, 0).0, [255, 0, 255]);
        let mut document = fixture(DRAW, 0);
        let image = image_mut(&mut document);
        image.dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
        image.dict.set("BitsPerComponent", 1);
        image.dict.set("Decode", vec![1.into(), 0.into()]);
        image.set_content(vec![0b01000000; 3]);
        let decoded = run(document).unwrap();
        assert_eq!(decoded.get_pixel(0, 0).0, [255; 3]);
        assert_eq!(decoded.get_pixel(1, 0).0, [0; 3]);
    }
    #[test]
    fn large_jpeg_is_extracted_without_a_scaled_dimension_mismatch() {
        let mut document = fixture("q 800 0 0 1200 0 0 cm /Cover Do Q", 0);
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut encoded).encode(&vec![100; 800 * 1200 * 3], 800, 1200, image::ColorType::Rgb8.into()).unwrap();
        let image = image_mut(&mut document);
        image.dict.set("Width", 800);
        image.dict.set("Height", 1200);
        image.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
        image.set_content(encoded);
        let page_id = *document.get_pages().get(&1).unwrap();
        document.get_object_mut(page_id).unwrap().as_dict_mut().unwrap().set("MediaBox", vec![0.into(), 0.into(), 800.into(), 1200.into()]);
        let image = run(document).unwrap();
        assert_eq!(image.dimensions(), (600, 900));
        assert!(image.as_raw().iter().all(|v| (i16::from(*v) - 100).abs() <= 2));
    }

    #[test]
    fn explicit_and_default_icc_profiles_convert_the_same_pixels() {
        let expected = run(fixture(DRAW, 0)).unwrap();
        for default in [false, true] {
            let mut document = fixture(DRAW, 0);
            let profile = document.add_object(Stream::new(dictionary! {"N"=>3}, moxcms::ColorProfile::new_srgb().encode().unwrap()));
            let space = Object::Array(vec![Object::Name(b"ICCBased".to_vec()), profile.into()]);
            if default {
                let page_id = *document.get_pages().get(&1).unwrap();
                document.get_object_mut(page_id).unwrap().as_dict_mut().unwrap().get_mut(b"Resources").unwrap().as_dict_mut().unwrap().set("ColorSpace", dictionary! {"DefaultRGB"=>space});
            } else {
                image_mut(&mut document).dict.set("ColorSpace", space);
            }
            let actual = run(document).unwrap();
            assert!(actual.as_raw().iter().zip(expected.as_raw()).all(|(a, b)| (i16::from(*a) - i16::from(*b)).abs() <= 1));
        }
    }
    #[test]
    fn jpeg2000_preserves_pixels_and_ignores_pdf_decode_ranges() {
        let mut document = fixture(DRAW, 0);
        let stream = image_mut(&mut document);
        stream.dict.set("Filter", Object::Name(b"JPXDecode".to_vec()));
        stream.dict.set("Decode", vec![1.into(), 0.into(), 1.into(), 0.into(), 1.into(), 0.into()]);
        stream.set_content(include_bytes!("../tests/fixtures/six-colours.jp2").to_vec());
        assert_eq!(run(document).unwrap(), run(fixture(DRAW, 0)).unwrap());
    }
    #[test]
    fn indirect_image_filter_names_are_resolved() {
        let mut document = fixture(DRAW, 0);
        let filter = document.add_object(Object::Name(b"JPXDecode".to_vec()));
        let stream = image_mut(&mut document);
        stream.dict.set("Filter", filter);
        stream.set_content(include_bytes!("../tests/fixtures/six-colours.jp2").to_vec());
        assert_eq!(run(document).unwrap(), run(fixture(DRAW, 0)).unwrap());
    }

    #[test]
    fn chained_filters_keep_individual_decode_parameters() {
        let mut document = fixture("q 800 0 0 1200 0 0 cm /Cover Do Q", 0);
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut encoded).encode(&vec![100; 800 * 1200 * 3], 800, 1200, image::ColorType::Rgb8.into()).unwrap();
        let mut compressed = Stream::new(Dictionary::new(), encoded);
        compressed.compress().unwrap();
        let stream = image_mut(&mut document);
        stream.dict.set("Width", 800);
        stream.dict.set("Height", 1200);
        stream.dict.set("Filter", vec![Object::Name(b"FlateDecode".to_vec()), Object::Name(b"DCTDecode".to_vec())]);
        stream.dict.set("DecodeParms", vec![Object::Null, dictionary! {"ColorTransform"=>1}.into()]);
        stream.set_content(compressed.content);
        let page_id = *document.get_pages().get(&1).unwrap();
        document.get_object_mut(page_id).unwrap().as_dict_mut().unwrap().set("MediaBox", vec![0.into(), 0.into(), 800.into(), 1200.into()]);
        let image = run(document).unwrap();
        assert_eq!(image.dimensions(), (600, 900));
        assert!(image.as_raw().iter().all(|v| (i16::from(*v) - 100).abs() <= 2));
    }
}
