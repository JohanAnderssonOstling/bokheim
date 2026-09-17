use super::*;

/// Never interpret raw PDF string bytes as Unicode whitespace. Validate their
/// font encoding, or prove that the referenced TrueType glyph has no outline.
pub(super) fn blank(document: &Document, resources: &Dictionary, font: Option<&[u8]>, operands: &[Object]) -> Option<bool> {
    let mut strings = Vec::new();
    for operand in operands {
        match operand {
            Object::String(bytes, _) => strings.push(bytes.as_slice()),
            Object::Array(items) => {
                for item in items {
                    match item {
                        Object::String(bytes, _) => strings.push(bytes.as_slice()),
                        Object::Integer(_) | Object::Real(_) => (),
                        _ => return None,
                    }
                }
            }
            Object::Integer(_) | Object::Real(_) => (),
            _ => return None,
        }
    }
    if strings.iter().all(|s| s.is_empty()) {
        return Some(true);
    }
    let fonts = resolve(document, resources.get(b"Font").ok()?)?.as_dict().ok()?;
    let font = resolve(document, fonts.get(font?).ok()?)?.as_dict().ok()?;
    match font.get(b"Subtype").and_then(Object::as_name).ok()? {
        b"Type1" => {
            if font.has(b"FontDescriptor") {
                return Some(false);
            }
            let name = font.get(b"BaseFont").and_then(Object::as_name).ok()?;
            if ![
                b"Helvetica".as_slice(),
                b"Helvetica-Bold",
                b"Helvetica-Oblique",
                b"Helvetica-BoldOblique",
                b"Times-Roman",
                b"Times-Bold",
                b"Times-Italic",
                b"Times-BoldItalic",
                b"Courier",
                b"Courier-Bold",
                b"Courier-Oblique",
                b"Courier-BoldOblique",
            ]
            .contains(&name)
            {
                return Some(false);
            }
            if let Ok(encoding) = font.get(b"Encoding") {
                let encoding = resolve(document, encoding)?;
                if let Ok(name) = encoding.as_name() {
                    if ![b"WinAnsiEncoding".as_slice(), b"MacRomanEncoding", b"StandardEncoding"].contains(&name) {
                        return Some(false);
                    }
                } else {
                    let encoding = encoding.as_dict().ok()?;
                    if let Ok(base) = encoding.get(b"BaseEncoding") {
                        if ![b"WinAnsiEncoding".as_slice(), b"MacRomanEncoding", b"StandardEncoding"].contains(&base.as_name().ok()?) {
                            return Some(false);
                        }
                    }
                    if let Ok(differences) = encoding.get(b"Differences") {
                        let mut code = 0_i64;
                        for entry in resolve(document, differences)?.as_array().ok()? {
                            match entry {
                                Object::Integer(value) => code = *value,
                                Object::Name(name) => {
                                    if code == 32 && name != b"space" {
                                        return Some(false);
                                    }
                                    code = code.checked_add(1)?;
                                }
                                _ => return None,
                            }
                        }
                    }
                }
            }
            Some(strings.iter().all(|s| s.iter().all(|b| *b == 32)))
        }
        b"Type0" => {
            if font.get(b"Encoding").and_then(Object::as_name).ok()? != b"Identity-H" {
                return Some(false);
            }
            let descendants = resolve(document, font.get(b"DescendantFonts").ok()?)?.as_array().ok()?;
            if descendants.len() != 1 {
                return None;
            }
            let descendant = resolve(document, &descendants[0])?.as_dict().ok()?;
            if descendant.get(b"Subtype").and_then(Object::as_name).ok()? != b"CIDFontType2" {
                return Some(false);
            }
            let mapping = match descendant.get(b"CIDToGIDMap") {
                Ok(value) => match resolve(document, value)? {
                    Object::Name(name) if name == b"Identity" => None,
                    Object::Stream(stream) => Some(stream.get_plain_content_with_limit(131072).ok()?),
                    _ => return None,
                },
                Err(_) => None,
            };
            let descriptor = resolve(document, descendant.get(b"FontDescriptor").ok()?)?.as_dict().ok()?;
            let program = resolve(document, descriptor.get(b"FontFile2").ok()?)?.as_stream().ok()?.get_plain_content_with_limit(4 * 1024 * 1024).ok()?;
            let face = ttf_parser::Face::parse(&program, 0).ok()?;
            let loca = face.raw_face().table(ttf_parser::Tag::from_bytes(b"loca"))?;
            let loca = ttf_parser::loca::Table::parse(std::num::NonZeroU16::new(face.number_of_glyphs())?, face.tables().head.index_to_location_format, loca)?;
            for string in strings {
                if string.len() % 2 != 0 {
                    return Some(false);
                }
                for code in string.chunks_exact(2) {
                    let cid = usize::from(u16::from_be_bytes([code[0], code[1]]));
                    let gid = match &mapping {
                        Some(mapping) => {
                            let b = mapping.get(cid * 2..cid * 2 + 2)?;
                            u16::from_be_bytes([b[0], b[1]])
                        }
                        None => u16::try_from(cid).ok()?,
                    };
                    if gid >= face.number_of_glyphs() || gid == u16::MAX {
                        return Some(false);
                    }
                    let empty = match loca {
                        ttf_parser::loca::Table::Short(offsets) => offsets.get(gid)? == offsets.get(gid + 1)?,
                        ttf_parser::loca::Table::Long(offsets) => offsets.get(gid)? == offsets.get(gid + 1)?,
                    };
                    if !empty {
                        return Some(false);
                    }
                }
            }
            Some(true)
        }
        _ => Some(false),
    }
}
