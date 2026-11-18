//! Compact page columns: a single element broadcasts; missing rotations mean zero.
use super::{Codec, Encoder, Field, Kind, PdfStoredPage, Schema, WireError, invalid, limit_exceeded};
use prost::Message;

const MAX_PAGES: usize = 32767;

#[derive(Clone, PartialEq, Message)]
pub(super) struct PageColumnsProto {
    #[prost(uint32, tag = "1")]
    count: u32,
    #[prost(bytes, tag = "2")]
    widths: Vec<u8>,
    #[prost(bytes, tag = "3")]
    heights: Vec<u8>,
    #[prost(bytes, tag = "4")]
    rotations: Vec<u8>,
}
impl Schema for PageColumnsProto {
    fn fields() -> &'static [Field] {
        &[
            Field { tag: 1, kind: Kind::Varint(MAX_PAGES as u64), count: 1 },
            Field { tag: 2, kind: Kind::Bytes(MAX_PAGES * 4), count: 1 },
            Field { tag: 3, kind: Kind::Bytes(MAX_PAGES * 4), count: 1 },
            Field { tag: 4, kind: Kind::Bytes(MAX_PAGES * 2), count: 1 },
        ]
    }
}

fn column_len<T: PartialEq>(pages: &[PdfStoredPage], value: impl Fn(&PdfStoredPage) -> T) -> usize {
    match pages.first() {
        Some(first) if pages.iter().all(|page| value(page) == value(first)) => 1,
        _ => pages.len(),
    }
}

impl Codec for Vec<PdfStoredPage> {
    type Proto = PageColumnsProto;
    fn encode_fields(&self, encoder: &mut Encoder<'_>) -> Result<(), WireError> {
        if self.len() > MAX_PAGES {
            return Err(limit_exceeded("too many PDF pages"));
        }
        encoder.varint(1, self.len() as u64)?;
        if self.is_empty() {
            return Ok(());
        }
        let widths = column_len(self, |page| page.width.to_bits());
        let heights = column_len(self, |page| page.height.to_bits());
        let rotations = column_len(self, |page| page.rotation);
        encoder.byte_chunks(2, self[..widths].iter().map(|page| page.width.to_le_bytes()))?;
        encoder.byte_chunks(3, self[..heights].iter().map(|page| page.height.to_le_bytes()))?;
        if rotations != 1 || self[0].rotation != 0 {
            encoder.byte_chunks(4, self[..rotations].iter().map(|page| page.rotation.to_le_bytes()))?;
        }
        Ok(())
    }
    fn from_proto(value: Self::Proto) -> Result<Self, WireError> {
        let count = value.count as usize;
        let valid = |bytes: &[u8], width: usize| bytes.len() == count * width || (count > 0 && bytes.len() == width);
        // Validate all cardinalities before allocating the expanded page list.
        if count > MAX_PAGES || !valid(&value.widths, 4) || !valid(&value.heights, 4) || (!value.rotations.is_empty() && !valid(&value.rotations, 2)) {
            return Err(invalid("invalid PDF page column lengths"));
        }
        fn element<const N: usize>(bytes: &[u8], index: usize) -> [u8; N] {
            let offset = if bytes.len() == N { 0 } else { index * N };
            let mut result = [0; N];
            result.copy_from_slice(&bytes[offset..offset + N]);
            result
        }
        Ok((0..count)
            .map(|index| PdfStoredPage {
                width: f32::from_le_bytes(element(&value.widths, index)),
                height: f32::from_le_bytes(element(&value.heights, index)),
                rotation: if value.rotations.is_empty() { 0 } else { u16::from_le_bytes(element(&value.rotations, index)) },
            })
            .collect())
    }
}
