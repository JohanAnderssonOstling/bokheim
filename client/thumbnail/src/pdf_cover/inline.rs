use super::*;
use lopdf::content::{Content, Operation};

/// Split inline images before lopdf's content parser. Length, not an EI search,
/// delimits raw binary samples (which can themselves contain PDF operators).
/// Flate images use the decompressor's consumed-byte count; other codecs fall back.
pub(super) fn operations(bytes: &[u8], document: &Document, resources: &Dictionary) -> Option<Vec<Operation>> {
    let mut output = Vec::new();
    let (mut cursor, mut start) = (0, 0);
    while cursor < bytes.len() {
        let (a, b) = token(bytes, &mut cursor)?;
        if &bytes[a..b] != b"BI" {
            continue;
        }
        output.extend(Content::decode(&bytes[start..a]).ok()?.operations);
        let dict_start = cursor;
        let dict_end = loop {
            let (a, b) = token(bytes, &mut cursor)?;
            if a == b {
                return None;
            }
            if &bytes[a..b] == b"ID" {
                break a;
            }
        };
        let mut encoded = b"<< ".to_vec();
        encoded.extend_from_slice(&bytes[dict_start..dict_end]);
        encoded.extend_from_slice(b" >> dictionary");
        let parsed = Content::decode(&encoded).ok()?;
        if parsed.operations.len() != 1 || parsed.operations[0].operands.len() != 1 {
            return None;
        }
        let mut dict = parsed.operations[0].operands[0].as_dict().ok()?.clone();
        for (short, long) in [("W", "Width"), ("H", "Height"), ("BPC", "BitsPerComponent"), ("CS", "ColorSpace"), ("D", "Decode"), ("IM", "ImageMask"), ("F", "Filter"), ("DP", "DecodeParms")] {
            if let Ok(value) = dict.get(short.as_bytes()) {
                dict.set(long, value.clone());
            }
        }
        if dict.get(b"ImageMask").and_then(Object::as_bool).ok() == Some(true) {
            return None;
        }
        if let Ok(Object::Name(name)) = dict.get(b"ColorSpace") {
            let full = match name.as_slice() {
                b"G" | b"Gray" => b"DeviceGray".as_slice(),
                b"RGB" => b"DeviceRGB",
                b"CMYK" => b"DeviceCMYK",
                _ => name,
            };
            dict.set("ColorSpace", Object::Name(full.to_vec()));
        }
        let width = u32::try_from(dict.get(b"Width").ok()?.as_i64().ok()?).ok()?;
        let height = u32::try_from(dict.get(b"Height").ok()?.as_i64().ok()?).ok()?;
        validate_cover_image_dimensions(width, height).ok()?;
        let bits = usize::try_from(dict.get(b"BitsPerComponent").ok()?.as_i64().ok()?).ok()?;
        if !matches!(bits, 1 | 2 | 4 | 8 | 16) {
            return None;
        }
        let channels = color::resolve_space(document, resources, dict.get(b"ColorSpace").ok()?)?.channels();
        let length = (width as usize).checked_mul(channels)?.checked_mul(bits)?.div_ceil(8).checked_mul(height as usize)?;
        // ID consumes exactly one whitespace separator, with CRLF treated as one.
        if !bytes.get(cursor)?.is_ascii_whitespace() {
            return None;
        }
        if bytes.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor += 2;
        } else {
            cursor += 1;
        }
        let encoded_length = match dict.get(b"Filter") {
            Err(_) => length,
            Ok(Object::Name(filter)) if matches!(filter.as_slice(), b"Fl" | b"FlateDecode") => {
                dict.set("Filter", "FlateDecode");
                let mut decoder = flate2::bufread::ZlibDecoder::new(bytes.get(cursor..)?);
                let mut decoded = Vec::new();
                (&mut decoder).take((length.checked_add(height as usize)?.checked_add(1)?) as u64).read_to_end(&mut decoded).ok()?;
                if decoded.len() > length.checked_add(height as usize)? {
                    return None;
                }
                usize::try_from(decoder.total_in()).ok()?
            }
            _ => return None,
        };
        let end = cursor.checked_add(encoded_length)?;
        let data = bytes.get(cursor..end)?.to_vec();
        cursor = end;
        if !bytes.get(cursor)?.is_ascii_whitespace() {
            return None;
        }
        let (a, b) = token(bytes, &mut cursor)?;
        if &bytes[a..b] != b"EI" {
            return None;
        }
        dict.set("Subtype", "Image");
        output.push(Operation::new("BI", vec![Object::Stream(Stream::new(dict, data))]));
        start = cursor;
    }
    output.extend(Content::decode(&bytes[start..]).ok()?.operations);
    Some(output)
}

// A bounded PDF lexical token, treating strings/arrays/dictionaries as indivisible.
fn token(bytes: &[u8], cursor: &mut usize) -> Option<(usize, usize)> {
    token_inner(bytes, cursor, 0)
}
fn token_inner(bytes: &[u8], cursor: &mut usize, nesting: usize) -> Option<(usize, usize)> {
    if nesting > 32 {
        return None;
    }
    space(bytes, cursor);
    let start = *cursor;
    if start == bytes.len() {
        return Some((start, start));
    }
    match bytes[start] {
        b'(' => {
            *cursor += 1;
            let mut depth = 1;
            while depth > 0 {
                match *bytes.get(*cursor)? {
                    b'\\' => {
                        *cursor += 1;
                        bytes.get(*cursor)?;
                    }
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => (),
                }
                *cursor += 1;
            }
        }
        b'[' | b'<' if bytes[start] == b'[' || bytes.get(start + 1) == Some(&b'<') => {
            let close = if bytes[start] == b'[' { b"]".as_slice() } else { b">>".as_slice() };
            *cursor += if bytes[start] == b'[' { 1 } else { 2 };
            loop {
                space(bytes, cursor);
                if bytes.get(*cursor..*cursor + close.len()) == Some(close) {
                    *cursor += close.len();
                    break;
                }
                let before = *cursor;
                token_inner(bytes, cursor, nesting + 1)?;
                if *cursor == before {
                    return None;
                }
            }
        }
        b'<' => {
            *cursor += 1;
            while *bytes.get(*cursor)? != b'>' {
                *cursor += 1;
            }
            *cursor += 1;
        }
        _ => {
            *cursor += 1;
            while bytes.get(*cursor).is_some_and(|b| !b.is_ascii_whitespace() && *b != 0 && !b"()<>[]{}/%".contains(b)) {
                *cursor += 1;
            }
        }
    }
    Some((start, *cursor))
}

fn space(bytes: &[u8], cursor: &mut usize) {
    while *cursor < bytes.len() {
        if bytes[*cursor].is_ascii_whitespace() || bytes[*cursor] == 0 {
            *cursor += 1;
        } else if bytes[*cursor] == b'%' {
            while *cursor < bytes.len() && !matches!(bytes[*cursor], b'\r' | b'\n') {
                *cursor += 1;
            }
        } else {
            break;
        }
    }
}
