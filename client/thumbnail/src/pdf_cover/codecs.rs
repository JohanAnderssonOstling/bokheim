use super::*;

pub(super) struct Payload {
    pub bytes: Vec<u8>,
    pub filter: Vec<u8>,
    pub parameters: Dictionary,
}

/// Peel ordinary PDF filters in order, retaining the final image codec and its
/// own DecodeParms. A parameter array belongs to individual filters, not all of them.
pub(super) fn payload(document: &Document, stream: &Stream) -> Option<Payload> {
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
    let parameters = match stream.dict.get(b"DecodeParms") {
        Ok(value) => Some(resolve(document, value)?),
        Err(_) => None,
    };
    let mut bytes = stream.content.clone();
    let limit = usize::try_from(MAX_COVER_IMAGE_PIXELS).ok()?.checked_mul(4)?.checked_add(MAX_COVER_IMAGE_DIMENSION as usize)?;
    if bytes.len() > limit {
        return None;
    }
    for (index, filter) in filters.iter().enumerate() {
        let parameter = match parameters {
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
        if matches!(*filter, b"DCTDecode" | b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode") {
            if index + 1 != filters.len() || bytes.len() > MAX_COVER_IMAGE_BYTES {
                return None;
            }
            return Some(Payload { bytes, filter: filter.to_vec(), parameters });
        }
        if !matches!(*filter, b"FlateDecode" | b"LZWDecode" | b"ASCII85Decode") {
            return None;
        }
        let mut dictionary = Dictionary::new();
        dictionary.set("Filter", Object::Name(filter.to_vec()));
        dictionary.set("DecodeParms", parameters);
        bytes = Stream::new(dictionary, bytes).get_plain_content_with_limit(limit).ok()?;
    }
    Some(Payload { bytes, filter: Vec::new(), parameters: Dictionary::new() })
}

pub(super) fn jpeg(payload: &Payload, width: u32, height: u32, channels: usize, reason: &mut &'static str) -> Option<Vec<u8>> {
    let limit = usize::try_from(u64::from(width) * u64::from(height)).ok()?.checked_mul(channels)?;
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(&payload.bytes));
    decoder.set_max_decoding_buffer_size(limit);
    *reason = "JPEG header parsing";
    decoder.read_info().ok()?;
    *reason = "JPEG header dimensions";
    let info = decoder.info()?;
    if (u32::from(info.width), u32::from(info.height), info.pixel_format.pixel_bytes()) != (width, height, channels) {
        return None;
    }
    *reason = "JPEG ColorTransform parameter";
    if let Ok(transform) = payload.parameters.get(b"ColorTransform") {
        let transform = match (transform.as_i64().ok()?, channels) {
            (0, 3) => jpeg_decoder::ColorTransform::RGB,
            (0, 4) => jpeg_decoder::ColorTransform::CMYK,
            (1, 3) => jpeg_decoder::ColorTransform::YCbCr,
            (1, 4) => jpeg_decoder::ColorTransform::YCCK,
            _ => return None,
        };
        decoder.set_color_transform(transform);
    }
    *reason = "JPEG sample decoding";
    let mut samples = decoder.decode().ok()?;
    // Undo jpeg-decoder's CMYK polarity normalization before PDF /Decode.
    if channels == 4 {
        for sample in &mut samples {
            *sample = 255 - *sample;
        }
    }
    Some(samples)
}

pub(super) struct Jpx {
    pub samples: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
    pub space: color::Space,
}
pub(super) fn jpeg2000(payload: &Payload, width: u32, height: u32) -> Option<Jpx> {
    let image = hayro_jpeg2000::Image::new(&payload.bytes, &Default::default()).ok()?;
    if (image.width(), image.height()) != (width, height) {
        return None;
    }
    let space = match image.color_space() {
        hayro_jpeg2000::ColorSpace::Gray => color::Space::Device(1),
        hayro_jpeg2000::ColorSpace::RGB => color::Space::Device(3),
        hayro_jpeg2000::ColorSpace::CMYK => color::Space::Device(4),
        hayro_jpeg2000::ColorSpace::Icc { profile, num_channels } => color::Space::icc(profile, usize::from(*num_channels))?,
        hayro_jpeg2000::ColorSpace::Unknown { .. } => return None,
    };
    let channels = space.channels();
    let mut context = hayro_jpeg2000::DecoderContext::default();
    let decoded = image.decode(&mut context).ok()?;
    let data = decoded.data_u8();
    let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
    if data.len() != pixels.checked_mul(channels + usize::from(image.has_alpha()))? {
        return None;
    }
    if image.has_alpha() {
        let mut samples = Vec::with_capacity(pixels * channels);
        let mut alpha = Vec::with_capacity(pixels);
        for pixel in data.chunks_exact(channels + 1) {
            samples.extend_from_slice(&pixel[..channels]);
            alpha.push(pixel[channels]);
        }
        Some(Jpx { samples, alpha: Some(alpha), space })
    } else {
        Some(Jpx { samples: data, alpha: None, space })
    }
}

/// The callbacks cannot overfill the expected raster, even on malformed input.
struct Bilevel {
    samples: Vec<u8>,
    width: usize,
    height: usize,
    row: usize,
    column: usize,
    invalid: bool,
}
impl Bilevel {
    fn new(width: u32, height: u32) -> Option<Self> {
        let pixels = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        Some(Self { samples: Vec::with_capacity(pixels), width: width as usize, height: height as usize, row: 0, column: 0, invalid: false })
    }
    fn push(&mut self, white: bool, count: usize) {
        if self.row >= self.height || count > self.width.saturating_sub(self.column) {
            self.invalid = true;
            return;
        }
        self.samples.resize(self.samples.len() + count, if white { 255 } else { 0 });
        self.column += count;
    }
    fn next(&mut self) {
        if self.column != self.width {
            self.invalid = true;
        }
        self.row = self.row.saturating_add(1);
        self.column = 0;
    }
    fn finish(self) -> Option<Vec<u8>> {
        (!self.invalid && self.row == self.height && self.column == 0 && self.samples.len() == self.width * self.height).then_some(self.samples)
    }
}
impl hayro_ccitt::Decoder for Bilevel {
    fn push_pixel(&mut self, white: bool) {
        self.push(white, 1);
    }
    fn push_pixel_chunk(&mut self, white: bool, chunk_count: u32) {
        self.push(white, (chunk_count as usize).saturating_mul(8));
    }
    fn next_line(&mut self) {
        self.next();
    }
}
impl hayro_jbig2::Decoder for Bilevel {
    fn push_pixel(&mut self, black: bool) {
        self.push(!black, 1);
    }
    fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
        self.push(!black, (chunk_count as usize).saturating_mul(8));
    }
    fn next_line(&mut self) {
        self.next();
    }
}
fn integer(parameters: &Dictionary, key: &[u8], default: i64) -> Option<i64> {
    match parameters.get(key) {
        Ok(value) => value.as_i64().ok(),
        Err(_) => Some(default),
    }
}
fn boolean(parameters: &Dictionary, key: &[u8], default: bool) -> Option<bool> {
    match parameters.get(key) {
        Ok(value) => value.as_bool().ok(),
        Err(_) => Some(default),
    }
}
pub(super) fn ccitt(payload: &Payload, width: u32, height: u32) -> Option<Vec<u8>> {
    let p = &payload.parameters;
    if integer(p, b"Columns", 1728)? != i64::from(width) {
        return None;
    }
    let rows = integer(p, b"Rows", 0)?;
    if rows != 0 && rows != i64::from(height) {
        return None;
    }
    let k = integer(p, b"K", 0)?;
    let encoding = if k < 0 {
        hayro_ccitt::EncodingMode::Group4
    } else if k == 0 {
        hayro_ccitt::EncodingMode::Group3_1D
    } else {
        hayro_ccitt::EncodingMode::Group3_2D { k: u32::try_from(k).ok()? }
    };
    let settings = hayro_ccitt::DecodeSettings {
        columns: width,
        rows: height,
        encoding,
        end_of_block: boolean(p, b"EndOfBlock", true)?,
        end_of_line: boolean(p, b"EndOfLine", false)?,
        rows_are_byte_aligned: boolean(p, b"EncodedByteAlign", false)?,
        // PDF's default sample polarity is white=1, unlike BlackIs1=true.
        invert_black: boolean(p, b"BlackIs1", false)?,
    };
    let mut raster = Bilevel::new(width, height)?;
    hayro_ccitt::decode(&payload.bytes, &mut raster, &mut hayro_ccitt::DecoderContext::new(settings)).ok()?;
    raster.finish()
}
pub(super) fn jbig2(document: &Document, payload: &Payload, width: u32, height: u32) -> Option<Vec<u8>> {
    let globals = match payload.parameters.get(b"JBIG2Globals") {
        Ok(value) => Some(resolve(document, value)?.as_stream().ok()?.get_plain_content_with_limit(MAX_COVER_IMAGE_BYTES).ok()?),
        Err(_) => None,
    };
    let image = hayro_jbig2::Image::new_embedded(&payload.bytes, globals.as_deref()).ok()?;
    if (image.width(), image.height()) != (width, height) {
        return None;
    }
    let mut raster = Bilevel::new(width, height)?;
    image.decode(&mut raster).ok()?;
    raster.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    #[test]
    fn ccitt_decodes_non_byte_aligned_rows_and_both_sample_polarities() {
        for (k, bytes) in [(0, include_bytes!("../../tests/fixtures/checkerboard-group3.bin").as_slice()), (-1, include_bytes!("../../tests/fixtures/checkerboard-group4.bin").as_slice())] {
            for invert in [false, true] {
                let payload = Payload { bytes: bytes.to_vec(), filter: b"CCITTFaxDecode".to_vec(), parameters: dictionary! {"K"=>k,"Columns"=>9,"Rows"=>3,"BlackIs1"=>invert} };
                let samples = ccitt(&payload, 9, 3).unwrap();
                for y in 0..3 {
                    for x in 0..9 {
                        // libtiff encoded BlackIsZero samples, so runs carry the
                        // opposite convention from PDF's default white=1 output.
                        assert_eq!(samples[y * 9 + x], if ((x + y) % 2 == 0) ^ !invert { 255 } else { 0 });
                    }
                }
            }
        }
    }
    fn segment(number: u32, kind: u8, data: &[u8]) -> Vec<u8> {
        let mut bytes = number.to_be_bytes().to_vec();
        bytes.extend([kind, 0, 1]); // no references, one-byte page association
        bytes.extend((data.len() as u32).to_be_bytes());
        bytes.extend(data);
        bytes
    }
    #[test]
    fn embedded_jbig2_checks_dimensions_and_preserves_black_white_polarity() {
        for black in [false, true] {
            let mut info = Vec::new();
            info.extend(9_u32.to_be_bytes());
            info.extend(3_u32.to_be_bytes());
            info.extend([0; 8]);
            info.push(if black { 4 } else { 0 });
            info.extend([0, 0]);
            let mut bytes = segment(1, 48, &info);
            bytes.extend(segment(2, 49, &[]));
            let payload = Payload { bytes, filter: b"JBIG2Decode".to_vec(), parameters: Dictionary::new() };
            assert_eq!(jbig2(&Document::new(), &payload, 9, 3).unwrap(), vec![if black { 0 } else { 255 }; 27]);
            assert!(jbig2(&Document::new(), &payload, 8, 3).is_none());
        }
    }
}
