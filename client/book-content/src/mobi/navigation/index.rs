//! Bounded decoding of compiled MOBI NCX (INDX/TAGX/CNCX) records.

use super::super::MobiContainer;
use std::collections::HashMap;
use std::io;

pub(crate) struct Entry {
    pub(super) title: String,
    pub(super) position: usize,
    pub(super) level: usize,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid MOBI navigation index")
}

fn word(bytes: &[u8], offset: usize) -> io::Result<usize> {
    Ok(u32::from_be_bytes(bytes.get(offset..).and_then(|tail| tail.get(..4)).ok_or_else(invalid)?.try_into().unwrap()) as usize)
}

fn integer(bytes: &mut &[u8]) -> io::Result<usize> {
    let mut value = 0usize;
    for _ in 0..5 {
        let (&byte, rest) = bytes.split_first().ok_or_else(invalid)?;
        *bytes = rest;
        value = value.checked_mul(128).and_then(|value| value.checked_add((byte & 0x7f) as usize)).ok_or_else(invalid)?;
        if byte & 0x80 != 0 {
            return Ok(value);
        }
    }
    Err(invalid())
}

fn tags(control_count: usize, definitions: &[[u8; 4]], mut bytes: &[u8]) -> io::Result<HashMap<u8, Vec<usize>>> {
    let controls = bytes.get(..control_count).ok_or_else(invalid)?;
    bytes = &bytes[control_count..];
    let mut control = 0;
    let mut pending = Vec::new();
    for &[tag, values, mask, end] in definitions {
        if end == 1 {
            control += 1;
            continue;
        }
        if mask == 0 {
            continue;
        }
        let value = controls.get(control).ok_or_else(invalid)? & mask;
        if value == 0 {
            continue;
        }
        let (count, length) = if value == mask && mask.count_ones() > 1 { (None, Some(integer(&mut bytes)?)) } else { (Some(if value == mask { 1 } else { usize::from(value >> mask.trailing_zeros()) }), None) };
        pending.push((tag, usize::from(values), count, length));
    }
    let mut result = HashMap::new();
    for (tag, values, count, length) in pending {
        let mut decoded = Vec::new();
        if let Some(count) = count {
            let count = count * values;
            if count > 256 {
                return Err(invalid());
            }
            for _ in 0..count {
                decoded.push(integer(&mut bytes)?);
            }
        } else {
            let length = length.unwrap();
            let mut encoded = bytes.get(..length).ok_or_else(invalid)?;
            bytes = &bytes[length..];
            while !encoded.is_empty() {
                if decoded.len() == 256 {
                    return Err(invalid());
                }
                decoded.push(integer(&mut encoded)?);
            }
        }
        result.insert(tag, decoded);
    }
    Ok(result)
}

pub(crate) async fn read(book: &mut MobiContainer) -> io::Result<Option<Vec<Entry>>> {
    let index = book.metadata.mobi.first_index_record;
    if index == u32::MAX {
        return Ok(None);
    }
    let index = index as usize;
    let header = book.record(index, 4 * 1024 * 1024).await?;
    if !header.starts_with(b"INDX") {
        return Err(invalid());
    }
    let count = word(&header, 24)?;
    let strings_count = word(&header, 52)?;
    if count > 128 || strings_count > 128 {
        return Err(invalid());
    }
    let tag_start = word(&header, 180).ok().filter(|&offset| header.get(offset..).and_then(|tail| tail.get(..4)) == Some(b"TAGX")).or_else(|| header.windows(4).position(|bytes| bytes == b"TAGX")).ok_or_else(invalid)?;
    let tagx = &header[tag_start..];
    let tag_length = word(tagx, 4)?;
    let control_count = word(tagx, 8)?;
    if control_count > 32 || tag_length < 12 || (tag_length - 12) % 4 != 0 {
        return Err(invalid());
    }
    let definitions = tagx.get(12..tag_length).ok_or_else(invalid)?.chunks_exact(4).map(|bytes| bytes.try_into().unwrap()).collect::<Vec<[u8; 4]>>();
    if definitions.len() > 256 {
        return Err(invalid());
    }
    let mut strings = HashMap::new();
    for offset in 0..strings_count {
        let raw = book.record(index + 1 + count + offset, 4 * 1024 * 1024).await?;
        let mut bytes = raw.as_slice();
        while !bytes.is_empty() && strings.len() < 8192 {
            let position = raw.len() - bytes.len() + offset * 0x10000;
            let length = integer(&mut bytes)?;
            let text = bytes.get(..length).ok_or_else(invalid)?;
            bytes = &bytes[length..];
            if length == 0 {
                continue;
            }
            if length > 4096 {
                return Err(invalid());
            }
            let text = match word(&header, 28)? {
                1252 => encoding_rs::WINDOWS_1252.decode_without_bom_handling(text).0.into_owned(),
                _ => String::from_utf8_lossy(text).into_owned(),
            };
            strings.insert(position, text);
        }
    }
    let mut entries = Vec::new();
    for offset in 0..count {
        let bytes = book.record(index + 1 + offset, 4 * 1024 * 1024).await?;
        if !bytes.starts_with(b"INDX") {
            return Err(invalid());
        }
        let table = word(&bytes, 20)?;
        let count = word(&bytes, 24)?;
        if count > 8192 || entries.len() + count > 8192 || bytes.get(table..).and_then(|tail| tail.get(..4)) != Some(b"IDXT") {
            return Err(invalid());
        }
        let offsets = bytes.get(table + 4..table + 4 + count * 2).ok_or_else(invalid)?.chunks_exact(2).map(|bytes| u16::from_be_bytes(bytes.try_into().unwrap()) as usize).chain(std::iter::once(table)).collect::<Vec<_>>();
        for bounds in offsets.windows(2) {
            let entry = bytes.get(bounds[0]..bounds[1]).ok_or_else(invalid)?;
            let name_length = usize::from(*entry.first().ok_or_else(invalid)?);
            let encoded = entry.get(1 + name_length..).ok_or_else(invalid)?;
            let fields = tags(control_count, &definitions, encoded)?;
            let field = |tag| fields.get(&tag).and_then(|values| values.first()).copied();
            let (Some(position), Some(title)) = (field(1), field(3).and_then(|offset| strings.get(&offset))) else { continue };
            let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
            if !title.is_empty() {
                entries.push(Entry { title, position, level: field(4).unwrap_or(0) });
            }
        }
    }
    Ok((!entries.is_empty()).then_some(entries))
}
