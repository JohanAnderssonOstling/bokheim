//! Extract a sole visible image or an opaque rectangular image mosaic.
//! Unknown drawing operations always fall back to rendering.
use super::*;
use lopdf::{Dictionary, Document, Object, Stream};

mod appearance;
mod codecs;
mod color;
mod inline;
mod text;
mod tiles;

type Matrix = [f64; 6];
type Rect = [f64; 4];
const IDENTITY: Matrix = [1., 0., 0., 1., 0., 0.];
const CONTENT_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
struct State {
    matrix: Matrix,
    clip: Rect,
    text_mode: i64,
    font: Option<Vec<u8>>,
    safe: bool,
    grouped: bool,
    group_spaces: Vec<color::Space>,
}
struct Candidate<'a> {
    stream: std::borrow::Cow<'a, Stream>,
    resources: &'a Dictionary,
    state: State,
}

pub(super) fn extract(reader: &mut (impl Read + Seek)) -> Option<RgbImage> {
    extract_with_reason(reader, &mut "")
}

fn extract_with_reason(reader: &mut (impl Read + Seek), reason: &mut &'static str) -> Option<RgbImage> {
    *reason = "PDF structure or inspection limits";
    let document = pdf_range_reader::cover(reader).ok()?;
    *reason = "Page properties or transparency group";
    let page = document.objects.values().find_map(|object| {
        let dictionary = object.as_dict().ok()?;
        (dictionary.get(b"BokheimInspectionPage").and_then(Object::as_i64).ok() == Some(1)).then_some(dictionary)
    })?;
    let media = rectangle(inherited(&document, page, b"MediaBox")?)?;
    let clip = match inherited(&document, page, b"CropBox") {
        Some(bounds) => intersect(media, rectangle(bounds)?)?,
        None => media,
    };
    let rotation = match inherited(&document, page, b"Rotate") {
        Some(value) => value.as_i64().ok()?.rem_euclid(360),
        None => 0,
    };
    if rotation % 90 != 0 {
        return None;
    }
    let resources = inherited(&document, page, b"Resources")?.as_dict().ok()?;
    let mut initial = State { matrix: IDENTITY, clip, text_mode: 0, font: None, safe: true, grouped: false, group_spaces: Vec::new() };
    appearance::group(&document, page, resources, &mut initial)?;
    let mut candidate = Vec::new();
    let mut remaining = CONTENT_LIMIT;
    *reason = "Page drawing instructions";
    walk(&document, page.get(b"Contents").ok()?, resources, initial, &mut candidate, &mut remaining, 0)?;
    if candidate.len() > 1 {
        return tiles::compose(&document, candidate, rotation, reason);
    }
    let candidate = candidate.pop()?;
    *reason = "Image encoding, colour space, mask, or dimensions";
    if !candidate.state.group_spaces.is_empty() {
        let space = color::resolve_space(&document, candidate.resources, candidate.stream.dict.get(b"ColorSpace").ok()?)?;
        if candidate.state.group_spaces.iter().any(|group| !group.equivalent(&space)) {
            *reason = "Transparency group colour conversion";
            return None;
        }
    }
    let mut image = decode(&document, &candidate.stream, candidate.resources, true, candidate.state.grouped, reason)?;
    *reason = "Image placement";
    image = place(image, candidate.state)?;
    Some(match rotation {
        90 => image::imageops::rotate90(&image),
        180 => image::imageops::rotate180(&image),
        270 => image::imageops::rotate270(&image),
        _ => image,
    })
}

fn walk<'a>(document: &'a Document, contents: &'a Object, resources: &'a Dictionary, mut state: State, candidate: &mut Vec<Candidate<'a>>, remaining: &mut usize, depth: usize) -> Option<()> {
    if depth >= 32 {
        return None;
    }
    let contents = resolve(document, contents)?;
    let mut bytes = Vec::new();
    let objects = match contents {
        Object::Array(items) => items.iter().collect::<Vec<_>>(),
        object => vec![object],
    };
    for object in objects {
        let stream = resolve(document, object)?.as_stream().ok()?;
        let content = stream.get_plain_content_with_limit(*remaining).ok()?;
        *remaining = remaining.checked_sub(content.len().checked_add(1)?)?;
        bytes.extend(content);
        bytes.push(b'\n');
    }
    let mut stack = Vec::new();
    let mut path = None;
    let mut pending_clip = false;
    let mut in_text = false;
    let mut marked = Vec::new();
    let mut optional_depth = 0_usize;
    let mut path_started = false;
    let mut path_complex = false;
    let mut points: Vec<[f64; 2]> = Vec::new();
    for operation in inline::operations(&bytes, document, resources)? {
        let args = &operation.operands;
        match operation.operator.as_str() {
            "q" if args.is_empty() && !in_text => {
                if stack.len() >= 32 {
                    return None;
                }
                stack.push(state.clone());
            }
            "Q" if args.is_empty() && !in_text => state = stack.pop()?,
            "cm" if args.len() == 6 && !in_text => {
                let matrix = numbers(args)?.try_into().ok()?;
                state.matrix = multiply(state.matrix, matrix)?;
            }
            "re" if args.len() == 4 && !in_text => {
                let v = numbers(args)?;
                if path_started || !axis_aligned(state.matrix) {
                    path_complex = true;
                }
                path_started = true;
                path = Some(transformed([v[0], v[1], v[0] + v[2], v[1] + v[3]], state.matrix)?);
            }
            "m" if args.len() == 2 && !in_text => {
                if path_started {
                    path_complex = true;
                }
                path_started = true;
                points.clear();
                points.push(point(numbers(args)?.try_into().ok()?, state.matrix)?);
            }
            "l" if args.len() == 2 && path_started && !in_text => {
                if points.len() >= 4 {
                    path_complex = true;
                } else {
                    points.push(point(numbers(args)?.try_into().ok()?, state.matrix)?);
                }
            }
            "c" | "v" | "y" if path_started && !in_text => path_complex = true,
            "h" if args.is_empty() && path_started && !in_text => (),
            "W" | "W*" if args.is_empty() && !in_text => {
                if path.is_none() {
                    path = path_rectangle(&points);
                }
                pending_clip = true;
            }
            "n" if args.is_empty() && !in_text => {
                if pending_clip {
                    if path_complex || path.is_none() {
                        state.safe = false;
                    } else {
                        state.clip = intersect(state.clip, path?).unwrap_or([0.; 4]);
                    }
                }
                path = None;
                points.clear();
                pending_clip = false;
                path_started = false;
                path_complex = false;
            }
            "BI" if args.len() == 1 && !in_text && !path_started && !pending_clip && state.safe && optional_depth == 0 && candidate.len() < 64 => {
                let stream = args[0].as_stream().ok()?.clone();
                candidate.push(Candidate { stream: std::borrow::Cow::Owned(stream), resources, state: state.clone() });
            }
            "Do" if args.len() == 1 && !in_text && !path_started && !pending_clip => {
                let objects = resolve(document, resources.get(b"XObject").ok()?)?.as_dict().ok()?;
                let object = objects.get(args[0].as_name().ok()?).ok()?;
                let stream = resolve(document, object)?.as_stream().ok()?;
                match stream.dict.get(b"Subtype").and_then(Object::as_name).ok()? {
                    b"Image" if candidate.len() < 64 => {
                        if !state.safe || stream.dict.has(b"OC") || optional_depth != 0 {
                            return None;
                        }
                        candidate.push(Candidate { stream: std::borrow::Cow::Borrowed(stream), resources, state: state.clone() });
                    }
                    b"Form" => {
                        if stream.dict.has(b"Ref") {
                            return None;
                        }
                        let mut nested = state.clone();
                        if stream.dict.has(b"OC") || optional_depth != 0 {
                            nested.safe = false;
                        }
                        if let Ok(matrix) = stream.dict.get(b"Matrix") {
                            nested.matrix = multiply(state.matrix, numbers(resolve(document, matrix)?.as_array().ok()?)?.try_into().ok()?)?;
                        }
                        if !axis_aligned(nested.matrix) {
                            nested.safe = false;
                        }
                        let bbox = rectangle(resolve(document, stream.dict.get(b"BBox").ok()?)?)?;
                        nested.clip = intersect(state.clip, transformed(bbox, nested.matrix)?).unwrap_or([0.; 4]);
                        let nested_resources = match stream.dict.get(b"Resources") {
                            Ok(value) => resolve(document, value)?.as_dict().ok()?,
                            Err(_) => resources,
                        };
                        appearance::group(document, &stream.dict, nested_resources, &mut nested)?;
                        walk(document, object, nested_resources, nested, candidate, remaining, depth + 1)?;
                    }
                    _ => return None,
                }
            }
            "gs" if args.len() == 1 && !in_text => {
                let states = resolve(document, resources.get(b"ExtGState").ok()?)?.as_dict().ok()?;
                let graphics = resolve(document, states.get(args[0].as_name().ok()?).ok()?)?.as_dict().ok()?;
                for (key, value) in graphics.iter() {
                    let value = resolve(document, value)?;
                    match key.as_slice() {
                        b"Type" | b"LW" | b"LC" | b"LJ" | b"ML" | b"D" | b"FL" | b"SA" | b"SM" => (),
                        b"CA" | b"ca" if number(value)? == 1. => (),
                        b"BM" if value.as_name().ok()? == b"Normal" || value.as_name().ok()? == b"Compatible" => (),
                        b"SMask" if value.as_name().ok()? == b"None" => (),
                        b"OP" | b"op" if !value.as_bool().ok()? => (),
                        // Overprint is always disabled in accepted graphics states.
                        b"OPM" if matches!(value.as_i64().ok()?, 0 | 1) => (),
                        b"AIS" | b"TK" if value.as_bool().is_ok() => (),
                        b"BG2" | b"UCR2" | b"HT" if value.as_name().ok()? == b"Default" => (),
                        _ => state.safe = false,
                    }
                }
            }
            "BMC" | "BDC" if (args.len() == 1 || args.len() == 2) => {
                if marked.len() >= 32 {
                    return None;
                }
                let mut optional = args[0].as_name().ok()? == b"OC";
                if args.len() == 2 {
                    let property = match &args[1] {
                        Object::Name(name) => {
                            let properties = resolve(document, resources.get(b"Properties").ok()?)?.as_dict().ok()?;
                            resolve(document, properties.get(name).ok()?)?
                        }
                        object => resolve(document, object)?,
                    };
                    optional |= property.as_dict().ok()?.has(b"OC");
                }
                marked.push(optional);
                optional_depth += usize::from(optional);
            }
            "EMC" if args.is_empty() => optional_depth -= usize::from(marked.pop()?),
            "BT" if args.is_empty() && !in_text => in_text = true,
            "ET" if args.is_empty() && in_text => in_text = false,
            "Tr" if args.len() == 1 => {
                state.text_mode = args[0].as_i64().ok()?;
                if !(0..=7).contains(&state.text_mode) {
                    return None;
                }
            }
            // Only mode 3 is invisible without also changing the clipping path.
            "Tj" | "TJ" | "'" | "\"" if in_text => {
                if state.text_mode != 3 && (state.text_mode >= 4 || !text::blank(document, resources, state.font.as_deref(), args)?) {
                    return None;
                }
            }
            "Tf" if in_text && args.len() == 2 => state.font = Some(args[0].as_name().ok()?.to_vec()),
            "Tm" | "Td" | "TD" | "T*" | "Tc" | "Tw" | "Tz" | "TL" | "Ts" if in_text => (),
            // Colour/stroke settings do not paint anything. Image colour is in its dictionary.
            "g" | "G" | "rg" | "RG" | "k" | "K" | "cs" | "CS" | "sc" | "SC" | "scn" | "SCN" | "w" | "J" | "j" | "M" | "d" | "i" => (),
            _ => return None,
        }
    }
    (stack.is_empty() && marked.is_empty() && !in_text && !path_started && !pending_clip).then_some(())
}

fn multiply(p: Matrix, q: Matrix) -> Option<Matrix> {
    let [a, b, c, d, e, f] = p;
    let [aa, bb, cc, dd, ee, ff] = q;
    let result = [a * aa + c * bb, b * aa + d * bb, a * cc + c * dd, b * cc + d * dd, a * ee + c * ff + e, b * ee + d * ff + f];
    result.iter().all(|v| v.is_finite()).then_some(result)
}
fn axis_aligned([a, b, c, d, _, _]: Matrix) -> bool {
    // Do not silently approximate skew as rotation.
    (b == 0. && c == 0. && a != 0. && d != 0.) || (a == 0. && d == 0. && b != 0. && c != 0.)
}
fn rectangle(object: &Object) -> Option<Rect> {
    let rect: Rect = numbers(object.as_array().ok()?)?.try_into().ok()?;
    (rect[2] > rect[0] && rect[3] > rect[1]).then_some(rect)
}
fn transformed(rect: Rect, [a, b, c, d, e, f]: Matrix) -> Option<Rect> {
    let points = [(rect[0], rect[1]), (rect[0], rect[3]), (rect[2], rect[1]), (rect[2], rect[3])].map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
    let result = [points.iter().map(|p| p.0).reduce(f64::min)?, points.iter().map(|p| p.1).reduce(f64::min)?, points.iter().map(|p| p.0).reduce(f64::max)?, points.iter().map(|p| p.1).reduce(f64::max)?];
    result.iter().all(|v| v.is_finite()).then_some(result)
}
fn point([x, y]: [f64; 2], [a, b, c, d, e, f]: Matrix) -> Option<[f64; 2]> {
    let p = [a * x + c * y + e, b * x + d * y + f];
    p.iter().all(|v| v.is_finite()).then_some(p)
}
fn path_rectangle(points: &[[f64; 2]]) -> Option<Rect> {
    if points.len() != 4 {
        return None;
    }
    for i in 0..4 {
        let p = points[i];
        let q = points[(i + 1) % 4];
        if !((p[0] == q[0]) ^ (p[1] == q[1])) {
            return None;
        }
        if points[..i].contains(&p) {
            return None;
        }
    }
    Some([points.iter().map(|p| p[0]).reduce(f64::min)?, points.iter().map(|p| p[1]).reduce(f64::min)?, points.iter().map(|p| p[0]).reduce(f64::max)?, points.iter().map(|p| p[1]).reduce(f64::max)?])
}
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (r[2] > r[0] && r[3] > r[1]).then_some(r)
}
fn place(mut image: RgbImage, state: State) -> Option<RgbImage> {
    let [a, b, c, d, _, _] = state.matrix;
    if !axis_aligned(state.matrix) {
        return place_affine(image, state);
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
    let bounds = transformed([0., 0., 1., 1.], state.matrix)?;
    let visible = intersect(bounds, state.clip)?;
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
    Some(image)
}

fn place_affine(image: RgbImage, state: State) -> Option<RgbImage> {
    let [a, b, c, d, e, f] = state.matrix;
    let determinant = a * d - b * c;
    if !determinant.is_finite() || determinant == 0. {
        return None;
    }
    let visible = intersect(transformed([0., 0., 1., 1.], state.matrix)?, state.clip)?;
    let width = image.width().min(THUMBNAIL_WIDTH);
    let height = (f64::from(width) * (visible[3] - visible[1]) / (visible[2] - visible[0])).round();
    if !height.is_finite() || height < 1. || height > f64::from(MAX_COVER_IMAGE_DIMENSION) {
        return None;
    }
    let height = height as u32;
    validate_cover_image_dimensions(width, height).ok()?;
    let mut output = RgbImage::from_pixel(width, height, image::Rgb([255; 3]));
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let px = visible[0] + (f64::from(x) + 0.5) * (visible[2] - visible[0]) / f64::from(width) - e;
        let py = visible[3] - (f64::from(y) + 0.5) * (visible[3] - visible[1]) / f64::from(height) - f;
        let u = (d * px - c * py) / determinant;
        let v = (-b * px + a * py) / determinant;
        if !(0. ..=1.).contains(&u) || !(0. ..=1.).contains(&v) {
            continue;
        }
        let sx = (u * f64::from(image.width()) - 0.5).clamp(0., f64::from(image.width() - 1));
        let sy = ((1. - v) * f64::from(image.height()) - 0.5).clamp(0., f64::from(image.height() - 1));
        let x0 = sx.floor() as u32;
        let y0 = sy.floor() as u32;
        let x1 = (x0 + 1).min(image.width() - 1);
        let y1 = (y0 + 1).min(image.height() - 1);
        let dx = sx - f64::from(x0);
        let dy = sy - f64::from(y0);
        for channel in 0..3 {
            let top = f64::from(image.get_pixel(x0, y0)[channel]) * (1. - dx) + f64::from(image.get_pixel(x1, y0)[channel]) * dx;
            let bottom = f64::from(image.get_pixel(x0, y1)[channel]) * (1. - dx) + f64::from(image.get_pixel(x1, y1)[channel]) * dx;
            pixel[channel] = (top * (1. - dy) + bottom * dy).round() as u8;
        }
    }
    Some(output)
}

fn resolve<'a>(document: &'a Document, mut object: &'a Object) -> Option<&'a Object> {
    for _ in 0..32 {
        match object {
            Object::Reference(id) => object = document.objects.get(id)?,
            _ => return Some(object),
        }
    }
    None
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
fn number(object: &Object) -> Option<f64> {
    let number = match object {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return None,
    };
    number.is_finite().then_some(number)
}
fn numbers(objects: &[Object]) -> Option<Vec<f64>> {
    objects.iter().map(number).collect()
}

fn decode(document: &Document, stream: &Stream, resources: &Dictionary, allow_mask: bool, opaque_only: bool, reason: &mut &'static str) -> Option<RgbImage> {
    *reason = "Image mask or subtype";
    let dict = &stream.dict;
    if dict.get(b"Subtype").and_then(Object::as_name).ok()? != b"Image" {
        return None;
    }
    // Preblended matte colours and standalone stencils need separate handling.
    if dict.has(b"Matte") {
        return None;
    }
    if let Ok(value) = dict.get(b"ImageMask") {
        if value.as_bool().ok()? {
            return None;
        }
    }
    *reason = "Image dimensions or size limits";
    let width = u32::try_from(dict.get(b"Width").ok()?.as_i64().ok()?).ok()?;
    let height = u32::try_from(dict.get(b"Height").ok()?.as_i64().ok()?).ok()?;
    validate_cover_image_dimensions(width, height).ok()?;
    *reason = "Image filter chain";
    let payload = codecs::payload(document, stream)?;
    let jpx = payload.filter == b"JPXDecode";
    let mut alpha = None;
    let space;
    let mut samples;
    let bits = if jpx { 8 } else { dict.get(b"BitsPerComponent").and_then(Object::as_i64).ok()? };
    if jpx {
        *reason = "JPEG 2000 decoding";
        let decoded = codecs::jpeg2000(&payload, width, height)?;
        // A PDF ColorSpace overrides the colour space embedded in JP2.
        space = match dict.get(b"ColorSpace") {
            Ok(value) => color::resolve_space(document, resources, value)?,
            Err(_) => decoded.space,
        };
        if let Ok(value) = dict.get(b"SMaskInData") {
            // Associated (premultiplied) alpha needs unpremultiplication in its
            // source colour space. Keep that less common case on the renderer.
            match value.as_i64().ok()? {
                0 => (),
                1 => alpha = decoded.alpha,
                _ => return None,
            }
        }
        samples = decoded.samples;
    } else {
        *reason = "Image colour space or ICC profile";
        space = color::resolve_space(document, resources, dict.get(b"ColorSpace").ok()?)?;
        let channels = space.channels();
        *reason = "Image bit depth";
        if !matches!(bits, 1 | 2 | 4 | 8 | 16) {
            return None;
        }
        *reason = "Image codec decoding";
        samples = match payload.filter.as_slice() {
            b"DCTDecode" if bits == 8 => codecs::jpeg(&payload, width, height, channels, reason)?,
            b"CCITTFaxDecode" if bits == 1 && channels == 1 => codecs::ccitt(&payload, width, height)?,
            b"JBIG2Decode" if bits == 1 && channels == 1 => codecs::jbig2(document, &payload, width, height)?,
            b"" => unpack(&payload.bytes, width, height, channels, bits)?,
            _ => return None,
        };
    }
    let channels = space.channels();
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    *reason = "Decoded sample dimensions";
    if samples.len() != pixels.checked_mul(channels)? {
        return None;
    }
    if let Ok(mask) = dict.get(b"Mask") {
        if !allow_mask || alpha.is_some() {
            return None;
        }
        let mask = resolve(document, mask)?;
        alpha = Some(match mask {
            Object::Array(values) if values.len() == channels * 2 && !jpx && bits <= 8 => {
                let limits = numbers(values)?;
                let max = ((1_u32 << bits) - 1) as f64;
                samples
                    .chunks_exact(channels)
                    .map(|pixel| {
                        if pixel.iter().enumerate().all(|(i, s)| {
                            let v = (f64::from(*s) * max / 255.).round();
                            v >= limits[2 * i] && v <= limits[2 * i + 1]
                        }) {
                            0
                        } else {
                            255
                        }
                    })
                    .collect()
            }
            Object::Stream(mask) => {
                if mask.dict.get(b"ImageMask").and_then(Object::as_bool).ok() != Some(true) {
                    return None;
                }
                let mut mask = mask.clone();
                mask.dict.remove(b"ImageMask");
                mask.dict.set("ColorSpace", "DeviceGray");
                mask.dict.set("BitsPerComponent", 1);
                let decoded = decode(document, &mask, resources, false, false, reason)?;
                if decoded.dimensions() != (width, height) {
                    return None;
                }
                decoded.pixels().map(|p| 255 - p[0]).collect()
            }
            _ => return None,
        });
    }
    // ISO 32000: /Decode is ignored for JPXDecode, whose samples are normalized
    // by the JPEG2000 decoder. All other codecs produce PDF sample values.
    if !jpx {
        *reason = "Image Decode mapping";
        let mapping = match dict.get(b"Decode") {
            Ok(value) => Some(numbers(resolve(document, value)?.as_array().ok()?)?),
            Err(_) => None,
        };
        space.map_samples(&mut samples, bits, mapping.as_deref())?;
    }
    *reason = "Image soft mask";
    if let Ok(value) = dict.get(b"SMask") {
        if !allow_mask || alpha.is_some() {
            return None;
        }
        let mask = resolve(document, value)?.as_stream().ok()?;
        if !matches!(color::resolve_space(document, resources, mask.dict.get(b"ColorSpace").ok()?)?, color::Space::Device(1)) {
            return None;
        }
        let mask = decode(document, mask, resources, false, false, reason)?;
        if mask.dimensions() != (width, height) {
            return None;
        }
        alpha = Some(mask.pixels().map(|pixel| pixel[0]).collect());
    }
    *reason = "Image colour conversion";
    let mut image = space.rgb(samples, width, height)?;
    if let Some(alpha) = alpha {
        if opaque_only && alpha.iter().any(|a| *a != 255) {
            return None;
        }
        if alpha.len() != pixels {
            return None;
        }
        // This path is used only for a sole visible image on the white page.
        // Opaque masks are a no-op; other soft masks composite onto that white.
        for (pixel, a) in image.pixels_mut().zip(alpha) {
            for channel in &mut pixel.0 {
                *channel = ((u32::from(*channel) * u32::from(a) + 255 * (255 - u32::from(a)) + 127) / 255) as u8;
            }
        }
    }
    Some(image)
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
    fn palette_decode_mapping_and_colour_key_mask_preserve_colours() {
        let mut document = fixture(DRAW, 0);
        let image = image_mut(&mut document);
        image.dict.set("ColorSpace", vec![Object::Name(b"Indexed".to_vec()), Object::Name(b"DeviceRGB".to_vec()), 1.into(), Object::String(vec![255, 0, 0, 0, 0, 255], lopdf::StringFormat::Hexadecimal)]);
        image.dict.set("BitsPerComponent", 1);
        image.set_content(vec![0x40, 0x80, 0x40]);
        image.dict.set("Mask", vec![0.into(), 0.into()]);
        let cover = run(document.clone()).unwrap();
        assert_eq!(cover.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(cover.get_pixel(1, 0).0, [0, 0, 255]);
        let image = image_mut(&mut document);
        image.dict.remove(b"Mask");
        image.dict.set("Decode", vec![1.into(), 0.into()]);
        let cover = run(document).unwrap();
        assert_eq!(cover.get_pixel(0, 0).0, [0, 0, 255]);
        assert_eq!(cover.get_pixel(1, 0).0, [255, 0, 0]);
    }
    #[test]
    fn explicit_mask_uses_zero_for_paint_and_respects_decode() {
        let mut document = fixture(DRAW, 0);
        let mask = document.add_object(Stream::new(dictionary! {"Subtype"=>"Image","Width"=>2,"Height"=>3,"ImageMask"=>true,"BitsPerComponent"=>1}, vec![0x40, 0x40, 0x40]));
        image_mut(&mut document).dict.set("Mask", mask);
        let cover = run(document.clone()).unwrap();
        assert_eq!(cover.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(cover.get_pixel(1, 0).0, [255, 255, 255]);
        document.get_object_mut(mask).unwrap().as_stream_mut().unwrap().dict.set("Decode", vec![1.into(), 0.into()]);
        let cover = run(document).unwrap();
        assert_eq!(cover.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(cover.get_pixel(1, 0).0, [0, 255, 0]);
    }
    #[test]
    fn comments_inside_marked_content_dictionaries_do_not_hide_the_cover() {
        let content = format!("/Figure << /Example [1 % comment before closing array\n] % comment before closing dictionary\n>> BDC {DRAW} EMC");
        assert_eq!(run(fixture(&content, 0)), run(fixture(DRAW, 0)));
    }

    #[test]
    fn inline_binary_bytes_are_not_parsed_as_operators() {
        let mut document = fixture("", 0);
        let page = *document.get_pages().get(&1).unwrap();
        let id = document.get_object(page).unwrap().as_dict().unwrap().get(b"Contents").unwrap().as_reference().unwrap();
        let bytes = b"q 2 0 0 3 0 0 cm BI /W 2 /H 3 /CS /G /BPC 8 ID  EI BI EI Q";
        document.get_object_mut(id).unwrap().as_stream_mut().unwrap().set_content(bytes.to_vec());
        let image = run(document).unwrap();
        assert_eq!(image.dimensions(), (2, 3));
        assert_eq!(image.get_pixel(0, 0).0, [32; 3]);
        assert_eq!(image.get_pixel(1, 2).0, [73; 3]);
        assert!(inline::operations(b"BI /W 1", &Document::new(), &Dictionary::new()).is_none());
        assert!(inline::operations(b"BI /W 1 /H 1 /BPC 8 ID x EI", &Document::new(), &Dictionary::new()).is_none());
    }
    #[test]
    fn flate_inline_image_uses_codec_boundary_and_rejects_truncation() {
        let mut stream = Stream::new(Dictionary::new(), vec![42; 300]);
        stream.compress().unwrap();
        assert!(stream.dict.has(b"Filter"));
        let mut bytes = b"BI /W 10 /H 10 /CS /RGB /BPC 8 /F /Fl ID ".to_vec();
        bytes.extend_from_slice(&stream.content);
        bytes.extend_from_slice(b" EI");
        let doc = Document::new();
        let resources = Dictionary::new();
        let operations = inline::operations(&bytes, &doc, &resources).unwrap();
        let image = decode(&doc, operations[0].operands[0].as_stream().unwrap(), &resources, true, false, &mut "").unwrap();
        assert!(image.pixels().all(|p| p.0 == [42; 3]));
        let mut document = fixture("", 0);
        let page = *document.get_pages().get(&1).unwrap();
        let content = document.get_object(page).unwrap().as_dict().unwrap().get(b"Contents").unwrap().as_reference().unwrap();
        let mut drawing = b"q 2 0 0 3 0 0 cm ".to_vec();
        drawing.extend_from_slice(&bytes);
        drawing.extend_from_slice(b" Q");
        document.get_object_mut(content).unwrap().as_stream_mut().unwrap().set_content(drawing);
        assert!(run(document).unwrap().pixels().all(|p| p.0 == [42; 3]));
        bytes.truncate(bytes.len() - 6);
        assert!(inline::operations(&bytes, &doc, &resources).is_none());
    }
    #[test]
    fn image_tiles_require_a_gapless_nonoverlapping_rectangle() {
        let image = run(fixture("q 1 0 0 3 0 0 cm /Cover Do Q q 1 0 0 3 1 0 cm /Cover Do Q", 0)).unwrap();
        assert_eq!(image.dimensions(), (600, 900));
        assert_eq!(image.get_pixel(20, 20).0, [255, 0, 0]);
        assert_eq!(image.get_pixel(320, 20).0, [255, 0, 0]);
        assert!(run(fixture("q 0.9 0 0 3 0 0 cm /Cover Do Q q 1 0 0 3 1 0 cm /Cover Do Q", 0)).is_none());
        assert!(run(fixture(&format!("{DRAW} {DRAW}"), 0)).is_none());
    }
    #[test]
    fn calibrated_spaces_have_neutral_white_and_black() {
        for name in ["CalGray", "CalRGB", "Lab"] {
            let space = color::resolve_space(
                &Document::new(),
                &Dictionary::new(),
                &Object::Array(vec![
                    Object::Name(name.as_bytes().to_vec()),
                    dictionary! {"WhitePoint"=>vec![0.9642.into(),1.into(),0.8249.into()],"Matrix"=>vec![0.9642.into(),0.into(),0.into(),0.into(),1.into(),0.into(),0.into(),0.into(),0.8249.into()]}.into(),
                ]),
            )
            .unwrap();
            let samples = match name {
                "CalGray" => vec![0, 255],
                "CalRGB" => vec![0, 0, 0, 255, 255, 255],
                _ => vec![0, 128, 128, 255, 128, 128],
            };
            let image = space.rgb(samples, 2, 1).unwrap();
            assert!(image.get_pixel(0, 0).0.iter().all(|v| *v <= 3), "{name}");
            assert!(image.get_pixel(1, 0).0.iter().all(|v| *v >= 253), "{name}: {:?}", image.get_pixel(1, 0));
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
    fn page_and_explicit_rectangular_crops_select_the_correct_pixels() {
        let original = run(fixture(DRAW, 0)).unwrap();
        let expected = image::imageops::crop_imm(&original, 1, 0, 1, 2).to_image();
        assert_eq!(run(fixture(&format!("1 1 1 2 re W n {DRAW}"), 0)).unwrap(), expected);
        assert_eq!(run(fixture(&format!("1 1 m 2 1 l 2 3 l 1 3 l h W n {DRAW}"), 0)).unwrap(), expected);
        let mut document = fixture(DRAW, 0);
        let id = *document.get_pages().get(&1).unwrap();
        document.get_object_mut(id).unwrap().as_dict_mut().unwrap().set("CropBox", vec![1.into(), 1.into(), 2.into(), 3.into()]);
        assert_eq!(run(document).unwrap(), expected);
        assert_eq!(run(fixture(&format!("q 1 1 1 2 re W n Q {DRAW}"), 0)).unwrap(), original);
    }
    #[test]
    fn visible_content_complex_clipping_and_skew_require_rendering() {
        for contents in
            [format!("{DRAW} BT (Title) Tj ET"), format!("{DRAW} {DRAW}"), format!("{DRAW} 0 0 1 1 re f"), format!("0 0 m 1 1 l W n {DRAW}"), format!("{DRAW} BT 7 Tr (OCR) Tj ET"), format!("{DRAW} BT 3 Tr (OCR) Tj 0 Tr (Title) Tj ET")]
        {
            assert!(run(fixture(&contents, 0)).is_none(), "{contents}");
        }
        assert_eq!(run(fixture(&format!("{DRAW} BT 3 Tr (OCR) Tj ET"), 0)), run(fixture(DRAW, 0)));
    }
    #[test]
    fn accessibility_tags_are_harmless_but_optional_content_requires_rendering() {
        let tagged = format!("/Figure <</MCID 0>> BDC {DRAW} EMC");
        assert_eq!(run(fixture(&tagged, 0)), run(fixture(DRAW, 0)));
        assert!(run(fixture(&format!("/OC <<>> BDC {DRAW} EMC"), 0)).is_none());
        assert!(run(fixture(&format!("/Figure BMC {DRAW}"), 0)).is_none());
    }
    #[test]
    fn nested_forms_apply_their_matrix_resources_and_crop() {
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
        let original = run(fixture(DRAW, 0)).unwrap();
        assert_eq!(run(document).unwrap(), image::imageops::crop_imm(&original, 0, 0, 1, 3).to_image());
    }
    #[test]
    fn soft_masks_are_composited_on_white() {
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
            assert_eq!(run(document).unwrap().get_pixel(0, 0).0, [255, 255 - value, 255 - value]);
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
        image::codecs::jpeg::JpegEncoder::new(&mut encoded).encode(&vec![100; 800 * 1200 * 3], 800, 1200, image::ColorType::Rgb8).unwrap();
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
        image::codecs::jpeg::JpegEncoder::new(&mut encoded).encode(&vec![100; 800 * 1200 * 3], 800, 1200, image::ColorType::Rgb8).unwrap();
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

    /// Read-only compatibility audit against a real library; never updates its database/cache.
    #[test]
    #[ignore = "set BOKHEIM_PDF_SURVEY_ROOT and BOKHEIM_PDF_SURVEY_OUTPUT for a local corpus audit"]
    fn audit_pdf_cover_extraction() {
        let root = std::path::PathBuf::from(std::env::var_os("BOKHEIM_PDF_SURVEY_ROOT").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("BOKHEIM_PDF_SURVEY_OUTPUT").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        let mut directories = vec![root.clone()];
        let mut paths = Vec::new();
        while let Some(directory) = directories.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if entry.file_type().unwrap().is_dir() {
                    directories.push(entry.path());
                } else if entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) {
                    paths.push(entry.path());
                }
            }
        }
        paths.sort();
        let mut extracted = Vec::new();
        let mut fallback = Vec::new();
        let mut reasons = Vec::new();
        for path in paths {
            let name = path.strip_prefix(&root).unwrap().display().to_string();
            let mut reason = "";
            match extract_with_reason(&mut std::fs::File::open(&path).unwrap(), &mut reason) {
                Some(image) => {
                    image.save(output.join(format!("{:03}.png", extracted.len() + 1))).unwrap();
                    extracted.push(name);
                }
                None => {
                    reasons.push(format!("{reason}\t{name}"));
                    fallback.push(name);
                }
            }
        }
        std::fs::write(output.join("extracted.txt"), extracted.join("\n")).unwrap();
        std::fs::write(output.join("pdfium-fallback.txt"), fallback.join("\n")).unwrap();
        std::fs::write(output.join("fallback-reasons.tsv"), reasons.join("\n")).unwrap();
        println!("Direct extraction: {}; PDFium fallback: {}", extracted.len(), fallback.len());
    }
}
