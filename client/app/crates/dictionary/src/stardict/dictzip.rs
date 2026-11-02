use std::io::{Read, Seek, SeekFrom};

use flate2::{Decompress, FlushDecompress};

use crate::{MAX_ENTRY_BYTES, Result, read_at};

struct Chunks {
    length: u64,
    offsets: Vec<(u64, usize)>,
}

pub struct Body<R> {
    reader: R,
    size: u64,
    chunks: Option<Chunks>,
    decoded: Option<(usize, Vec<u8>)>,
}

impl<R: Read + Seek> Body<R> {
    pub fn open(mut reader: R, compressed: bool) -> Result<Self> {
        let file_size = reader.seek(SeekFrom::End(0)).map_err(|error| error.to_string())?;
        if !compressed {
            return Ok(Self { reader, size: file_size, chunks: None, decoded: None });
        }
        if file_size < 20 {
            return Err("Truncated dictzip file".into());
        }
        let header = read_at(&mut reader, 0, 12)?;
        if header[..3] != [0x1f, 0x8b, 8] || header[3] & 4 == 0 || header[3] & 0xe0 != 0 {
            return Err("The .dict.dz file must use dictzip random-access compression".into());
        }
        let extra_length = u16::from_le_bytes([header[10], header[11]]) as usize;
        let extra = read_at(&mut reader, 12, extra_length)?;
        let mut cursor = 0;
        let mut table = None;
        while cursor < extra.len() {
            let field = extra.get(cursor..cursor + 4).ok_or("Truncated dictzip extra field")?;
            let length = u16::from_le_bytes([field[2], field[3]]) as usize;
            cursor += 4;
            let data = extra.get(cursor..cursor + length).ok_or("Truncated dictzip chunk table")?;
            if field[..2] == *b"RA" {
                if table.is_some() || data.len() < 6 {
                    return Err("Invalid dictzip chunk table".into());
                }
                let short = |index| u16::from_le_bytes([data[index], data[index + 1]]);
                let count = short(4) as usize;
                if short(0) != 1 || short(2) == 0 || count == 0 || data.len() != 6 + count * 2 {
                    return Err("Unsupported dictzip chunk table".into());
                }
                table = Some((u64::from(short(2)), (0..count).map(|index| short(6 + index * 2) as usize).collect::<Vec<_>>()));
            }
            cursor += length;
        }
        let (length, sizes) = table.ok_or("Missing dictzip random-access table")?;
        let mut offset = 12 + extra_length as u64;
        for flag in [8, 16] {
            if header[3] & flag != 0 {
                let mut count = 0;
                loop {
                    if offset >= file_size - 8 || count == 65536 {
                        return Err("Invalid dictzip header string".into());
                    }
                    let byte = read_at(&mut reader, offset, 1)?[0];
                    offset += 1;
                    count += 1;
                    if byte == 0 {
                        break;
                    }
                }
            }
        }
        if header[3] & 2 != 0 {
            offset += 2;
        }
        let mut offsets = Vec::with_capacity(sizes.len());
        for size in sizes {
            if size == 0 || offset + size as u64 > file_size - 8 {
                return Err("Dictzip chunk points outside its file".into());
            }
            offsets.push((offset, size));
            offset += size as u64;
        }
        let trailer = read_at(&mut reader, file_size - 4, 4)?;
        let size = u64::from(u32::from_le_bytes(trailer.try_into().map_err(|_| "Invalid dictzip trailer")?));
        if size == 0 || size.div_ceil(length) != offsets.len() as u64 {
            return Err("Dictzip size does not match its chunk table".into());
        }
        Ok(Self { reader, size, chunks: Some(Chunks { length, offsets }), decoded: None })
    }

    pub fn len(&self) -> u64 {
        self.size
    }

    pub fn read_entry(&mut self, offset: u64, size: u32) -> Result<Vec<u8>> {
        if size > MAX_ENTRY_BYTES || offset.checked_add(u64::from(size)).is_none_or(|end| end > self.size) {
            return Err("Invalid dictionary entry range".into());
        }
        let Some(chunks) = &self.chunks else {
            return read_at(&mut self.reader, offset, size as usize);
        };
        if size == 0 {
            return Ok(Vec::new());
        }
        let mut output = Vec::with_capacity(size as usize);
        let end = offset + u64::from(size);
        for index in offset / chunks.length..=(end - 1) / chunks.length {
            let chunk_index = index as usize;
            let expected = chunks.length.min(self.size - index * chunks.length) as usize;
            if self.decoded.as_ref().is_none_or(|(cached, _)| *cached != chunk_index) {
                let (file_offset, compressed_size) = chunks.offsets[chunk_index];
                let compressed = read_at(&mut self.reader, file_offset, compressed_size)?;
                let mut decoded = vec![0; expected + 1];
                let mut inflater = Decompress::new(false);
                inflater.decompress(&compressed, &mut decoded, FlushDecompress::Sync).map_err(|error| format!("Invalid dictzip chunk: {error}"))?;
                if inflater.total_out() as usize != expected {
                    return Err("Dictzip chunk has an invalid expanded size".into());
                }
                decoded.truncate(expected);
                self.decoded = Some((chunk_index, decoded));
            }
            let decoded = &self.decoded.as_ref().unwrap().1;
            let start = offset.saturating_sub(index * chunks.length) as usize;
            let stop = (end - index * chunks.length).min(expected as u64) as usize;
            output.extend_from_slice(&decoded[start..stop]);
        }
        Ok(output)
    }
}
