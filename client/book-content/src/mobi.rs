//! MOBI text decoding and virtual document preparation.

use crate::book::{PreparedText, ResourceSource};
use crate::metadata::{CreatorRole, IdentifierScheme, MetadataCredit, MetadataDate, MetadataIdentifier, MetadataProperty, MetadataSubject};
use crate::resources::{MAX_BINARY_RESOURCE_BYTES, MAX_PREPARED_TEXT_BYTES, PreparedResources};
use crate::source::{ReaderFactory, Source};
use crate::{BookReader, DeclaredMetadata, ReadingBook};
use futures_lite::io::{AsyncReadExt, AsyncSeekExt};
mod navigation;
use ::mobi::HuffmanDecoder;
use ::mobi::headers::{Compression, Encryption, TextEncoding};
pub use ::mobi::{MobiMetadata, headers::ExthRecord};

use std::io::{self, Read, SeekFrom};
use std::ops::Range;
use std::sync::Arc;

pub(crate) struct MobiContainer {
    metadata: MobiMetadata,
    source: Source,
    length: u64,
    extra_data_flags: u32,
}

/// Read only the headers and EXTH fields, including for non-seekable imports.
pub fn read_metadata(reader: impl Read) -> io::Result<DeclaredMetadata> {
    read_headers(reader).map(|metadata| declared_metadata(&metadata))
}

fn read_headers(reader: impl Read) -> io::Result<MobiMetadata> {
    let metadata = MobiMetadata::from_read(reader).map_err(io::Error::other)?;
    if metadata.encryption() != Encryption::No {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "DRM-protected MOBI books are not supported"));
    }
    Ok(metadata)
}

/// Project MOBI header fields into the same declared metadata as EPUB.
/// Identifier validation, name cleanup, and classification policy belong to
/// the library's common metadata projection.
pub fn declared_metadata(raw: &MobiMetadata) -> DeclaredMetadata {
    let mut metadata = DeclaredMetadata { title: raw.title(), description: raw.description().unwrap_or_default(), ..Default::default() };
    for (key, values) in raw.exth.records() {
        if !matches!(
            key,
            ExthRecord::Title
                | ExthRecord::Author
                | ExthRecord::Publisher
                | ExthRecord::Imprint
                | ExthRecord::Description
                | ExthRecord::Isbn
                | ExthRecord::Asin
                | ExthRecord::Subject
                | ExthRecord::Subjectcode
                | ExthRecord::Source
                | ExthRecord::Rights
                | ExthRecord::PublishDate
                | ExthRecord::Language
                | ExthRecord::Contributor
                | ExthRecord::Type
        ) {
            continue;
        }
        let source = format!("mobi:exth:{}", key.position());
        for value in values.into_iter().filter(|value| !value.trim().is_empty()) {
            metadata.properties.push(MetadataProperty { id: None, property: source.clone(), value: value.clone(), refines: None, scheme: None, language: None });
            match key {
                ExthRecord::Isbn | ExthRecord::Asin => metadata.identifiers.push(MetadataIdentifier { value, scheme: if *key == ExthRecord::Isbn { IdentifierScheme::ISBN } else { IdentifierScheme::ASIN }, identifier_types: Vec::new() }),
                ExthRecord::Author => metadata.creators.push(MetadataCredit::author(value)),
                ExthRecord::Contributor => metadata.contributors.push(MetadataCredit { id: None, name: value, file_as: None, role: CreatorRole::contributor() }),
                ExthRecord::Publisher | ExthRecord::Imprint => metadata.publishers.push(value),
                ExthRecord::PublishDate => metadata.dates.push(MetadataDate { source: source.clone(), id: None, value, event: None }),
                ExthRecord::Language => metadata.languages.push(value),
                ExthRecord::Subject | ExthRecord::Subjectcode => metadata.subjects.push(MetadataSubject { id: None, value, source: source.clone(), authority: None, code: None }),
                ExthRecord::Rights => metadata.rights.push(value),
                ExthRecord::Source => metadata.sources.push(value),
                ExthRecord::Type => metadata.types.push(value),
                _ => {}
            }
        }
    }
    metadata
}

impl MobiContainer {
    pub(crate) async fn open(mut source: Source) -> io::Result<Self> {
        const PDB_HEADER_BYTES: usize = 78;
        const MAX_HEADER_BYTES: u64 = 8 * 1024 * 1024;
        let length = source.reader.seek(SeekFrom::End(0)).await?;
        source.reader.seek(SeekFrom::Start(0)).await?;
        let mut prefix = vec![0; PDB_HEADER_BYTES];
        source.reader.read_exact(&mut prefix).await?;
        let count = u16::from_be_bytes(prefix[76..78].try_into().unwrap()) as usize;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MOBI contains no records"));
        }
        prefix.resize(PDB_HEADER_BYTES + count * 8 + 2, 0);
        source.reader.read_exact(&mut prefix[PDB_HEADER_BYTES..]).await?;
        let header_end = if count > 1 { u32::from_be_bytes(prefix[86..90].try_into().unwrap()) as u64 } else { length };
        if header_end < prefix.len() as u64 || header_end > length || header_end > MAX_HEADER_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid MOBI header bounds"));
        }
        let directory_end = prefix.len();
        prefix.resize(header_end as usize, 0);
        source.reader.read_exact(&mut prefix[directory_end..]).await?;
        let metadata = read_headers(prefix.as_slice())?;
        if metadata.records.records.iter().any(|record| u64::from(record.offset) > length) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MOBI record exceeds the source"));
        }
        let extra_data_flags = mobi_extra_record_data_flags(&metadata, &prefix).map_err(io::Error::other)?;
        Ok(Self { metadata, source, length, extra_data_flags })
    }

    pub(crate) fn metadata(&self) -> DeclaredMetadata {
        declared_metadata(&self.metadata)
    }

    fn image_uri(&self, index: usize) -> Option<String> {
        let first = self.metadata.mobi.first_image_index as usize;
        let relative = index.checked_sub(first)?.checked_add(1)?;
        self.metadata.records.records.get(index)?;
        Some(format!("mobi/resources/{relative}"))
    }

    pub(crate) fn preferred_covers(&self) -> Vec<String> {
        [ExthRecord::ThumbOffset, ExthRecord::CoverOffset]
            .into_iter()
            .filter_map(|key| {
                let bytes: [u8; 4] = self.metadata.exth_record(key)?.first()?.as_slice().try_into().ok()?;
                let index = (self.metadata.mobi.first_image_index as usize).checked_add(u32::from_be_bytes(bytes) as usize)?;
                self.image_uri(index)
            })
            .collect()
    }

    pub(crate) fn image_resources(&self) -> Vec<String> {
        (self.metadata.mobi.first_image_index as usize..self.metadata.records.records.len()).filter_map(|index| self.image_uri(index)).collect()
    }

    pub(crate) async fn read_resource(&mut self, uri: &str, limit: u64) -> io::Result<Vec<u8>> {
        let range = image_resource_range(&self.metadata, self.length, uri, limit)?;
        read_record(self.source.reader.as_mut(), range).await
    }

    async fn record(&mut self, index: usize, limit: u64) -> io::Result<Vec<u8>> {
        let range = record_range(&self.metadata, self.length, index, limit)?;
        read_record(self.source.reader.as_mut(), range).await
    }

    async fn reading_text(&mut self) -> io::Result<navigation::ReadingText> {
        if u64::from(self.metadata.palmdoc.text_length) > MAX_PREPARED_TEXT_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MOBI text exceeds the prepared text limit"));
        }
        let mut decoder = TextDecoder::new(&self.metadata, self.extra_data_flags, MAX_PREPARED_TEXT_BYTES as usize).map_err(io::Error::other)?;
        if self.metadata.compression() == Compression::Huff {
            let range = huff_record_bounds(&self.metadata).map_err(io::Error::other)?;
            let mut records = Vec::new();
            let mut remaining = 16 * 1024 * 1024;
            for index in range {
                let bytes = self.record(index, remaining).await?;
                remaining -= bytes.len() as u64;
                records.push(bytes);
            }
            decoder.load_huff(&records).map_err(io::Error::other)?;
        }
        let (start, end) = mobi_text_record_bounds(self.metadata.mobi.first_content_record, self.metadata.palmdoc.record_count, self.metadata.records.records.len()).map_err(io::Error::other)?;
        for index in start..end {
            let record = self.record(index, 64 * 1024).await?;
            decoder.decode_record(&record).map_err(io::Error::other)?;
            if decoder.complete() {
                break;
            }
        }
        if !decoder.complete() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "MOBI text is shorter than its declared length"));
        }
        let text = decoder.finish(true).map_err(io::Error::other)?;
        let entries = navigation::index::read(self).await.ok().flatten();
        navigation::ReadingText::new(&self.metadata, text, entries)
    }

    pub(crate) async fn toc(&mut self) -> io::Result<Option<Vec<crate::TocEntry>>> {
        self.reading_text().await.map(|text| text.toc())
    }

    pub(crate) async fn prepare_text(&mut self) -> io::Result<PreparedText> {
        self.reading_text().await?.prepare()
    }

    pub(crate) async fn prepare_reading(mut self) -> io::Result<ReadingBook> {
        let text = self.prepare_text().await?;
        let source = ResourceSource::Mobi(Arc::new(MobiResources { metadata: self.metadata, length: self.length, factory: self.source.into_factory().await? }));
        Ok(text.into_reading(source))
    }
}

/// Immutable record directory and a pinned source, like EPUB's ZIP directory.
pub(crate) struct MobiResources {
    metadata: MobiMetadata,
    length: u64,
    factory: ReaderFactory,
}

impl MobiResources {
    pub(crate) fn languages(&self) -> Vec<String> {
        declared_metadata(&self.metadata).languages
    }

    pub(crate) async fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        let range = image_resource_range(&self.metadata, self.length, uri, MAX_BINARY_RESOURCE_BYTES)?;
        read_record(self.factory.reader().as_mut(), range).await
    }
}

async fn read_record(reader: &mut dyn BookReader, range: Range<u64>) -> io::Result<Vec<u8>> {
    reader.seek(SeekFrom::Start(range.start)).await?;
    let mut bytes = vec![0; usize::try_from(range.end - range.start).map_err(io::Error::other)?];
    reader.read_exact(&mut bytes).await?;
    Ok(bytes)
}

/// Cover extraction and lazy reading share URI mapping, bounds, and byte limits.
fn image_resource_range(metadata: &MobiMetadata, length: u64, uri: &str, limit: u64) -> io::Result<Range<u64>> {
    let relative = uri.strip_prefix("mobi/resources/").and_then(|index| index.parse::<usize>().ok()).and_then(|index| index.checked_sub(1));
    let index = relative.and_then(|relative| (metadata.mobi.first_image_index as usize).checked_add(relative)).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown MOBI resource"))?;
    record_range(metadata, length, index, limit)
}

fn record_range(metadata: &MobiMetadata, length: u64, index: usize, limit: u64) -> io::Result<Range<u64>> {
    let records = &metadata.records.records;
    let start = records.get(index).map(|record| u64::from(record.offset)).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown MOBI record"))?;
    let end = records.get(index + 1).map_or(length, |record| u64::from(record.offset));
    if start >= end || end > length {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid MOBI resource bounds"));
    }
    if end - start > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "MOBI resource exceeds the byte limit"));
    }
    Ok(start..end)
}

/// Bounded frontmatter decoding for evidence extraction. Uses the same record
/// bounds, trailer handling, PalmDOC decoder, and encoding as reading.
pub fn inspect_text(reader: impl Read) -> io::Result<(DeclaredMetadata, String)> {
    const MAX_INPUT: usize = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    reader.take((MAX_INPUT + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_INPUT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "MOBI exceeds the 16 MiB inspection limit"));
    }
    let metadata = read_headers(bytes.as_slice())?;
    let text = decode_text(&metadata, &bytes, 32, 128 * 1024).map_err(io::Error::other)?;
    Ok((declared_metadata(&metadata), text))
}

fn huff_record_bounds(metadata: &MobiMetadata) -> Result<Range<usize>, String> {
    let start = metadata.mobi.first_huff_record as usize;
    let count = metadata.mobi.huff_record_count as usize;
    let end = start.checked_add(count).ok_or("MOBI HUFF/CDIC record range overflows")?;
    if start == 0 || count < 2 || count > 128 || end > metadata.records.records.len() {
        return Err("MOBI HUFF/CDIC record bounds are invalid".to_owned());
    }
    Ok(start..end)
}

/// The same bounded, record-oriented decoder serves reading and frontmatter.
struct TextDecoder {
    compression: Compression,
    encoding: TextEncoding,
    flags: u32,
    limit: usize,
    decoded: Vec<u8>,
    huff: Option<HuffmanDecoder>,
}

impl TextDecoder {
    fn new(metadata: &MobiMetadata, flags: u32, byte_limit: usize) -> Result<Self, String> {
        let length = metadata.palmdoc.text_length as usize;
        if length > 512 * 1024 * 1024 {
            return Err("MOBI declares an excessive uncompressed text size".to_owned());
        }
        let limit = length.min(byte_limit);
        Ok(Self { compression: metadata.compression(), encoding: metadata.text_encoding(), flags, limit, decoded: Vec::with_capacity(limit), huff: None })
    }

    fn load_huff(&mut self, records: &[Vec<u8>]) -> Result<(), String> {
        let records = records.iter().map(Vec::as_slice).collect::<Vec<_>>();
        self.huff = Some(HuffmanDecoder::new(&records, 64 * 1024 * 1024).map_err(|error| error.to_string())?);
        Ok(())
    }

    fn decode_record(&mut self, record: &[u8]) -> Result<(), String> {
        let content = trim_mobi_trailing_data(record, self.flags)?;
        match self.compression {
            Compression::No => {
                let remaining = self.limit - self.decoded.len();
                self.decoded.extend_from_slice(&content[..content.len().min(remaining)]);
            }
            Compression::PalmDoc => palmdoc_decompress_into(content, &mut self.decoded, self.limit)?,
            Compression::Huff => self.huff.as_mut().ok_or("MOBI HUFF/CDIC tables are missing")?.decode_into(content, &mut self.decoded, self.limit).map_err(|error| error.to_string())?,
        }
        Ok(())
    }

    fn complete(&self) -> bool {
        self.decoded.len() == self.limit
    }

    fn finish(self, strict_utf8: bool) -> Result<String, String> {
        match self.encoding {
            TextEncoding::CP1252 => Ok(encoding_rs::WINDOWS_1252.decode_without_bom_handling(&self.decoded).0.into_owned()),
            TextEncoding::UTF8 if strict_utf8 => String::from_utf8(self.decoded).map_err(|error| error.to_string()),
            _ => Ok(String::from_utf8_lossy(&self.decoded).into_owned()),
        }
    }
}

fn decode_text(metadata: &MobiMetadata, bytes: &[u8], record_limit: usize, byte_limit: usize) -> Result<String, String> {
    let flags = mobi_extra_record_data_flags(metadata, bytes)?;
    let mut decoder = TextDecoder::new(metadata, flags, byte_limit)?;
    let record = |index, limit| {
        let range = record_range(metadata, bytes.len() as u64, index, limit).map_err(|error| error.to_string())?;
        Ok::<_, String>(&bytes[range.start as usize..range.end as usize])
    };
    if metadata.compression() == Compression::Huff {
        let mut records = Vec::new();
        let mut remaining = 16 * 1024 * 1024;
        for index in huff_record_bounds(metadata)? {
            let bytes = record(index, remaining)?;
            remaining -= bytes.len() as u64;
            records.push(bytes.to_vec());
        }
        decoder.load_huff(&records)?;
    }
    let (start, end) = mobi_text_record_bounds(metadata.mobi.first_content_record, metadata.palmdoc.record_count, metadata.records.records.len())?;
    for index in start..end.min(start.saturating_add(record_limit)) {
        decoder.decode_record(record(index, 64 * 1024)?)?;
        if decoder.complete() {
            break;
        }
    }
    decoder.finish(false)
}

fn mobi_extra_record_data_flags(metadata: &MobiMetadata, source_bytes: &[u8]) -> Result<u32, String> {
    const PALMDOC_HEADER_LEN: usize = 16;
    const EXTRA_DATA_FLAGS_OFFSET: usize = 0xe0;
    let Some(record_zero) = metadata.records.records.first() else {
        return Err("MOBI contains no header record".to_owned());
    };
    if metadata.mobi.header_length < (EXTRA_DATA_FLAGS_OFFSET + 4) as u32 {
        return Ok(0);
    }
    let offset = usize::try_from(record_zero.offset).ok().and_then(|offset| offset.checked_add(PALMDOC_HEADER_LEN + EXTRA_DATA_FLAGS_OFFSET)).ok_or_else(|| "MOBI extra-data flags offset overflows".to_owned())?;
    let bytes = source_bytes.get(offset..offset + 4).ok_or_else(|| "MOBI extra-data flags are truncated".to_owned())?;
    Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
}

fn trim_mobi_trailing_data(mut data: &[u8], flags: u32) -> Result<&[u8], String> {
    let mut trailing_entries = flags >> 1;
    while trailing_entries != 0 {
        if trailing_entries & 1 != 0 {
            let size = mobi_trailing_entry_size(data)?;
            data = data.get(..data.len() - size).ok_or_else(|| "MOBI trailing-data entry exceeds its text record".to_owned())?;
        }
        trailing_entries >>= 1;
    }
    if flags & 1 != 0 {
        let last = *data.last().ok_or_else(|| "MOBI multibyte trailer is missing".to_owned())?;
        let size = usize::from(last & 0x03) + 1;
        if size > data.len() {
            return Err("MOBI multibyte trailer exceeds its text record".to_owned());
        }
        data = &data[..data.len() - size];
    }
    Ok(data)
}

fn mobi_trailing_entry_size(data: &[u8]) -> Result<usize, String> {
    let mut result = 0usize;
    let mut bit_position = 0u32;
    for byte in data.iter().rev().take(4) {
        result |= usize::from(byte & 0x7f) << bit_position;
        if byte & 0x80 != 0 {
            return (result > 0 && result <= data.len()).then_some(result).ok_or_else(|| "invalid MOBI trailing-data entry size".to_owned());
        }
        bit_position += 7;
    }
    Err("unterminated MOBI trailing-data entry size".to_owned())
}

fn mobi_text_record_bounds(first_content_record: u16, record_count: u16, total_records: usize) -> Result<(usize, usize), String> {
    let record_count = usize::from(record_count);
    if record_count == 0 {
        return Err("MOBI contains no text records".to_owned());
    }
    let declared_start = usize::from(first_content_record);
    let declared_is_usable = first_content_record != u16::MAX && declared_start > 0 && declared_start.checked_add(record_count).is_some_and(|end| end <= total_records);
    let start = if declared_is_usable { declared_start } else { 1 };
    let end = start.checked_add(record_count).ok_or_else(|| "MOBI text record range overflows".to_owned())?;
    if end > total_records {
        return Err(format!("MOBI declares {record_count} text records but contains only {} data records", total_records.saturating_sub(start)));
    }
    Ok((start, end))
}

fn palmdoc_decompress_into(data: &[u8], output: &mut Vec<u8>, output_limit: usize) -> Result<(), String> {
    let record_start = output.len();
    let mut position = 0;
    while position < data.len() && output.len() < output_limit {
        let byte = data[position];
        position += 1;
        match byte {
            0 | 0x09..=0x7f => output.push(byte),
            1..=8 => {
                let literal_count = usize::from(byte);
                let end = position.checked_add(literal_count).filter(|end| *end <= data.len()).ok_or_else(|| "truncated PalmDOC literal run".to_owned())?;
                let remaining = output_limit - output.len();
                output.extend_from_slice(&data[position..end][..literal_count.min(remaining)]);
                position = end;
            }
            0x80..=0xbf => {
                let next = *data.get(position).ok_or_else(|| "truncated PalmDOC back-reference".to_owned())?;
                position += 1;
                let pair = u16::from_be_bytes([byte, next]);
                let distance = usize::from((pair >> 3) & 0x07ff);
                let length = usize::from((pair & 0x0007) + 3);
                if distance == 0 || distance > output.len() - record_start {
                    return Err("invalid PalmDOC back-reference distance".to_owned());
                }
                for _ in 0..length {
                    if output.len() == output_limit {
                        break;
                    }
                    let source = output.len() - distance;
                    output.push(output[source]);
                }
            }
            _ => {
                if output.len() < output_limit {
                    output.push(b' ');
                }
                if output.len() < output_limit {
                    output.push(byte ^ 0x80);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mobi_unset_content_bounds_fall_back_to_palmdoc_record_count() {
        assert_eq!(mobi_text_record_bounds(u16::MAX, 174, 185), Ok((1, 175)));
        assert_eq!(mobi_text_record_bounds(2, 3, 8), Ok((2, 5)));
        assert!(mobi_text_record_bounds(u16::MAX, 8, 4).is_err());
    }

    #[test]
    fn palmdoc_decoder_rejects_invalid_back_references() {
        let mut output = Vec::new();
        assert!(palmdoc_decompress_into(b"\x80\x18", &mut output, 32).is_err());
    }

    #[test]
    fn mobi_text_record_trailers_are_removed_before_decompression() {
        assert_eq!(trim_mobi_trailing_data(b"text\0\x81", 0b11).unwrap(), b"text");
        assert_eq!(trim_mobi_trailing_data(b"text", 0).unwrap(), b"text");
        assert!(trim_mobi_trailing_data(b"\x81", 0b11).is_err());
    }

    #[test]
    #[ignore = "set BOKHEIM_MOBI_FIXTURE to a DRM-free real-world MOBI file"]
    fn probes_real_mobi_fixture() {
        let path = std::env::var_os("BOKHEIM_MOBI_FIXTURE").map(std::path::PathBuf::from).expect("BOKHEIM_MOBI_FIXTURE is set");
        let bytes = std::fs::read(&path).unwrap();
        let book = futures_lite::future::block_on(async { MobiContainer::open(Source::from_reader(std::io::Cursor::new(bytes))).await.unwrap().prepare_reading().await.unwrap() });
        assert!(!book.document_uris().is_empty());
        let first = String::from_utf8(book.resources.read_bytes(&book.document_uris()[0]).unwrap()).unwrap();
        assert!(first.contains("<body>"));
        assert!(first.len() > 100);
    }
}
